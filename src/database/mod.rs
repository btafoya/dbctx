//! Database introspection: open a read-only connection and populate the
//! canonical schema model from catalog metadata.
//!
//! `SPEC.md` §7 requires `INFORMATION_SCHEMA` as the primary source on every
//! engine, with native catalog views (`sys.*` on SQL Server) supplying only the
//! facts `INFORMATION_SCHEMA` does not expose. Nothing here parses SQL or writes
//! to the database.

use thiserror::Error;

use crate::Result;
use crate::config::{ConnectionConfig, Driver};
use crate::model::Database;

pub(crate) mod mysql;
pub(crate) mod sqlserver;

/// Why a database could not be inspected.
#[derive(Debug, Error)]
pub enum DatabaseError {
    /// The connection string or handshake failed.
    #[error(
        "could not connect to the database: {0}\n\
         check --host, --port, --user and --password, and that the server is reachable"
    )]
    Connection(String),

    /// A catalog query returned an unexpected shape.
    #[error("could not read catalog metadata: {0}")]
    Catalog(String),
}

impl DatabaseError {
    /// A connection failure from any underlying driver.
    pub fn connection(source: impl std::error::Error) -> Self {
        Self::Connection(source.to_string())
    }
}

/// Read the schema described by `config` and return it as the canonical model.
///
/// The caller owns the connection settings; this function opens the connection,
/// collects metadata, closes the connection, and sorts the model before
/// returning it.
pub async fn inspect(config: &ConnectionConfig) -> Result<Database> {
    match config.driver() {
        Driver::Mysql | Driver::Mariadb => mysql::inspect(config).await,
        Driver::Sqlsrv => sqlserver::inspect(config).await,
    }
}
