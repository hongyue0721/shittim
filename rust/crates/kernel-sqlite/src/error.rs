//! Stable storage errors with machine-readable codes.

use rusqlite::{ffi::ErrorCode as SqliteErrorCode, Error as SqliteError};
use thiserror::Error;

/// Machine-readable SQLite store error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreErrorCode {
    /// The configured database path is empty, in-memory, or a SQLite URI.
    InvalidDatabasePath,
    /// SQLite could not open the database file.
    SqliteOpenFailed,
    /// Required connection pragmas could not be applied or verified.
    SqliteConfigurationFailed,
    /// SQLite could not acquire a lock within the configured timeout.
    SqliteBusy,
    /// The database or containing filesystem is full.
    SqliteFull,
    /// SQLite reported corruption or a non-database file.
    SqliteCorrupt,
    /// A migration could not be applied or recorded.
    MigrationFailed,
    /// An applied migration's name or checksum differs from the binary.
    MigrationDrift,
    /// The database contains a migration newer than this binary.
    DatabaseSchemaTooNew,
    /// A uniqueness, foreign-key, or check constraint was violated.
    ConstraintViolation,
    /// JSON serialization, canonicalization, or decoding failed.
    SerializationFailed,
    /// A generated JSON contract rejected the record.
    ContractInvalid,
    /// A cursor, position, or page limit is invalid.
    InvalidCursor,
    /// The requested immutable record does not exist.
    NotFound,
    /// An internal store invariant failed.
    InternalStoreError,
}

impl StoreErrorCode {
    /// Returns the stable machine code.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidDatabasePath => "invalid_database_path",
            Self::SqliteOpenFailed => "sqlite_open_failed",
            Self::SqliteConfigurationFailed => "sqlite_configuration_failed",
            Self::SqliteBusy => "sqlite_busy",
            Self::SqliteFull => "sqlite_full",
            Self::SqliteCorrupt => "sqlite_corrupt",
            Self::MigrationFailed => "migration_failed",
            Self::MigrationDrift => "migration_drift",
            Self::DatabaseSchemaTooNew => "database_schema_too_new",
            Self::ConstraintViolation => "constraint_violation",
            Self::SerializationFailed => "serialization_failed",
            Self::ContractInvalid => "contract_invalid",
            Self::InvalidCursor => "invalid_cursor",
            Self::NotFound => "not_found",
            Self::InternalStoreError => "internal_store_error",
        }
    }
}

/// Store error whose message never includes SQL text, parameters, or payload bodies.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}", code = .code.as_str())]
pub struct StoreError {
    /// Stable machine-readable error code.
    pub code: StoreErrorCode,
    /// Non-sensitive human-readable context.
    pub message: String,
}

impl StoreError {
    pub(crate) fn new(code: StoreErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn sqlite(error: SqliteError, fallback: StoreErrorCode) -> Self {
        let code = match &error {
            SqliteError::SqliteFailure(details, _) => map_sqlite_error_code(details.code, fallback),
            _ => fallback,
        };
        Self::new(code, safe_message(code))
    }
}

const fn map_sqlite_error_code(code: SqliteErrorCode, fallback: StoreErrorCode) -> StoreErrorCode {
    match code {
        SqliteErrorCode::DatabaseBusy | SqliteErrorCode::DatabaseLocked => {
            StoreErrorCode::SqliteBusy
        }
        SqliteErrorCode::DiskFull => StoreErrorCode::SqliteFull,
        SqliteErrorCode::DatabaseCorrupt | SqliteErrorCode::NotADatabase => {
            StoreErrorCode::SqliteCorrupt
        }
        SqliteErrorCode::ConstraintViolation => StoreErrorCode::ConstraintViolation,
        _ => fallback,
    }
}

fn safe_message(code: StoreErrorCode) -> &'static str {
    match code {
        StoreErrorCode::InvalidDatabasePath => "database path must name a file",
        StoreErrorCode::SqliteOpenFailed => "failed to open SQLite database",
        StoreErrorCode::SqliteConfigurationFailed => "failed to configure SQLite connection",
        StoreErrorCode::SqliteBusy => "SQLite database is busy",
        StoreErrorCode::SqliteFull => "SQLite database storage is full",
        StoreErrorCode::SqliteCorrupt => "SQLite database is corrupt or invalid",
        StoreErrorCode::MigrationFailed => "SQLite migration failed",
        StoreErrorCode::MigrationDrift => "applied SQLite migration differs from this binary",
        StoreErrorCode::DatabaseSchemaTooNew => "SQLite database schema is newer than this binary",
        StoreErrorCode::ConstraintViolation => "SQLite constraint was violated",
        StoreErrorCode::SerializationFailed => "record serialization failed",
        StoreErrorCode::ContractInvalid => "record violates its JSON contract",
        StoreErrorCode::InvalidCursor => "cursor, position, or page limit is invalid",
        StoreErrorCode::NotFound => "record was not found",
        StoreErrorCode::InternalStoreError => "internal SQLite store error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_primary_codes_map_without_message_matching() {
        let fallback = StoreErrorCode::InternalStoreError;
        for (sqlite, expected) in [
            (SqliteErrorCode::DatabaseBusy, StoreErrorCode::SqliteBusy),
            (SqliteErrorCode::DatabaseLocked, StoreErrorCode::SqliteBusy),
            (SqliteErrorCode::DiskFull, StoreErrorCode::SqliteFull),
            (
                SqliteErrorCode::DatabaseCorrupt,
                StoreErrorCode::SqliteCorrupt,
            ),
            (SqliteErrorCode::NotADatabase, StoreErrorCode::SqliteCorrupt),
            (
                SqliteErrorCode::ConstraintViolation,
                StoreErrorCode::ConstraintViolation,
            ),
        ] {
            assert_eq!(map_sqlite_error_code(sqlite, fallback), expected);
        }
    }
}
