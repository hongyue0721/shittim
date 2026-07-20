//! Embedded, ordered, checksum-protected SQLite migrations.

use crate::{StoreError, StoreErrorCode};
use kernel_contracts::{canonical_json_string, CausationRef, CausationRefKind};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MIGRATION_0003_ASSET_PATH: &str =
    "rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql";
const MIGRATION_0003_ALGORITHM_ID: &str = "shittim.kernel-sqlite.outbox-v1-to-versioned-v1";
const MIGRATION_0003_IMPLEMENTATION_ID: &str =
    "kernel_sqlite::migration::outbox_v1_to_versioned_v1";

#[derive(Debug, Clone, Copy)]
struct LegacySqlMigration {
    version: i64,
    name: &'static str,
    sql: &'static [u8],
}

#[derive(Debug, Clone, Copy)]
struct DescriptorV1Migration {
    version: i64,
    name: &'static str,
    asset_path: &'static str,
    sql: &'static [u8],
    transform: TransformIdentity,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct TransformIdentity {
    algorithm_id: &'static str,
    version: u64,
    implementation_id: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum MigrationDefinition {
    LegacySql(LegacySqlMigration),
    DescriptorV1(DescriptorV1Migration),
}

impl MigrationDefinition {
    const fn version(self) -> i64 {
        match self {
            Self::LegacySql(migration) => migration.version,
            Self::DescriptorV1(migration) => migration.version,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::LegacySql(migration) => migration.name,
            Self::DescriptorV1(migration) => migration.name,
        }
    }
}

const MIGRATIONS: &[MigrationDefinition] = &[
    MigrationDefinition::LegacySql(LegacySqlMigration {
        version: 1,
        name: "initial",
        sql: include_bytes!("../migrations/0001_initial.sql"),
    }),
    MigrationDefinition::LegacySql(LegacySqlMigration {
        version: 2,
        name: "task_repository",
        sql: include_bytes!("../migrations/0002_task_repository.sql"),
    }),
    MigrationDefinition::DescriptorV1(DescriptorV1Migration {
        version: 3,
        name: "versioned_event_outbox",
        asset_path: MIGRATION_0003_ASSET_PATH,
        sql: include_bytes!("../migrations/0003_versioned_event_outbox.sql"),
        transform: TransformIdentity {
            algorithm_id: MIGRATION_0003_ALGORITHM_ID,
            version: 1,
            implementation_id: MIGRATION_0003_IMPLEMENTATION_ID,
        },
    }),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LedgerShape {
    Legacy,
    DescriptorV1,
}

#[derive(Debug)]
struct LedgerRow {
    version: i64,
    name: String,
    checksum: String,
    descriptor_hash: Option<String>,
    descriptor_format_version: Option<i64>,
}

#[derive(Debug, Serialize)]
struct DescriptorV1<'a> {
    descriptor_format_version: u64,
    migration_version: i64,
    name: &'a str,
    sql_assets: [DescriptorSqlAsset<'a>; 1],
    transform: TransformIdentity,
}

#[derive(Debug, Serialize)]
struct DescriptorSqlAsset<'a> {
    path: &'a str,
    sha256: String,
}

pub(crate) fn apply_migrations(connection: &Connection) -> Result<(), StoreError> {
    ensure_migration_table(connection)?;
    verify_applied(connection)?;
    for migration in MIGRATIONS {
        if !is_applied(connection, migration.version())? {
            apply_one(connection, *migration)?;
        }
    }
    Ok(())
}

fn ensure_migration_table(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (\
                version INTEGER PRIMARY KEY CHECK(version > 0),\
                name TEXT NOT NULL UNIQUE CHECK(length(name) > 0),\
                checksum TEXT NOT NULL CHECK(length(checksum) = 64),\
                applied_at TEXT NOT NULL\
            ) WITHOUT ROWID;",
        )
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))
}

fn detect_ledger_shape(connection: &Connection) -> Result<LedgerShape, StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(schema_migrations)")
        .map_err(migration_error)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(migration_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(migration_error)?;
    let has_hash = names.iter().any(|name| name == "descriptor_hash");
    let has_format = names.iter().any(|name| name == "descriptor_format_version");
    match (has_hash, has_format) {
        (false, false) => Ok(LedgerShape::Legacy),
        (true, true) => Ok(LedgerShape::DescriptorV1),
        _ => Err(migration_drift(
            "migration ledger descriptor columns are incomplete",
        )),
    }
}

fn verify_applied(connection: &Connection) -> Result<(), StoreError> {
    let max_version: Option<i64> = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .map_err(migration_error)?;
    let binary_version = MIGRATIONS.last().map_or(0, |migration| migration.version());
    if max_version.is_some_and(|version| version > binary_version) {
        return Err(StoreError::new(
            StoreErrorCode::DatabaseSchemaTooNew,
            "database contains a migration newer than this binary",
        ));
    }

    let shape = detect_ledger_shape(connection)?;
    let sql = match shape {
        LedgerShape::Legacy => {
            "SELECT version, name, checksum, NULL, NULL FROM schema_migrations ORDER BY version"
        }
        LedgerShape::DescriptorV1 => {
            "SELECT version, name, checksum, descriptor_hash, descriptor_format_version \
             FROM schema_migrations ORDER BY version"
        }
    };
    let mut statement = connection.prepare(sql).map_err(migration_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(LedgerRow {
                version: row.get(0)?,
                name: row.get(1)?,
                checksum: row.get(2)?,
                descriptor_hash: row.get(3)?,
                descriptor_format_version: row.get(4)?,
            })
        })
        .map_err(migration_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(migration_error)?;
    if rows.len() > MIGRATIONS.len() {
        return Err(migration_drift(
            "database migration ledger is not a binary prefix",
        ));
    }
    for (index, row) in rows.iter().enumerate() {
        let expected = MIGRATIONS
            .get(index)
            .ok_or_else(|| migration_drift("database migration ledger is not a binary prefix"))?;
        if row.version != expected.version() {
            return Err(migration_drift(
                "database migration ledger is not a continuous prefix",
            ));
        }
        if row.name != expected.name() {
            return Err(migration_drift(
                "applied migration name differs from this binary",
            ));
        }
        verify_ledger_row(row, *expected)?;
    }
    Ok(())
}

fn verify_ledger_row(row: &LedgerRow, expected: MigrationDefinition) -> Result<(), StoreError> {
    match expected {
        MigrationDefinition::LegacySql(migration) => {
            if row.checksum != checksum_hex(migration.sql)
                || row.descriptor_hash.is_some()
                || row.descriptor_format_version.is_some()
            {
                return Err(migration_drift(
                    "legacy applied migration metadata differs from this binary",
                ));
            }
        }
        MigrationDefinition::DescriptorV1(migration) => {
            validate_descriptor_migration(migration)?;
            let expected_hash = descriptor_hash(migration)?;
            if row.descriptor_format_version != Some(1)
                || row.descriptor_hash.as_deref() != Some(expected_hash.as_str())
                || row.checksum != expected_hash
            {
                return Err(migration_drift(
                    "descriptor migration metadata differs from this binary",
                ));
            }
        }
    }
    Ok(())
}

fn validate_descriptor_migration(migration: DescriptorV1Migration) -> Result<(), StoreError> {
    if migration.version != 3
        || migration.name != "versioned_event_outbox"
        || migration.asset_path != MIGRATION_0003_ASSET_PATH
        || migration.transform.algorithm_id != MIGRATION_0003_ALGORITHM_ID
        || migration.transform.version != 1
        || migration.transform.implementation_id != MIGRATION_0003_IMPLEMENTATION_ID
    {
        return Err(migration_drift(
            "migration 0003 descriptor identity is not the accepted closed identity",
        ));
    }
    validate_raw_asset(migration.sql)?;
    Ok(())
}

fn validate_raw_asset(bytes: &[u8]) -> Result<(), StoreError> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf])
        || !bytes.ends_with(b"\n")
        || bytes.ends_with(b"\n\n")
        || bytes.contains(&b'\r')
        || std::str::from_utf8(bytes).is_err()
    {
        return Err(migration_drift(
            "descriptor SQL asset must be UTF-8 LF bytes with one trailing LF and no BOM",
        ));
    }
    Ok(())
}

fn descriptor_bytes(migration: DescriptorV1Migration) -> Result<Vec<u8>, StoreError> {
    validate_descriptor_migration(migration)?;
    let descriptor = DescriptorV1 {
        descriptor_format_version: 1,
        migration_version: migration.version,
        name: migration.name,
        sql_assets: [DescriptorSqlAsset {
            path: migration.asset_path,
            sha256: checksum_hex(migration.sql),
        }],
        transform: migration.transform,
    };
    let value = serde_json::to_value(descriptor).map_err(|_| {
        StoreError::new(
            StoreErrorCode::MigrationFailed,
            "migration descriptor serialization failed",
        )
    })?;
    let mut bytes = canonical_json_string(&value)
        .map_err(|_| {
            StoreError::new(
                StoreErrorCode::MigrationFailed,
                "migration descriptor canonicalization failed",
            )
        })?
        .into_bytes();
    bytes.push(b'\n');
    Ok(bytes)
}

fn descriptor_hash(migration: DescriptorV1Migration) -> Result<String, StoreError> {
    Ok(checksum_hex(&descriptor_bytes(migration)?))
}

fn is_applied(connection: &Connection, version: i64) -> Result<bool, StoreError> {
    connection
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE version = ?1",
            [version],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(migration_error)
}

fn apply_one(connection: &Connection, migration: MigrationDefinition) -> Result<(), StoreError> {
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(migration_error)?;
    let result = (|| {
        verify_applied(connection)?;
        if !is_applied(connection, migration.version())? {
            match migration {
                MigrationDefinition::LegacySql(migration) => apply_legacy(connection, migration),
                MigrationDefinition::DescriptorV1(migration) => {
                    apply_descriptor_v1(connection, migration)
                }
            }?;
        }
        connection.execute_batch("COMMIT").map_err(migration_error)
    })();
    if let Err(error) = result {
        if connection.execute_batch("ROLLBACK").is_err() {
            return Err(StoreError::new(
                StoreErrorCode::MigrationFailed,
                format!(
                    "migration failed with {} and rollback also failed",
                    error.code.as_str()
                ),
            ));
        }
        return Err(error);
    }
    Ok(())
}

fn apply_legacy(connection: &Connection, migration: LegacySqlMigration) -> Result<(), StoreError> {
    let sql = std::str::from_utf8(migration.sql).map_err(|_| {
        StoreError::new(
            StoreErrorCode::MigrationFailed,
            "legacy migration SQL is not UTF-8",
        )
    })?;
    connection.execute_batch(sql).map_err(migration_error)?;
    connection
        .execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at) \
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![
                migration.version,
                migration.name,
                checksum_hex(migration.sql)
            ],
        )
        .map_err(migration_error)?;
    Ok(())
}

fn apply_descriptor_v1(
    connection: &Connection,
    migration: DescriptorV1Migration,
) -> Result<(), StoreError> {
    validate_descriptor_migration(migration)?;
    let phases = parse_migration_phases(migration.sql)?;
    execute_phase(connection, &phases, "ledger_upgrade")?;
    execute_phase(connection, &phases, "replacement_schema")?;
    outbox_v1_to_versioned_v1(connection)?;
    execute_phase(connection, &phases, "table_swap")?;
    validate_versioned_outbox(connection)?;
    let hash = descriptor_hash(migration)?;
    connection
        .execute(
            "INSERT INTO schema_migrations(\
                version, name, checksum, applied_at, descriptor_hash, descriptor_format_version\
             ) VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?3, 1)",
            params![migration.version, migration.name, hash],
        )
        .map_err(migration_error)?;
    Ok(())
}

fn parse_migration_phases(bytes: &[u8]) -> Result<BTreeMap<String, String>, StoreError> {
    let sql = std::str::from_utf8(bytes)
        .map_err(|_| migration_drift("migration phase asset is not UTF-8"))?;
    let marker = "-- kernel-sqlite migration phase: ";
    let expected = ["ledger_upgrade", "replacement_schema", "table_swap"];
    let mut phases = BTreeMap::new();
    let mut encountered = Vec::new();
    let mut current: Option<String> = None;
    let mut body = String::new();
    for line in sql.lines() {
        if let Some(name) = line.strip_prefix(marker) {
            if name.is_empty()
                || name
                    .chars()
                    .any(|character| !character.is_ascii_lowercase() && character != '_')
            {
                return Err(migration_drift("migration phase marker is malformed"));
            }
            if let Some(previous) = current.replace(name.to_owned()) {
                encountered.push(previous.clone());
                if phases.insert(previous, std::mem::take(&mut body)).is_some() {
                    return Err(migration_drift("migration phase marker is duplicated"));
                }
            } else if !body.trim().is_empty() {
                return Err(migration_drift(
                    "SQL exists before the first migration phase",
                ));
            }
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    let Some(last) = current else {
        return Err(migration_drift("migration phase markers are missing"));
    };
    encountered.push(last.clone());
    if phases.insert(last, body).is_some() {
        return Err(migration_drift("migration phase marker is duplicated"));
    }
    if encountered != expected
        || phases.len() != expected.len()
        || expected
            .iter()
            .any(|phase| !phases.contains_key(*phase) || phases[*phase].trim().is_empty())
    {
        return Err(migration_drift("migration phase set is not exact"));
    }
    Ok(phases)
}

fn execute_phase(
    connection: &Connection,
    phases: &BTreeMap<String, String>,
    name: &str,
) -> Result<(), StoreError> {
    connection
        .execute_batch(&phases[name])
        .map_err(migration_error)
}

/// Exact migration 0003 transformation algorithm implementation.
fn outbox_v1_to_versioned_v1(connection: &Connection) -> Result<(), StoreError> {
    #[derive(Debug)]
    struct LegacyRow {
        position: i64,
        event_id: String,
        event_type: String,
        schema_version: i64,
        aggregate_type: String,
        aggregate_id: String,
        sequence: i64,
        occurred_at: String,
        causation_kind: String,
        causation_id: String,
        correlation_id: String,
        dedup_key: String,
        payload_json: String,
        delivered_at: Option<String>,
    }

    let rows = {
        let mut statement = connection
            .prepare(
                "SELECT outbox_position, event_id, event_type, schema_version, aggregate_type, \
                        aggregate_id, sequence, occurred_at, causation_kind, causation_id, \
                        correlation_id, dedup_key, payload_json, delivered_at \
                 FROM outbox ORDER BY outbox_position",
            )
            .map_err(migration_error)?;
        let mapped = statement
            .query_map([], |row| {
                Ok(LegacyRow {
                    position: row.get(0)?,
                    event_id: row.get(1)?,
                    event_type: row.get(2)?,
                    schema_version: row.get(3)?,
                    aggregate_type: row.get(4)?,
                    aggregate_id: row.get(5)?,
                    sequence: row.get(6)?,
                    occurred_at: row.get(7)?,
                    causation_kind: row.get(8)?,
                    causation_id: row.get(9)?,
                    correlation_id: row.get(10)?,
                    dedup_key: row.get(11)?,
                    payload_json: row.get(12)?,
                    delivered_at: row.get(13)?,
                })
            })
            .map_err(migration_error)?;
        mapped
            .collect::<Result<Vec<_>, _>>()
            .map_err(migration_error)?
    };

    for row in &rows {
        let payload = crate::outbox::decode_legacy_storage_parts(
            row.position,
            &row.event_id,
            &row.event_type,
            row.schema_version,
            &row.aggregate_type,
            &row.aggregate_id,
            row.sequence,
            &row.occurred_at,
            &row.causation_kind,
            &row.causation_id,
            &row.correlation_id,
            &row.dedup_key,
            &row.payload_json,
            row.delivered_at.as_deref(),
        )?;
        let causation = CausationRef {
            kind: match row.causation_kind.as_str() {
                "command_request" => CausationRefKind::CommandRequest,
                "event" => CausationRefKind::Event,
                _ => return Err(stored_invalid()),
            },
            id: row.causation_id.clone(),
        };
        let causation_value = serde_json::to_value(&causation).map_err(|_| stored_invalid())?;
        let causation_json =
            canonical_json_string(&causation_value).map_err(|_| stored_invalid())?;
        connection
            .execute(
                "INSERT INTO outbox_versioned_replacement(\
                    outbox_position, event_id, event_type, schema_version, aggregate_type, \
                    aggregate_id, sequence, occurred_at, causation_json, correlation_id, \
                    dedup_key, payload_json, delivered_at\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    row.position,
                    row.event_id,
                    row.event_type,
                    row.schema_version,
                    row.aggregate_type,
                    row.aggregate_id,
                    row.sequence,
                    row.occurred_at,
                    causation_json,
                    row.correlation_id,
                    row.dedup_key,
                    row.payload_json,
                    row.delivered_at,
                ],
            )
            .map_err(migration_error)?;
        let readback = crate::outbox::decode_versioned_row_at(
            connection,
            "outbox_versioned_replacement",
            row.position,
        )?
        .ok_or_else(stored_invalid)?;
        if readback != payload {
            return Err(stored_invalid());
        }
    }

    let copied: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM outbox_versioned_replacement",
            [],
            |row| row.get(0),
        )
        .map_err(migration_error)?;
    if copied != rows.len() as i64 {
        return Err(stored_invalid());
    }
    validate_sequence_closure(connection, "outbox_versioned_replacement")?;
    let next_sequence = rows.last().map_or(1, |row| row.position + 1);
    connection
        .execute(
            "INSERT OR REPLACE INTO sqlite_sequence(name, seq) VALUES ('outbox_versioned_replacement', ?1)",
            [next_sequence - 1],
        )
        .map_err(migration_error)?;
    Ok(())
}

fn validate_versioned_outbox(connection: &Connection) -> Result<(), StoreError> {
    validate_sequence_closure(connection, "outbox")?;
    let max_position: Option<i64> = connection
        .query_row("SELECT MAX(outbox_position) FROM outbox", [], |row| {
            row.get(0)
        })
        .map_err(migration_error)?;
    let sequence: Option<i64> = connection
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'outbox'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(migration_error)?;
    if sequence.unwrap_or(0) != max_position.unwrap_or(0) {
        return Err(stored_invalid());
    }
    Ok(())
}

fn validate_sequence_closure(connection: &Connection, table: &str) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT aggregate_type, aggregate_id, MIN(sequence), MAX(sequence), COUNT(*) \
         FROM {table} GROUP BY aggregate_type, aggregate_id"
    );
    let mut statement = connection.prepare(&sql).map_err(migration_error)?;
    let aggregates = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(migration_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(migration_error)?;
    for (aggregate_type, aggregate_id, minimum, maximum, count) in aggregates {
        if minimum != 0 || count != maximum + 1 {
            return Err(stored_invalid());
        }
        let last: Option<i64> = connection
            .query_row(
                "SELECT last_sequence FROM aggregate_event_sequences \
                 WHERE aggregate_type = ?1 AND aggregate_id = ?2",
                params![aggregate_type, aggregate_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(migration_error)?;
        if last != Some(maximum) {
            return Err(stored_invalid());
        }
    }
    let orphan_sequences: i64 = connection
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM aggregate_event_sequences AS sequences \
                 WHERE NOT EXISTS (SELECT 1 FROM {table} AS events \
                                   WHERE events.aggregate_type = sequences.aggregate_type \
                                     AND events.aggregate_id = sequences.aggregate_id)"
            ),
            [],
            |row| row.get(0),
        )
        .map_err(migration_error)?;
    if orphan_sequences != 0 {
        return Err(stored_invalid());
    }
    Ok(())
}

fn checksum_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn migration_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::MigrationFailed)
}

fn migration_drift(message: &'static str) -> StoreError {
    StoreError::new(StoreErrorCode::MigrationDrift, message)
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "legacy Outbox data failed migration integrity validation",
    )
}

#[cfg(test)]
pub(crate) fn create_v1_database_for_test(connection: &Connection) -> Result<(), StoreError> {
    ensure_migration_table(connection)?;
    apply_one(connection, MIGRATIONS[0])
}

#[cfg(test)]
pub(crate) fn create_v2_database_for_test(connection: &Connection) -> Result<(), StoreError> {
    ensure_migration_table(connection)?;
    apply_one(connection, MIGRATIONS[0])?;
    apply_one(connection, MIGRATIONS[1])
}

#[cfg(test)]
pub(crate) fn migration_0003_descriptor_bytes_for_test() -> Vec<u8> {
    let MigrationDefinition::DescriptorV1(migration) = MIGRATIONS[2] else {
        unreachable!()
    };
    descriptor_bytes(migration).expect("descriptor")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn failed_migration_rolls_back_its_ddl_and_ledger_row() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("migration.sqlite3");
        let connection = Connection::open(path).expect("connection");
        ensure_migration_table(&connection).expect("migration table");
        connection.execute_batch("BEGIN IMMEDIATE").expect("begin");
        let result =
            connection.execute_batch("CREATE TABLE partially_applied(id INTEGER); INVALID SQL;");
        assert!(result.is_err());
        connection.execute_batch("ROLLBACK").expect("rollback");
        let table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'partially_applied'",
                [],
                |row| row.get(0),
            )
            .expect("table count");
        assert_eq!(table_count, 0);
    }
}
