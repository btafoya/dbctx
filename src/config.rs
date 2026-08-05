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

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;
use thiserror::Error;

/// Environment variable names `SPEC.md` §6 supports, in `.env` and in the
/// process environment alike.
const DB_CONNECTION: &str = "DB_CONNECTION";
const DB_HOST: &str = "DB_HOST";
const DB_PORT: &str = "DB_PORT";
const DB_DATABASE: &str = "DB_DATABASE";
const DB_USERNAME: &str = "DB_USERNAME";
const DB_PASSWORD: &str = "DB_PASSWORD";

/// The host to connect to when no source names one, per `SPEC.md` §6.
///
/// The loopback address rather than `localhost`, which resolves to a Unix
/// socket on some MySQL clients and to a TCP port on others.
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// The database driver to connect with.
///
/// These are the names the CLI's `--driver` option and the `DB_CONNECTION`
/// environment variable accept. They are not the engine names written into
/// documents: `sqlsrv` here is `sqlserver` in [`crate::model::Engine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Driver {
    /// MySQL.
    Mysql,
    /// MariaDB.
    Mariadb,
    /// Microsoft SQL Server.
    Sqlsrv,
    /// PostgreSQL.
    Postgres,
    /// SQLite.
    Sqlite,
}

impl Driver {
    /// The port to connect on when none was configured.
    ///
    /// SQLite has no default port: [`ConnectionConfig::resolve`] never calls
    /// this for that driver, since host and port are meaningless for a file.
    pub const fn default_port(self) -> u16 {
        match self {
            Driver::Mysql | Driver::Mariadb => 3306,
            Driver::Sqlsrv => 1433,
            Driver::Postgres => 5432,
            Driver::Sqlite => 0,
        }
    }

    /// The name this driver is selected by.
    pub const fn as_str(self) -> &'static str {
        match self {
            Driver::Mysql => "mysql",
            Driver::Mariadb => "mariadb",
            Driver::Sqlsrv => "sqlsrv",
            Driver::Postgres => "postgres",
            Driver::Sqlite => "sqlite",
        }
    }

    /// Whether this driver connects to a file rather than a host and port.
    pub const fn is_file_based(self) -> bool {
        matches!(self, Driver::Sqlite)
    }
}

impl FromStr for Driver {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "mysql" => Ok(Driver::Mysql),
            "mariadb" => Ok(Driver::Mariadb),
            "sqlsrv" => Ok(Driver::Sqlsrv),
            "postgres" => Ok(Driver::Postgres),
            "sqlite" => Ok(Driver::Sqlite),
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
    /// Database(s) to inspect. Empty means this layer supplies nothing. More
    /// than one entry is only meaningful for SQLite, where the first is the
    /// main database and the rest are attached in order.
    pub database: Vec<String>,
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
                DB_DATABASE => source.database = vec![value],
                DB_USERNAME => source.user = Some(value),
                DB_PASSWORD => source.password = Some(value),
                _ => {}
            }
        }
        Ok(source)
    }

    /// The layer described by asking, for the settings nothing else supplied.
    ///
    /// Reading and writing are parameters rather than the real streams so the
    /// exchange can be tested without a terminal. Callers consult this only
    /// when `stdin` is a terminal; `SPEC.md` §6 makes it the last source
    /// before failing.
    ///
    /// Only settings that are required and still missing are asked for. The
    /// password is never among them: a connection that needs one and does not
    /// have it fails when it is attempted, with a message that says so.
    pub fn from_prompt(
        missing: &[MissingSetting],
        input: impl BufRead,
        mut output: impl Write,
    ) -> Result<Self, ConfigError> {
        let mut source = Self::default();
        let mut lines = input.lines();

        for setting in missing {
            write!(output, "{}: ", setting.prompt()).map_err(ConfigError::Prompt)?;
            output.flush().map_err(ConfigError::Prompt)?;

            let answer = match lines.next() {
                Some(line) => line.map_err(ConfigError::Prompt)?,
                None => break,
            };
            let answer = answer.trim();
            if answer.is_empty() {
                continue;
            }

            match setting {
                MissingSetting::Database => source.database = vec![answer.to_string()],
                MissingSetting::Driver => source.driver = Some(answer.parse()?),
            }
        }

        Ok(source)
    }
}

/// A required setting that no configuration source supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingSetting {
    /// Which database to inspect.
    Database,
    /// Which engine to speak.
    Driver,
}

impl MissingSetting {
    /// What to ask the user for this setting.
    const fn prompt(self) -> &'static str {
        match self {
            MissingSetting::Database => "Database",
            MissingSetting::Driver => "Driver (mysql, mariadb, sqlsrv, postgres or sqlite)",
        }
    }
}

/// The `.dbctx.toml` project configuration file.
///
/// Every key is a long option from `CLI.md` under a single `[dbctx]` table,
/// so the file reads as a saved command line. `password` is deliberately
/// absent: `SPEC.md` §20 says dbctx never persists credentials, and a key
/// that exists only to be rejected is clearer than one that silently works.
///
/// Only the connection keys are consumed today, by [`ProjectConfig::connection`].
/// The rest are parsed and held for the phases that own them, so the file
/// format is settled before anything depends on it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    /// Engine to connect with.
    pub driver: Option<Driver>,
    /// Host to connect to.
    pub host: Option<String>,
    /// Port to connect on.
    pub port: Option<u16>,
    /// Database to inspect.
    pub database: Option<String>,
    /// User to connect as.
    pub user: Option<String>,
    /// Unix socket to connect through.
    pub socket: Option<PathBuf>,
    /// Directory to write artifacts to.
    pub output: Option<PathBuf>,
    /// Which documents to write.
    pub format: Option<String>,
    /// Add deterministic analysis.
    pub analyze: Option<bool>,
    /// Add AI-generated context.
    pub llm: Option<bool>,
    /// Replace artifacts that are already there.
    pub overwrite: Option<bool>,
    /// Skip the Markdown document.
    pub no_markdown: Option<bool>,
    /// Skip the JSON documents.
    pub no_json: Option<bool>,
    /// Skip the Mermaid diagram.
    pub no_mermaid: Option<bool>,
    /// Diagnostic verbosity.
    pub verbose: Option<u8>,
    /// Report errors only.
    pub quiet: Option<bool>,
    /// When to colour output.
    pub color: Option<String>,
    /// How to format log output.
    pub log_format: Option<String>,
    /// SQLite-specific settings, from `[dbctx.sqlite]`.
    pub sqlite: SqliteSection,
}

/// SQLite-specific project settings.
///
/// A separate table rather than flat keys because it only applies to one
/// driver and, unlike the rest of [`ProjectConfig`], has no `CLI.md` option
/// of its own to mirror.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SqliteSection {
    /// Named attachments from `[dbctx.sqlite.attach]`: attachment name to
    /// file path, mirroring `ATTACH DATABASE 'path' AS name`. The main
    /// database configured elsewhere is never a key here.
    pub attach: BTreeMap<String, String>,
}

/// The document `.dbctx.toml` holds.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigDocument {
    dbctx: ProjectConfig,
}

impl ProjectConfig {
    /// Read `.dbctx.toml`, or nothing at all when it is not there.
    ///
    /// Unknown keys are rejected rather than ignored: a misspelled setting
    /// that quietly does nothing is worse than one that says so.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(path = %path.display(), "no project configuration");
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(ConfigError::ConfigFileUnreadable {
                    path: path.to_path_buf(),
                    source: error,
                });
            }
        };

        if let Some(line) = password_line(&text) {
            return Err(ConfigError::PasswordInConfigFile {
                path: path.to_path_buf(),
                line,
            });
        }

        let document: ConfigDocument =
            toml::from_str(&text).map_err(|source| ConfigError::ConfigFile {
                path: path.to_path_buf(),
                source: Box::new(source),
            })?;

        tracing::debug!(path = %path.display(), "read project configuration");
        Ok(document.dbctx)
    }

    /// The connection settings this file states.
    pub fn connection(&self) -> ConnectionSource {
        ConnectionSource {
            driver: self.driver,
            host: self.host.clone(),
            port: self.port,
            database: self.database.clone().into_iter().collect(),
            user: self.user.clone(),
            password: None,
            socket: self.socket.clone(),
        }
    }
}

/// The line number of a `password` key, so the refusal can point at it.
fn password_line(text: &str) -> Option<usize> {
    text.lines().enumerate().find_map(|(index, line)| {
        let line = line.trim();
        let key = line.split('=').next()?.trim();
        (key == "password").then_some(index + 1)
    })
}

/// Resolved connection settings.
///
/// Built once by [`ConnectionConfig::resolve`] and read-only from then on.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    driver: Driver,
    host: String,
    port: u16,
    database: Vec<String>,
    user: Option<String>,
    password: Option<String>,
    socket: Option<PathBuf>,
}

impl ConnectionConfig {
    /// The required settings no source in `sources` supplies.
    ///
    /// Callers use this to decide what to ask for before resolving, so the
    /// prompt asks only for what is genuinely absent. The order matches the
    /// order the settings are asked about.
    pub fn missing(sources: &[ConnectionSource]) -> Vec<MissingSetting> {
        let mut missing = Vec::new();
        if !sources.iter().any(|source| !source.database.is_empty()) {
            missing.push(MissingSetting::Database);
        }
        if !sources.iter().any(|source| source.driver.is_some()) {
            missing.push(MissingSetting::Driver);
        }
        missing
    }

    /// Resolve `sources` into one configuration, taking each field from the
    /// earliest source that supplies it.
    ///
    /// The driver is required. `SPEC.md` §6 has it detected from the
    /// connection, which in practice means the image of a discovered
    /// container; when nothing discovered one and nobody named one, this
    /// fails rather than guessing at an engine.
    ///
    /// The port falls back to the driver's default once the driver is known,
    /// and the host to [`DEFAULT_HOST`].
    ///
    /// More than one `database` value is only meaningful for SQLite, where
    /// the first is the main file and the rest are attached in order; every
    /// other driver connects to exactly one database.
    pub fn resolve(sources: &[ConnectionSource]) -> Result<Self, ConfigError> {
        let pick = |field: fn(&ConnectionSource) -> Option<&str>| {
            sources.iter().find_map(field).map(str::to_string)
        };

        let database = sources
            .iter()
            .find(|source| !source.database.is_empty())
            .map(|source| source.database.clone())
            .ok_or(ConfigError::MissingDatabase)?;
        let driver = sources
            .iter()
            .find_map(|source| source.driver)
            .ok_or(ConfigError::UnknownEngine)?;
        if driver != Driver::Sqlite && database.len() > 1 {
            return Err(ConfigError::MultipleDatabasesRequireSqlite {
                driver,
                count: database.len(),
            });
        }
        let port = sources
            .iter()
            .find_map(|source| source.port)
            .unwrap_or_else(|| driver.default_port());

        let config = Self {
            driver,
            host: pick(|source| source.host.as_deref()).unwrap_or_else(|| DEFAULT_HOST.to_string()),
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
            database = ?config.database,
            user = ?config.user,
            socket = ?config.socket,
            "resolved connection"
        );

        Ok(config)
    }

    /// Driver to connect with.
    pub fn driver(&self) -> Driver {
        self.driver
    }

    /// Host to connect to, defaulted to the loopback address when none was
    /// named.
    ///
    /// A configured [`socket`](Self::socket) takes precedence over this: the
    /// host is still populated, and the code that opens the connection
    /// prefers the socket when there is one.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Port to connect on, defaulted from the driver when none was named.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The main database to inspect: the only database for every driver
    /// except SQLite, where it is the first entry of [`Self::databases`].
    pub fn database(&self) -> &str {
        &self.database[0]
    }

    /// Every configured database, main first, in the order they were
    /// supplied. More than one entry only occurs for SQLite, where the rest
    /// are attached databases.
    pub fn databases(&self) -> &[String] {
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

    /// No source named an engine and nothing discovered one.
    #[error(
        "could not determine the database engine\n\
         dbctx detects the engine from the image of a discovered container; \
         nothing was discovered and no source named one\n\
         try: --driver mysql|mariadb|sqlsrv|postgres|sqlite, or set DB_CONNECTION"
    )]
    UnknownEngine,

    /// A driver name nothing recognises.
    #[error(
        "unknown driver `{value}`\n\
         --driver and DB_CONNECTION select the database engine to connect with\n\
         try one of: mysql, mariadb, sqlsrv, postgres, sqlite"
    )]
    UnknownDriver {
        /// The name that was supplied.
        value: String,
    },

    /// A port that is not a number in range.
    #[error(
        "invalid port `{value}`\n\
         a port must be a whole number between 0 and 65535\n\
         try: --port 3306 for MySQL or MariaDB, --port 1433 for SQL Server, \
         --port 5432 for PostgreSQL"
    )]
    InvalidPort {
        /// The value that was supplied.
        value: String,
    },

    /// More than one `--database` value was given to a driver other than
    /// SQLite.
    #[error(
        "{count} --database values were given, but {driver} connects to a single database\n\
         multiple databases are only meaningful for --driver sqlite, where the \
         first is the main file and the rest are attached\n\
         try: a single --database value, or --driver sqlite"
    )]
    MultipleDatabasesRequireSqlite {
        /// The driver that was configured.
        driver: Driver,
        /// How many `--database` values were given.
        count: usize,
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

    /// A project configuration file that could not be read.
    #[error(
        "could not read `{path}`: {source}\n\
         dbctx reads project settings from this file\n\
         check that it is readable, or remove it"
    )]
    ConfigFileUnreadable {
        /// The file that was being read.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },

    /// A project configuration file that is not valid.
    #[error(
        "could not parse `{path}`: {source}\n\
         settings belong under a [dbctx] table and are named after the long \
         command line options\n\
         try: dbctx init --force to write a fresh file"
    )]
    ConfigFile {
        /// The file that was being read.
        path: PathBuf,
        /// What went wrong.
        source: Box<toml::de::Error>,
    },

    /// A password written into the project configuration file.
    #[error(
        "`{path}` line {line} sets a password\n\
         dbctx never persists credentials, so this file has no password key\n\
         try: --password, DB_PASSWORD, or a .env file that is not committed"
    )]
    PasswordInConfigFile {
        /// The file that was being read.
        path: PathBuf,
        /// Where the key is.
        line: usize,
    },

    /// The prompt could not be read or written.
    #[error(
        "could not read the answer: {0}\n\
         dbctx asks for settings nothing else supplied\n\
         try: supplying them with --database and --driver instead"
    )]
    Prompt(#[source] std::io::Error),
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

    /// A source naming only the database, so the engine is still to be found.
    fn named(database: &str) -> ConnectionSource {
        ConnectionSource {
            database: vec![database.to_string()],
            ..ConnectionSource::default()
        }
    }

    /// A source complete enough to resolve.
    fn complete(database: &str) -> ConnectionSource {
        ConnectionSource {
            driver: Some(Driver::Mysql),
            ..named(database)
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
            ..complete("env-database")
        };

        let config = ConnectionConfig::resolve(&[cli, dotenv, env]).unwrap();

        assert_eq!(config.host(), "cli-host");
        assert_eq!(config.user(), Some("dotenv-user"));
        assert_eq!(config.database(), "env-database");
    }

    #[test]
    fn a_dotenv_file_outranks_the_process_environment() {
        let dotenv = ConnectionSource::from_vars(vars(&[
            ("DB_DATABASE", "from-dotenv"),
            ("DB_CONNECTION", "mysql"),
        ]))
        .unwrap();
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

            assert_eq!(config.port(), port, "{driver}");
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

        assert_eq!(config.port(), 3307);
    }

    #[test]
    fn the_host_falls_back_to_the_loopback_address() {
        let config = ConnectionConfig::resolve(&[complete("shop")]).unwrap();

        assert_eq!(config.host(), DEFAULT_HOST);
        assert_eq!(config.host(), "127.0.0.1");
    }

    #[test]
    fn a_configured_host_beats_the_default() {
        let source = ConnectionSource {
            host: Some("db.internal".to_string()),
            ..complete("shop")
        };

        let config = ConnectionConfig::resolve(&[source]).unwrap();

        assert_eq!(config.host(), "db.internal");
    }

    #[test]
    fn a_socket_connection_still_carries_a_host() {
        let source = ConnectionSource {
            socket: Some(PathBuf::from("/tmp/mysql.sock")),
            ..complete("shop")
        };

        let config = ConnectionConfig::resolve(&[source]).unwrap();

        assert_eq!(config.socket(), Some(Path::new("/tmp/mysql.sock")));
        assert_eq!(config.host(), DEFAULT_HOST);
    }

    #[test]
    fn resolving_without_an_engine_reports_what_to_supply() {
        let error = ConnectionConfig::resolve(&[named("shop")]).unwrap_err();

        assert!(matches!(error, ConfigError::UnknownEngine));
        assert!(error.to_string().contains("--driver"));
    }

    #[test]
    fn missing_settings_are_named_before_anything_is_asked() {
        assert_eq!(
            ConnectionConfig::missing(&[ConnectionSource::default()]),
            [MissingSetting::Database, MissingSetting::Driver]
        );
        assert_eq!(
            ConnectionConfig::missing(&[named("shop")]),
            [MissingSetting::Driver]
        );
        assert_eq!(
            ConnectionConfig::missing(&[ConnectionSource {
                driver: Some(Driver::Mysql),
                ..named("shop")
            }]),
            []
        );
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
                database: vec!["shop".to_string()],
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
        let error = ConnectionSource::from_vars(vars(&[("DB_CONNECTION", "oracle")])).unwrap_err();

        assert!(matches!(error, ConfigError::UnknownDriver { .. }));
        let message = error.to_string();
        assert!(message.contains("oracle"));
        assert!(message.contains("mysql, mariadb, sqlsrv, postgres, sqlite"));
    }

    #[test]
    fn drivers_round_trip_through_their_names() {
        for driver in [
            Driver::Mysql,
            Driver::Mariadb,
            Driver::Sqlsrv,
            Driver::Postgres,
            Driver::Sqlite,
        ] {
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
        assert_eq!(source.database, vec!["shop".to_string()]);
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
    fn a_project_file_supplies_the_connection_settings_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".dbctx.toml");
        std::fs::write(
            &path,
            "[dbctx]\ndriver = \"sqlsrv\"\nhost = \"db.internal\"\nport = 1434\n\
             database = \"shop\"\nuser = \"reader\"\noutput = \"docs/database\"\n",
        )
        .unwrap();

        let project = ProjectConfig::load(&path).unwrap();

        assert_eq!(project.output.as_deref(), Some(Path::new("docs/database")));
        assert_eq!(
            project.connection(),
            ConnectionSource {
                driver: Some(Driver::Sqlsrv),
                host: Some("db.internal".to_string()),
                port: Some(1434),
                database: vec!["shop".to_string()],
                user: Some("reader".to_string()),
                password: None,
                socket: None,
            }
        );
    }

    #[test]
    fn a_project_file_reads_named_sqlite_attachments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".dbctx.toml");
        std::fs::write(
            &path,
            "[dbctx]\ndriver = \"sqlite\"\ndatabase = \"main.db\"\n\n\
             [dbctx.sqlite.attach]\narchive = \"archive.db\"\n",
        )
        .unwrap();

        let project = ProjectConfig::load(&path).unwrap();

        assert_eq!(project.driver, Some(Driver::Sqlite));
        assert_eq!(
            project.sqlite.attach.get("archive"),
            Some(&"archive.db".to_string())
        );
    }

    #[test]
    fn an_unknown_key_under_the_sqlite_table_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".dbctx.toml");
        std::fs::write(
            &path,
            "[dbctx]\ndriver = \"sqlite\"\ndatabase = \"main.db\"\n\n\
             [dbctx.sqlite]\nbogus = true\n",
        )
        .unwrap();

        let error = ProjectConfig::load(&path).unwrap_err();

        assert!(matches!(error, ConfigError::ConfigFile { .. }));
    }

    #[test]
    fn an_absent_project_file_simply_supplies_nothing() {
        let dir = tempfile::tempdir().unwrap();

        let project = ProjectConfig::load(&dir.path().join(".dbctx.toml")).unwrap();

        assert_eq!(project, ProjectConfig::default());
        assert_eq!(project.connection(), ConnectionSource::default());
    }

    #[test]
    fn a_misspelled_key_is_refused_rather_than_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".dbctx.toml");
        std::fs::write(&path, "[dbctx]\ndatabse = \"shop\"\n").unwrap();

        let error = ProjectConfig::load(&path).unwrap_err();

        assert!(matches!(error, ConfigError::ConfigFile { .. }));
        assert!(error.to_string().contains("databse"), "{error}");
    }

    #[test]
    fn a_password_in_the_project_file_is_refused_and_located() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".dbctx.toml");
        std::fs::write(
            &path,
            "[dbctx]\ndatabase = \"shop\"\npassword = \"hunter2\"\n",
        )
        .unwrap();

        let error = ProjectConfig::load(&path).unwrap_err();

        assert!(matches!(error, ConfigError::PasswordInConfigFile { .. }));
        let message = error.to_string();
        assert!(message.contains("line 3"), "{message}");
        assert!(!message.contains("hunter2"), "{message}");
    }

    #[test]
    fn a_settings_table_that_is_not_dbctx_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".dbctx.toml");
        std::fs::write(&path, "[connection]\ndatabase = \"shop\"\n").unwrap();

        let error = ProjectConfig::load(&path).unwrap_err();

        assert!(matches!(error, ConfigError::ConfigFile { .. }));
    }

    #[test]
    fn the_prompt_asks_only_for_what_is_missing() {
        let mut asked = Vec::new();

        let source =
            ConnectionSource::from_prompt(&[MissingSetting::Database], &b"shop\n"[..], &mut asked)
                .unwrap();

        assert_eq!(String::from_utf8(asked).unwrap(), "Database: ");
        assert_eq!(source.database, vec!["shop".to_string()]);
        assert_eq!(source.driver, None);
    }

    #[test]
    fn the_prompt_asks_for_every_missing_setting_in_turn() {
        let mut asked = Vec::new();

        let source = ConnectionSource::from_prompt(
            &[MissingSetting::Database, MissingSetting::Driver],
            &b"shop\nmariadb\n"[..],
            &mut asked,
        )
        .unwrap();

        let asked = String::from_utf8(asked).unwrap();
        assert!(asked.contains("Database: "), "{asked}");
        assert!(
            asked.contains("Driver (mysql, mariadb, sqlsrv, postgres or sqlite): "),
            "{asked}"
        );
        assert_eq!(source.database, vec!["shop".to_string()]);
        assert_eq!(source.driver, Some(Driver::Mariadb));
    }

    #[test]
    fn an_empty_answer_supplies_nothing_rather_than_an_empty_name() {
        let source =
            ConnectionSource::from_prompt(&[MissingSetting::Database], &b"\n"[..], &mut Vec::new())
                .unwrap();

        assert_eq!(source, ConnectionSource::default());
    }

    #[test]
    fn a_closed_input_ends_the_prompt_rather_than_looping() {
        let source = ConnectionSource::from_prompt(
            &[MissingSetting::Database, MissingSetting::Driver],
            &b""[..],
            &mut Vec::new(),
        )
        .unwrap();

        assert_eq!(source, ConnectionSource::default());
    }

    #[test]
    fn an_unusable_answer_to_the_driver_is_reported() {
        let error = ConnectionSource::from_prompt(
            &[MissingSetting::Driver],
            &b"oracle\n"[..],
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(matches!(error, ConfigError::UnknownDriver { .. }));
    }

    #[test]
    fn debug_output_redacts_the_password() {
        let source = ConnectionSource {
            user: Some("reader".to_string()),
            password: Some("hunter2".to_string()),
            ..complete("shop")
        };

        let config = ConnectionConfig::resolve(&[source]).unwrap();
        let debug = format!("{config:?}");

        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(debug.contains("reader"), "{debug}");
        assert_eq!(config.password(), Some("hunter2"));
    }
}
