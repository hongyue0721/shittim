//! File-backed connection configuration.

use crate::{StoreError, StoreErrorCode};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Required SQLite connection settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqliteConfig {
    /// Explicit non-zero wait time for database locks.
    pub busy_timeout: Duration,
}

impl SqliteConfig {
    /// Creates validated connection settings.
    pub fn new(busy_timeout: Duration) -> Result<Self, StoreError> {
        if busy_timeout.is_zero() {
            return Err(StoreError::new(
                StoreErrorCode::SqliteConfigurationFailed,
                "busy_timeout must be non-zero",
            ));
        }
        Ok(Self { busy_timeout })
    }
}

pub(crate) fn validated_path(path: &Path) -> Result<PathBuf, StoreError> {
    let raw = path.as_os_str().to_string_lossy();
    if raw.is_empty()
        || raw == ":memory:"
        || raw.starts_with("file:")
        || raw.contains("mode=memory")
    {
        return Err(StoreError::new(
            StoreErrorCode::InvalidDatabasePath,
            "database path must name a file",
        ));
    }
    if let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() || !parent.exists() || !parent.is_dir() {
            return Err(StoreError::new(
                StoreErrorCode::InvalidDatabasePath,
                "database parent directory must already exist",
            ));
        }
    }
    if path.exists() && !path.is_file() {
        return Err(StoreError::new(
            StoreErrorCode::InvalidDatabasePath,
            "database path must name a regular file",
        ));
    }
    Ok(path.to_path_buf())
}

pub(crate) fn open_connection(path: &Path, config: SqliteConfig) -> Result<Connection, StoreError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| StoreError::sqlite(error, StoreErrorCode::SqliteOpenFailed))?;
    configure_connection(&connection, config)?;
    Ok(connection)
}

pub(crate) fn configure_connection(
    connection: &Connection,
    config: SqliteConfig,
) -> Result<(), StoreError> {
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .and_then(|()| connection.busy_timeout(config.busy_timeout))
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::SqliteConfigurationFailed))?;

    let foreign_keys: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::SqliteConfigurationFailed))?;
    let timeout_millis: i64 = connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::SqliteConfigurationFailed))?;
    let expected_timeout = i64::try_from(config.busy_timeout.as_millis()).map_err(|_| {
        StoreError::new(
            StoreErrorCode::SqliteConfigurationFailed,
            "busy_timeout is too large",
        )
    })?;
    if foreign_keys != 1 || timeout_millis != expected_timeout {
        return Err(StoreError::new(
            StoreErrorCode::SqliteConfigurationFailed,
            "required SQLite connection pragmas were not retained",
        ));
    }
    Ok(())
}

pub(crate) fn initialize_wal(
    connection: &Connection,
    busy_timeout: Duration,
) -> Result<(), StoreError> {
    let started_at = std::time::Instant::now();
    let applied = loop {
        match connection.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        }) {
            Ok(mode) => break mode,
            Err(error) if is_busy(&error) && started_at.elapsed() < busy_timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                return Err(StoreError::sqlite(
                    error,
                    StoreErrorCode::SqliteConfigurationFailed,
                ));
            }
        }
    };
    if !applied.eq_ignore_ascii_case("wal") {
        return Err(StoreError::new(
            StoreErrorCode::SqliteConfigurationFailed,
            "SQLite did not enable WAL journal mode",
        ));
    }
    let verified: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::SqliteConfigurationFailed))?;
    if !verified.eq_ignore_ascii_case("wal") {
        return Err(StoreError::new(
            StoreErrorCode::SqliteConfigurationFailed,
            "SQLite WAL journal mode verification failed",
        ));
    }
    Ok(())
}

fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(details, _)
            if matches!(
                details.code,
                rusqlite::ffi::ErrorCode::DatabaseBusy
                    | rusqlite::ffi::ErrorCode::DatabaseLocked
            )
    )
}
