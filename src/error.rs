//! The error type the library returns.
//!
//! Each layer defines the failures it raises next to the code that raises
//! them; this enum unifies those so one call into the library has one error
//! type to match on. Variants arrive with the phases that can produce them.
//!
//! The enum is deliberately not `#[non_exhaustive]`: the binary matches on it
//! to choose the exit codes `CLI.md` declares stable, and that mapping should
//! stop compiling when a variant appears without one.

use thiserror::Error;

use crate::config::ConfigError;
use crate::database::DatabaseError;
use crate::diff::DiffError;
use crate::discovery::DiscoveryError;
use crate::execution::ExecutionError;
use crate::export::ExportError;
use crate::mcp_server::McpServerError;

/// Anything that can go wrong inside dbctx.
#[derive(Debug, Error)]
pub enum Error {
    /// Configuration could not be resolved.
    #[error(transparent)]
    Config(#[from] ConfigError),

    /// A schema diff could not be performed.
    #[error(transparent)]
    Diff(#[from] DiffError),

    /// A connection could not be discovered.
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),

    /// The database could not be inspected.
    #[error(transparent)]
    Database(#[from] DatabaseError),

    /// A read-only statement could not be executed.
    #[error(transparent)]
    Execution(#[from] ExecutionError),

    /// Artifacts could not be written or validated.
    #[error(transparent)]
    Export(#[from] ExportError),

    /// The MCP server could not start or keep running.
    #[error(transparent)]
    Mcp(Box<McpServerError>),
}

impl From<McpServerError> for Error {
    fn from(error: McpServerError) -> Self {
        // Boxed so `Error` and `McpServerError`, which wraps an `Error` of
        // its own for schema-read failures, do not size each other
        // infinitely.
        Error::Mcp(Box::new(error))
    }
}

/// A [`Result`](std::result::Result) carrying [`enum@Error`] unless told
/// otherwise.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configuration_failure_converts_into_the_library_error() {
        let error: Error = ConfigError::MissingDatabase.into();

        assert!(matches!(error, Error::Config(_)));
        assert!(error.to_string().contains("no database was configured"));
    }

    #[test]
    fn an_export_failure_converts_into_the_library_error() {
        let error: Error = ExportError::Io {
            path: std::path::PathBuf::from("schema.json"),
            source: std::io::Error::other("fail"),
        }
        .into();

        assert!(matches!(error, Error::Export(_)));
    }
}
