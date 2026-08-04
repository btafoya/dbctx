//! The `dbctx` binary: parse the command line, set up logging, resolve
//! configuration, run the requested command and map failures onto the exit
//! codes `CLI.md` fixes.
//!
//! Command bodies return `anyhow::Result` so they can add context freely.
//! [`run`] converts each into a [`CliError`] variant, which is what carries
//! the exit code, keeping the code table exhaustive and compiler-checked.

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::Parser;
use clap::error::ErrorKind;
use dbctx::cli::{Cli, ColorChoice, Command, ConnectionArgs, GlobalArgs, InitArgs, LogFormat};
use dbctx::config::{ConnectionConfig, ConnectionSource};
use thiserror::Error;
use tracing::Level;

/// Exit code for invalid command line usage.
const EXIT_USAGE: u8 = 64;

/// The file `dbctx init` writes.
const CONFIG_FILE: &str = ".dbctx.toml";

/// Template written by `dbctx init`, mirroring the connection options.
const CONFIG_TEMPLATE: &str = "\
# dbctx project configuration.
#
# These settings rank below command line options and Docker Compose
# autodiscovery, and above .env and environment variables.

[connection]
# driver = \"mysql\"    # mysql, mariadb or sqlsrv
# host = \"127.0.0.1\"
# port = 3306
# database = \"\"
# user = \"\"

[output]
# directory = \".ai/dbctx\"
# format = \"all\"       # json, markdown or all
";

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return match error.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => ExitCode::SUCCESS,
                _ => ExitCode::from(EXIT_USAGE),
            };
        }
    };

    init_logging(&cli.global);

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

/// Send diagnostics to stderr at the level and in the shape the global
/// options ask for.
///
/// Diagnostics go to stderr so that command output on stdout stays clean for
/// piping. `SPEC.md` §17 wants the default quiet, so an uninstructed run
/// reports warnings and errors only.
fn init_logging(global: &GlobalArgs) {
    let level = if global.quiet {
        Level::ERROR
    } else {
        match global.verbose {
            0 => Level::WARN,
            1 => Level::INFO,
            2 => Level::DEBUG,
            _ => Level::TRACE,
        }
    };

    let ansi = match global.color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => std::io::stderr().is_terminal(),
    };

    let builder = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .with_ansi(ansi);

    match global.log_format {
        LogFormat::Text => builder.init(),
        LogFormat::Json => builder.json().init(),
    }
}

/// Run the requested command.
fn run(cli: &Cli) -> Result<(), CliError> {
    let command = cli.command.name();
    match &cli.command {
        Command::Inspect(args) => {
            connect(&args.connection)?;
            Err(CliError::NotImplemented { command })
        }
        Command::Validate(args) | Command::Stats(args) => {
            connect(args)?;
            Err(CliError::NotImplemented { command })
        }
        Command::Graph(args) => {
            connect(&args.connection)?;
            Err(CliError::NotImplemented { command })
        }
        Command::Diff(_) => Err(CliError::NotImplemented { command }),
        Command::Init(args) => {
            init(args, Path::new(CONFIG_FILE)).map_err(CliError::Init)?;
            if !cli.global.quiet {
                println!("wrote {CONFIG_FILE}");
            }
            Ok(())
        }
    }
}

/// Resolve the connection settings for a command that reaches a database.
///
/// The layers are assembled in the order `SPEC.md` §6 fixes. Docker Compose
/// autodiscovery and `.dbctx.toml` sit between the command line and `.env`,
/// and the interactive prompt after the environment; all three arrive with
/// Phase 3.
fn connect(args: &ConnectionArgs) -> dbctx::Result<ConnectionConfig> {
    let (env_file, required) = match &args.env {
        Some(path) => (path.clone(), true),
        None => (PathBuf::from(".env"), false),
    };

    let sources = [
        args.source(),
        ConnectionSource::from_dotenv(&env_file, required)?,
        ConnectionSource::from_env()?,
    ];

    Ok(ConnectionConfig::resolve(&sources)?)
}

/// Write the project configuration file, refusing to replace one that is
/// already there unless `--force` says otherwise.
fn init(args: &InitArgs, path: &Path) -> anyhow::Result<()> {
    if path.exists() && !args.force {
        bail!(
            "`{}` already exists\n\
             dbctx init will not replace a configuration file you may have edited\n\
             try: dbctx init --force",
            path.display()
        );
    }

    fs::write(path, CONFIG_TEMPLATE)
        .with_context(|| format!("could not write `{}`", path.display()))?;

    Ok(())
}

/// A failure worth reporting to the user, carrying the exit code it maps to.
#[derive(Debug, Error)]
enum CliError {
    /// The library could not do what was asked.
    #[error(transparent)]
    Library(#[from] dbctx::Error),

    /// A command that parses and configures but cannot yet run.
    #[error(
        "the {command} command is not implemented yet\n\
         this build of dbctx parses and configures it but cannot run it"
    )]
    NotImplemented {
        /// The command that was asked for.
        command: &'static str,
    },

    /// `dbctx init` could not write the configuration file.
    #[error("{0:#}")]
    Init(anyhow::Error),
}

impl CliError {
    /// The exit code `CLI.md` gives this failure.
    ///
    /// Matching the library error variant by variant is deliberate: a new one
    /// should not compile until it has been given a code.
    fn exit_code(&self) -> u8 {
        match self {
            CliError::Library(dbctx::Error::Config(_)) => 3,
            CliError::NotImplemented { .. } | CliError::Init(_) => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use dbctx::config::ConfigError;

    #[test]
    fn init_writes_a_configuration_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);

        init(&InitArgs { force: false }, &path).unwrap();

        assert!(path.exists());
        assert!(fs::read_to_string(&path).unwrap().contains("[connection]"));
    }

    #[test]
    fn init_refuses_to_replace_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "keep me").unwrap();

        let error = init(&InitArgs { force: false }, &path).unwrap_err();

        assert!(error.to_string().contains("--force"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep me");
        assert_eq!(CliError::Init(error).exit_code(), 1);
    }

    #[test]
    fn init_replaces_an_existing_file_when_forced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "replace me").unwrap();

        init(&InitArgs { force: true }, &path).unwrap();

        assert!(fs::read_to_string(&path).unwrap().contains("[connection]"));
    }

    #[test]
    fn a_failed_write_keeps_the_context_of_what_it_was_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-such-directory").join(CONFIG_FILE);

        let error = init(&InitArgs { force: false }, &path).unwrap_err();

        let reported = format!("{:#}", CliError::Init(error));
        assert!(reported.contains("could not write"), "{reported}");
        assert!(reported.contains(CONFIG_FILE), "{reported}");
    }

    #[test]
    fn configuration_failures_exit_with_the_documented_code() {
        let error = CliError::Library(ConfigError::MissingDatabase.into());

        assert_eq!(error.exit_code(), 3);
    }
}
