//! Embedded, ordered, checksum-protected SQLite migrations.

use crate::{StoreError, StoreErrorCode};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("../migrations/0001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "task_repository",
        sql: include_str!("../migrations/0002_task_repository.sql"),
    },
];

pub(crate) fn apply_migrations(connection: &Connection) -> Result<(), StoreError> {
    // Non-business write exception (ADR-0004): ledger bootstrap sits outside pending migration
    // units and is not a public business write API surface.
    ensure_migration_table(connection)?;
    verify_applied(connection)?;
    for migration in MIGRATIONS {
        if !is_applied(connection, migration.version)? {
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

fn verify_applied(connection: &Connection) -> Result<(), StoreError> {
    let max_version: Option<i64> = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))?;
    let binary_version = MIGRATIONS.last().map_or(0, |migration| migration.version);
    if max_version.is_some_and(|version| version > binary_version) {
        return Err(StoreError::new(
            StoreErrorCode::DatabaseSchemaTooNew,
            "database contains a migration newer than this binary",
        ));
    }

    let mut statement = connection
        .prepare("SELECT version, name, checksum FROM schema_migrations ORDER BY version")
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))?;
    let applied = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))?;
    if applied.len() > MIGRATIONS.len() {
        return Err(StoreError::new(
            StoreErrorCode::MigrationDrift,
            "database migration ledger is not a binary prefix",
        ));
    }
    for (index, (version, name, checksum)) in applied.into_iter().enumerate() {
        let expected = MIGRATIONS.get(index).ok_or_else(|| {
            StoreError::new(
                StoreErrorCode::MigrationDrift,
                "database migration ledger is not a binary prefix",
            )
        })?;
        if version != expected.version {
            return Err(StoreError::new(
                StoreErrorCode::MigrationDrift,
                "database migration ledger is not a continuous prefix",
            ));
        }
        if name != expected.name || checksum != checksum_hex(expected.sql) {
            return Err(StoreError::new(
                StoreErrorCode::MigrationDrift,
                "applied migration metadata differs from this binary",
            ));
        }
    }
    Ok(())
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
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))
}

fn apply_one(connection: &Connection, migration: Migration) -> Result<(), StoreError> {
    // Non-business write exception: each pending migration is its own BEGIN IMMEDIATE unit and is
    // not part of SqliteStore public business write APIs.
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))?;
    let result = (|| {
        if is_applied(connection, migration.version)? {
            connection
                .execute_batch("COMMIT")
                .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))?;
            return Ok(());
        }
        connection
            .execute_batch(migration.sql)
            .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))?;
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
            .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))?;
        connection
            .execute_batch("COMMIT")
            .map_err(|error| StoreError::sqlite(error, StoreErrorCode::MigrationFailed))
    })();
    if result.is_err() {
        let _ = connection.execute_batch("ROLLBACK");
    }
    result
}

fn checksum_hex(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}

#[cfg(test)]
pub(crate) fn create_v1_database_for_test(connection: &Connection) -> Result<(), StoreError> {
    ensure_migration_table(connection)?;
    apply_one(connection, MIGRATIONS[0])
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
        let journal: String = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .expect("enable WAL");
        assert!(journal.eq_ignore_ascii_case("wal"));
        ensure_migration_table(&connection).expect("migration table");
        let migration = Migration {
            version: 99,
            name: "broken",
            sql: "CREATE TABLE partially_applied(id INTEGER); INVALID SQL;",
        };

        assert_eq!(
            apply_one(&connection, migration)
                .expect_err("migration must fail")
                .code,
            StoreErrorCode::MigrationFailed
        );
        let table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type = 'table' AND name = 'partially_applied'",
                [],
                |row| row.get(0),
            )
            .expect("table count");
        let ledger_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 99",
                [],
                |row| row.get(0),
            )
            .expect("ledger count");
        assert_eq!(table_count, 0);
        assert_eq!(ledger_count, 0);
    }
}
