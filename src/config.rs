//! Connection configuration and the precedence rules that build it.
//!
//! Configuration is assembled from ordered [`ConnectionSource`] layers and
//! resolved once into a [`ConnectionConfig`], which exposes its values through
//! accessors and offers no way to change them afterwards.
//!
//! `SPEC.md` §6 fixes the order the layers are consulted in:
//!
//! 1. CLI options
//! 2. Docker Compose autodiscovery
//! 3. `.dbctx.toml`
//! 4. `.env`
//! 5. Environment variables
//! 6. Interactive prompt
//!
//! Layers 2, 3 and 6 arrive with Phase 3 alongside connection discovery.
//! [`ConnectionConfig::resolve`] takes the layers in priority order, so they
//! slot in without disturbing the ones already here.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use thiserror::Error;

/// Environment variable names `SPEC.md` §6 supports, in `.env` and in the
/// process environment alike.
const DB_CONNECTION: &str = "DB_CONNECTION";
const DB_HOST: &str = "DB_HOST";
const DB_PORT: &str = "DB_PORT";
const DB_DATABASE: &str = "DB_DATABASE";
const DB_USERNAME: &str = "DB_USERNAME";
const DB_PASSWORD: &str = "DB_PASSWORD";

/// The database driver to connect with.
///
/// These are the names the CLI's `--driver` option and the `DB_CONNECTION`
/// environment variable accept. They are not the engine names written into
/// documents: `sqlsrv` here is `sqlserver` in [`crate::model::Engine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    /// MySQL.
    Mysql,
    /// MariaDB.
    Mariadb,
    /// Microsoft SQL Server.
    Sqlsrv,
}

impl Driver {
    /// The port to connect on when none was configured.
    pub const fn default_port(self) -> u16 {
        match self {
            Driver::Mysql | Driver::Mariadb => 3306,
            Driver::Sqlsrv => 1433,
        }
    }

    /// The name this driver is selected by.
    pub const fn as_str(self) -> &'static str {
        match self {
            Driver::Mysql => "mysql",
            Driver::Mariadb => "mariadb",
            Driver::Sqlsrv => "sqlsrv",
        }
    }
}

impl FromStr for Driver {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "mysql" => Ok(Driver::Mysql),
            "mariadb" => Ok(Driver::Mariadb),
            "sqlsrv" => Ok(Driver::Sqlsrv),
            other => Err(ConfigError::UnknownDriver {
                value: other.to_string(),
            }),
        }
    }
}

impl fmt::Display for Driver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One layer of connection settings, with every value optional.
///
/// A layer says only what it knows. Resolution takes the first layer that
/// supplies each field, so a layer never has to be aware of the ones around
/// it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionSource {
    /// Driver to connect with.
    pub driver: Option<Driver>,
    /// Host to connect to.
    pub host: Option<String>,
    /// Port to connect on.
    pub port: Option<u16>,
    /// Database to inspect.
    pub database: Option<String>,
    /// User to connect as.
    pub user: Option<String>,
    /// Password to connect with.
    pub password: Option<String>,
    /// Unix socket to connect through, MySQL and MariaDB only.
    pub socket: Option<PathBuf>,
}

impl ConnectionSource {
    /// The layer described by the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_vars(std::env::vars())
    }

    /// The layer described by a `.env` file.
    ///
    /// A file that is not there yields no settings when `required` is false,
    /// which is how the default `.env` is treated, and an error when it is
    /// true, which is how a file named with `--env` is treated: a path the
    /// user asked for and that is not readable is a mistake worth reporting.
    pub fn from_dotenv(path: &Path, required: bool) -> Result<Self, ConfigError> {
        let entries = match dotenvy::from_path_iter(path) {
            Ok(entries) => entries,
            Err(error) => {
                if !required && is_not_found(&error) {
                    tracing::debug!(path = %path.display(), "no environment file");
                    return Ok(Self::default());
                }
                return Err(ConfigError::EnvFile {
                    path: path.to_path_buf(),
                    source: error,
                });
            }
        };

        let mut vars = Vec::new();
        for entry in entries {
            vars.push(entry.map_err(|error| ConfigError::EnvFile {
                path: path.to_path_buf(),
                source: error,
            })?);
        }
        tracing::debug!(path = %path.display(), variables = vars.len(), "read environment file");
        Self::from_vars(vars)
    }

    /// The layer described by the `DB_*` variables among `vars`.
    ///
    /// Shared by the process environment and `.env`, which `SPEC.md` gives
    /// the same variable names and different priorities.
    pub fn from_vars(
        vars: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, ConfigError> {
        let mut source = Self::default();
        for (key, value) in vars {
            match key.as_str() {
                DB_CONNECTION => source.driver = Some(value.parse()?),
                DB_HOST => source.host = Some(value),
                DB_PORT => {
                    source.port = Some(value.parse().map_err(|_| ConfigError::InvalidPort {
                        value: value.clone(),
                    })?);
                }
                DB_DATABASE => source.database = Some(value),
                DB_USERNAME => source.user = Some(value),
                DB_PASSWORD => source.password = Some(value),
                _ => {}
            }
        }
        Ok(source)
    }
}

/// Resolved connection settings.
///
/// Built once by [`ConnectionConfig::resolve`] and read-only from then on.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    driver: Option<Driver>,
    host: Option<String>,
    port: Option<u16>,
    database: String,
    user: Option<String>,
    password: Option<String>,
    socket: Option<PathBuf>,
}

impl ConnectionConfig {
    /// Resolve `sources` into one configuration, taking each field from the
    /// earliest source that supplies it.
    ///
    /// The port falls back to the driver's default once a driver is known.
    /// When no driver was configured both stay unset, because `SPEC.md` has
    /// the driver detected from the connection, which is Phase 3's job.
    pub fn resolve(sources: &[ConnectionSource]) -> Result<Self, ConfigError> {
        let pick = |field: fn(&ConnectionSource) -> Option<&str>| {
            sources.iter().find_map(field).map(str::to_string)
        };

        let driver = sources.iter().find_map(|source| source.driver);
        let port = sources
            .iter()
            .find_map(|source| source.port)
            .or_else(|| driver.map(Driver::default_port));
        let database =
            pick(|source| source.database.as_deref()).ok_or(ConfigError::MissingDatabase)?;

        let config = Self {
            driver,
            host: pick(|source| source.host.as_deref()),
            port,
            database,
            user: pick(|source| source.user.as_deref()),
            password: pick(|source| source.password.as_deref()),
            socket: sources
                .iter()
                .find_map(|source| source.socket.as_deref())
                .map(Path::to_path_buf),
        };

        // The password is left out rather than redacted: a credential that is
        // never formatted cannot leak through a format string.
        tracing::debug!(
            driver = ?config.driver,
            host = ?config.host,
            port = ?config.port,
            database = %config.database,
            user = ?config.user,
            socket = ?config.socket,
            "resolved connection"
        );

        Ok(config)
    }

    /// Driver to connect with, or `None` to detect it from the connection.
    pub fn driver(&self) -> Option<Driver> {
        self.driver
    }

    /// Host to connect to, if one was configured.
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// Port to connect on. Set once a driver is known.
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// Database to inspect.
    pub fn database(&self) -> &str {
        &self.database
    }

    /// User to connect as, if one was configured.
    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    /// Password to connect with, if one was configured.
    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    /// Unix socket to connect through, if one was configured.
    pub fn socket(&self) -> Option<&Path> {
        self.socket.as_deref()
    }
}

/// Redacts the password, so a configuration can be logged or dumped without
/// leaking the credential `SPEC.md` §20 says is never persisted.
impl fmt::Debug for ConnectionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionConfig")
            .field("driver", &self.driver)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("socket", &self.socket)
            .finish()
    }
}

/// Why a configuration could not be resolved.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// No layer named a database.
    #[error(
        "no database was configured\n\
         dbctx needs to know which database to inspect, and neither --database, \
         a .env file nor DB_DATABASE supplied one\n\
         try: dbctx inspect --database <NAME>"
    )]
    MissingDatabase,

    /// A driver name nothing recognises.
    #[error(
        "unknown driver `{value}`\n\
         --driver and DB_CONNECTION select the database engine to connect with\n\
         try one of: mysql, mariadb, sqlsrv"
    )]
    UnknownDriver {
        /// The name that was supplied.
        value: String,
    },

    /// A port that is not a number in range.
    #[error(
        "invalid port `{value}`\n\
         a port must be a whole number between 0 and 65535\n\
         try: --port 3306 for MySQL or MariaDB, --port 1433 for SQL Server"
    )]
    InvalidPort {
        /// The value that was supplied.
        value: String,
    },

    /// An environment file that could not be read.
    #[error(
        "could not read the environment file `{path}`: {source}\n\
         dbctx reads connection settings from this file\n\
         check that the path is correct and the file is readable"
    )]
    EnvFile {
        /// The file that was being read.
        path: PathBuf,
        /// What went wrong.
        source: dotenvy::Error,
    },
}

/// Whether a dotenvy failure is just an absent file.
fn is_not_found(error: &dotenvy::Error) -> bool {
    matches!(error, dotenvy::Error::Io(io) if io.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn named(database: &str) -> ConnectionSource {
        ConnectionSource {
            database: Some(database.to_string()),
            ..ConnectionSource::default()
        }
    }

    #[test]
    fn each_field_comes_from_the_highest_priority_source_that_supplies_it() {
        let cli = ConnectionSource {
            host: Some("cli-host".to_string()),
            ..ConnectionSource::default()
        };
        let dotenv = ConnectionSource {
            host: Some("dotenv-host".to_string()),
            user: Some("dotenv-user".to_string()),
            ..ConnectionSource::default()
        };
        let env = ConnectionSource {
            host: Some("env-host".to_string()),
            user: Some("env-user".to_string()),
            database: Some("env-database".to_string()),
            ..ConnectionSource::default()
        };

        let config = ConnectionConfig::resolve(&[cli, dotenv, env]).unwrap();

        assert_eq!(config.host(), Some("cli-host"));
        assert_eq!(config.user(), Some("dotenv-user"));
        assert_eq!(config.database(), "env-database");
    }

    #[test]
    fn a_dotenv_file_outranks_the_process_environment() {
        let dotenv = ConnectionSource::from_vars(vars(&[("DB_DATABASE", "from-dotenv")])).unwrap();
        let env =
            ConnectionSource::from_vars(vars(&[("DB_DATABASE", "from-environment")])).unwrap();

        let config = ConnectionConfig::resolve(&[dotenv, env]).unwrap();

        assert_eq!(config.database(), "from-dotenv");
    }

    #[test]
    fn the_port_falls_back_to_the_default_for_the_configured_driver() {
        for (driver, port) in [
            (Driver::Mysql, 3306),
            (Driver::Mariadb, 3306),
            (Driver::Sqlsrv, 1433),
        ] {
            let source = ConnectionSource {
                driver: Some(driver),
                ..named("shop")
            };

            let config = ConnectionConfig::resolve(&[source]).unwrap();

            assert_eq!(config.port(), Some(port), "{driver}");
        }
    }

    #[test]
    fn a_configured_port_beats_the_driver_default() {
        let source = ConnectionSource {
            driver: Some(Driver::Mysql),
            port: Some(3307),
            ..named("shop")
        };

        let config = ConnectionConfig::resolve(&[source]).unwrap();

        assert_eq!(config.port(), Some(3307));
    }

    #[test]
    fn the_port_stays_unset_while_the_driver_is_still_to_be_detected() {
        let config = ConnectionConfig::resolve(&[named("shop")]).unwrap();

        assert_eq!(config.driver(), None);
        assert_eq!(config.port(), None);
    }

    #[test]
    fn resolving_without_a_database_reports_what_to_supply() {
        let error = ConnectionConfig::resolve(&[ConnectionSource::default()]).unwrap_err();

        assert!(matches!(error, ConfigError::MissingDatabase));
        assert!(error.to_string().contains("--database"));
    }

    #[test]
    fn environment_variables_map_onto_connection_settings() {
        let source = ConnectionSource::from_vars(vars(&[
            ("DB_CONNECTION", "mariadb"),
            ("DB_HOST", "db.internal"),
            ("DB_PORT", "3307"),
            ("DB_DATABASE", "shop"),
            ("DB_USERNAME", "reader"),
            ("DB_PASSWORD", "secret"),
            ("UNRELATED", "ignored"),
        ]))
        .unwrap();

        assert_eq!(
            source,
            ConnectionSource {
                driver: Some(Driver::Mariadb),
                host: Some("db.internal".to_string()),
                port: Some(3307),
                database: Some("shop".to_string()),
                user: Some("reader".to_string()),
                password: Some("secret".to_string()),
                socket: None,
            }
        );
    }

    #[test]
    fn an_unparseable_port_names_the_offending_value() {
        let error = ConnectionSource::from_vars(vars(&[("DB_PORT", "not-a-port")])).unwrap_err();

        assert!(matches!(error, ConfigError::InvalidPort { .. }));
        assert!(error.to_string().contains("not-a-port"));
    }

    #[test]
    fn a_port_outside_the_range_is_rejected() {
        let error = ConnectionSource::from_vars(vars(&[("DB_PORT", "65536")])).unwrap_err();

        assert!(matches!(error, ConfigError::InvalidPort { .. }));
    }

    #[test]
    fn an_unknown_driver_lists_the_ones_that_work() {
        let error =
            ConnectionSource::from_vars(vars(&[("DB_CONNECTION", "postgres")])).unwrap_err();

        assert!(matches!(error, ConfigError::UnknownDriver { .. }));
        let message = error.to_string();
        assert!(message.contains("postgres"));
        assert!(message.contains("mysql, mariadb, sqlsrv"));
    }

    #[test]
    fn drivers_round_trip_through_their_names() {
        for driver in [Driver::Mysql, Driver::Mariadb, Driver::Sqlsrv] {
            assert_eq!(driver.as_str().parse::<Driver>().unwrap(), driver);
        }
    }

    #[test]
    fn a_dotenv_file_supplies_the_settings_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(
            &path,
            "# connection\nDB_CONNECTION=mysql\nDB_HOST=localhost\nexport DB_DATABASE=shop\nDB_PASSWORD=\"a secret\"\n",
        )
        .unwrap();

        let source = ConnectionSource::from_dotenv(&path, true).unwrap();

        assert_eq!(source.driver, Some(Driver::Mysql));
        assert_eq!(source.host.as_deref(), Some("localhost"));
        assert_eq!(source.database.as_deref(), Some("shop"));
        assert_eq!(source.password.as_deref(), Some("a secret"));
    }

    #[test]
    fn an_absent_default_dotenv_file_simply_supplies_nothing() {
        let dir = tempfile::tempdir().unwrap();

        let source = ConnectionSource::from_dotenv(&dir.path().join(".env"), false).unwrap();

        assert_eq!(source, ConnectionSource::default());
    }

    #[test]
    fn an_absent_requested_env_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.env");

        let error = ConnectionSource::from_dotenv(&path, true).unwrap_err();

        assert!(matches!(error, ConfigError::EnvFile { .. }));
        assert!(error.to_string().contains("missing.env"));
    }

    #[test]
    fn debug_output_redacts_the_password() {
        let source = ConnectionSource {
            user: Some("reader".to_string()),
            password: Some("hunter2".to_string()),
            ..named("shop")
        };

        let config = ConnectionConfig::resolve(&[source]).unwrap();
        let debug = format!("{config:?}");

        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(debug.contains("reader"), "{debug}");
        assert_eq!(config.password(), Some("hunter2"));
    }
}
