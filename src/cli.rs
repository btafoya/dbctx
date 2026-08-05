//! The command line interface defined by `CLI.md`.
//!
//! This module only describes and parses the interface. Commands are run by
//! the binary, and the connection settings gathered here become a
//! [`crate::config::ConnectionConfig`] through [`ConnectionArgs::source`].
//!
//! Command names, long option names and exit codes are stable within a major
//! version, so changes here are public API changes.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::config::{ConnectionSource, Driver};

/// Generate deterministic database context.
/// Running without a command is invalid usage rather than a request for
/// help, so it reports the error and exits 64 like any other usage mistake.
#[derive(Debug, Parser)]
#[command(
    name = "dbctx",
    version,
    about,
    long_about = None,
    subcommand_required = true,
    arg_required_else_help = false
)]
pub struct Cli {
    /// Options that apply to every command.
    #[command(flatten)]
    pub global: GlobalArgs,

    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Options accepted alongside any command.
#[derive(Debug, Args)]
pub struct GlobalArgs {
    /// Show more detail; repeat for more still.
    #[arg(short, long, action = clap::ArgAction::Count, global = true, conflicts_with = "quiet")]
    pub verbose: u8,

    /// Suppress all output except errors.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// When to colour output.
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto, global = true)]
    pub color: ColorChoice,

    /// How to format log output.
    #[arg(long, value_enum, default_value_t = LogFormat::Text, global = true)]
    pub log_format: LogFormat,
}

/// When to colour output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    /// Colour when writing to a terminal.
    Auto,
    /// Always colour.
    Always,
    /// Never colour.
    Never,
}

/// How to format log output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogFormat {
    /// Human-readable lines.
    Text,
    /// One JSON object per record.
    Json,
}

/// Which documents an inspection writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// JSON only.
    Json,
    /// Markdown only.
    Markdown,
    /// Every format.
    All,
}

/// The commands `CLI.md` defines.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect a database and generate artifacts.
    Inspect(InspectArgs),
    /// Report validation findings for the inspected schema.
    Validate(ConnectionArgs),
    /// Generate a Mermaid ER diagram.
    Graph(GraphArgs),
    /// Compare two exported schemas.
    Diff(DiffArgs),
    /// Display schema statistics.
    Stats(ConnectionArgs),
    /// Initialize a project.
    Init(InitArgs),
}

impl Command {
    /// The name this command is invoked by.
    pub const fn name(&self) -> &'static str {
        match self {
            Command::Inspect(_) => "inspect",
            Command::Validate(_) => "validate",
            Command::Graph(_) => "graph",
            Command::Diff(_) => "diff",
            Command::Stats(_) => "stats",
            Command::Init(_) => "init",
        }
    }
}

/// How to reach the database.
#[derive(Debug, Args)]
pub struct ConnectionArgs {
    /// Host to connect to.
    #[arg(long)]
    pub host: Option<String>,

    /// Port to connect on. Defaults to 3306 for MySQL and MariaDB, 1433 for
    /// SQL Server.
    #[arg(long)]
    pub port: Option<u16>,

    /// Database to inspect.
    #[arg(long)]
    pub database: Option<String>,

    /// User to connect as.
    #[arg(long)]
    pub user: Option<String>,

    /// Password to connect with.
    #[arg(long)]
    pub password: Option<String>,

    /// Database engine. Detected from the connection when omitted.
    #[arg(long, value_name = "mysql|mariadb|sqlsrv")]
    pub driver: Option<Driver>,

    /// Unix socket to connect through. MySQL and MariaDB only.
    #[arg(long)]
    pub socket: Option<PathBuf>,

    /// Environment file to read connection settings from.
    #[arg(long, value_name = "FILE")]
    pub env: Option<PathBuf>,

    /// Docker Compose service to discover the connection from.
    #[arg(long, value_name = "SERVICE")]
    pub compose_service: Option<String>,

    /// Docker container to discover the connection from.
    #[arg(long, value_name = "CONTAINER")]
    pub docker_container: Option<String>,
}

impl ConnectionArgs {
    /// The connection settings these options state.
    ///
    /// This is the highest priority layer, so anything named here wins over
    /// every other source.
    pub fn source(&self) -> ConnectionSource {
        ConnectionSource {
            driver: self.driver,
            host: self.host.clone(),
            port: self.port,
            database: self.database.clone(),
            user: self.user.clone(),
            password: self.password.clone(),
            socket: self.socket.clone(),
        }
    }
}

/// Options for `dbctx inspect`.
#[derive(Debug, Args)]
pub struct InspectArgs {
    /// How to reach the database.
    #[command(flatten)]
    pub connection: ConnectionArgs,

    /// Directory to write artifacts to.
    #[arg(
        long,
        value_name = "DIR",
        default_value = ".ai/dbctx",
        conflicts_with = "stdout"
    )]
    pub output: PathBuf,

    /// Write to standard output instead of a directory.
    #[arg(long)]
    pub stdout: bool,

    /// Which documents to write.
    #[arg(long, value_enum, default_value_t = Format::All)]
    pub format: Format,

    /// Add deterministic analysis.
    #[arg(long)]
    pub analyze: bool,

    /// Add AI-generated context, clearly labelled.
    #[arg(long)]
    pub llm: bool,

    /// Replace artifacts that are already there.
    #[arg(long)]
    pub overwrite: bool,

    /// Skip the Markdown document.
    #[arg(long)]
    pub no_markdown: bool,

    /// Skip the JSON documents.
    #[arg(long)]
    pub no_json: bool,

    /// Skip the Mermaid diagram.
    #[arg(long)]
    pub no_mermaid: bool,
}

/// Options for `dbctx graph`.
#[derive(Debug, Args)]
pub struct GraphArgs {
    /// How to reach the database.
    #[command(flatten)]
    pub connection: ConnectionArgs,

    /// File to write the diagram to.
    #[arg(long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

/// Options for `dbctx diff`.
///
/// Diff reads exported documents rather than databases, so it takes no
/// connection options.
#[derive(Debug, Args)]
pub struct DiffArgs {
    /// The earlier schema document.
    pub old: PathBuf,

    /// The later schema document.
    pub new: PathBuf,
}

/// Options for `dbctx init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Overwrite a configuration file that is already there.
    #[arg(long)]
    pub force: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::CommandFactory;
    use clap::error::ErrorKind;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("arguments parse")
    }

    fn parse_error(args: &[&str]) -> ErrorKind {
        Cli::try_parse_from(args)
            .expect_err("arguments rejected")
            .kind()
    }

    #[test]
    fn the_interface_is_internally_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_documented_command_is_accepted() {
        for (args, name) in [
            (vec!["dbctx", "inspect"], "inspect"),
            (vec!["dbctx", "validate"], "validate"),
            (vec!["dbctx", "graph"], "graph"),
            (vec!["dbctx", "diff", "old.json", "new.json"], "diff"),
            (vec!["dbctx", "stats"], "stats"),
            (vec!["dbctx", "init"], "init"),
        ] {
            assert_eq!(parse(&args).command.name(), name);
        }
    }

    #[test]
    fn connection_options_become_the_highest_priority_source() {
        let cli = parse(&[
            "dbctx",
            "inspect",
            "--host",
            "db.internal",
            "--port",
            "3307",
            "--database",
            "shop",
            "--user",
            "reader",
            "--password",
            "secret",
            "--driver",
            "mariadb",
            "--socket",
            "/tmp/mysql.sock",
        ]);

        let Command::Inspect(args) = cli.command else {
            panic!("expected inspect");
        };
        assert_eq!(
            args.connection.source(),
            ConnectionSource {
                driver: Some(Driver::Mariadb),
                host: Some("db.internal".to_string()),
                port: Some(3307),
                database: Some("shop".to_string()),
                user: Some("reader".to_string()),
                password: Some("secret".to_string()),
                socket: Some(PathBuf::from("/tmp/mysql.sock")),
            }
        );
    }

    #[test]
    fn omitted_connection_options_state_nothing() {
        let cli = parse(&["dbctx", "inspect"]);

        let Command::Inspect(args) = cli.command else {
            panic!("expected inspect");
        };
        assert_eq!(args.connection.source(), ConnectionSource::default());
    }

    #[test]
    fn inspect_defaults_to_writing_every_format_under_the_documented_directory() {
        let cli = parse(&["dbctx", "inspect"]);

        let Command::Inspect(args) = cli.command else {
            panic!("expected inspect");
        };
        assert_eq!(args.output, PathBuf::from(".ai/dbctx"));
        assert_eq!(args.format, Format::All);
        assert!(!args.stdout);
        assert!(!args.analyze);
        assert!(!args.llm);
        assert!(!args.overwrite);
    }

    #[test]
    fn inspect_format_can_be_limited_to_json_or_markdown() {
        let json = parse(&["dbctx", "inspect", "--format", "json"]);
        let markdown = parse(&["dbctx", "inspect", "--format", "markdown"]);

        let Command::Inspect(json_args) = json.command else {
            panic!("expected inspect");
        };
        let Command::Inspect(markdown_args) = markdown.command else {
            panic!("expected inspect");
        };

        assert_eq!(json_args.format, Format::Json);
        assert_eq!(markdown_args.format, Format::Markdown);
    }

    #[test]
    fn no_markdown_and_no_json_flags_parse() {
        let cli = parse(&["dbctx", "inspect", "--no-markdown", "--no-json"]);

        let Command::Inspect(args) = cli.command else {
            panic!("expected inspect");
        };
        assert!(args.no_markdown);
        assert!(args.no_json);
    }

    #[test]
    fn output_and_stdout_cannot_both_be_given() {
        assert_eq!(
            parse_error(&["dbctx", "inspect", "--output", "docs", "--stdout"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn verbose_and_quiet_cannot_both_be_given() {
        assert_eq!(
            parse_error(&["dbctx", "--verbose", "--quiet", "inspect"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn verbosity_counts_up() {
        assert_eq!(parse(&["dbctx", "inspect"]).global.verbose, 0);
        assert_eq!(parse(&["dbctx", "-v", "inspect"]).global.verbose, 1);
        assert_eq!(parse(&["dbctx", "-vvv", "inspect"]).global.verbose, 3);
    }

    #[test]
    fn global_options_are_accepted_before_and_after_the_command() {
        assert_eq!(parse(&["dbctx", "-vv", "inspect"]).global.verbose, 2);
        assert_eq!(parse(&["dbctx", "inspect", "-vv"]).global.verbose, 2);
    }

    #[test]
    fn global_options_default_to_the_documented_values() {
        let cli = parse(&["dbctx", "inspect"]);

        assert_eq!(cli.global.color, ColorChoice::Auto);
        assert_eq!(cli.global.log_format, LogFormat::Text);
        assert!(!cli.global.quiet);
    }

    #[test]
    fn an_unknown_driver_is_rejected_by_the_parser() {
        assert_eq!(
            parse_error(&["dbctx", "inspect", "--driver", "postgres"]),
            ErrorKind::ValueValidation
        );
    }

    #[test]
    fn diff_takes_two_documents_and_no_connection() {
        let cli = parse(&[
            "dbctx",
            "diff",
            "previous/schema.json",
            "current/schema.json",
        ]);

        let Command::Diff(args) = cli.command else {
            panic!("expected diff");
        };
        assert_eq!(args.old, PathBuf::from("previous/schema.json"));
        assert_eq!(args.new, PathBuf::from("current/schema.json"));
        assert_eq!(
            parse_error(&["dbctx", "diff", "only-one.json"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn init_only_overwrites_when_forced() {
        let Command::Init(args) = parse(&["dbctx", "init"]).command else {
            panic!("expected init");
        };
        assert!(!args.force);

        let Command::Init(forced) = parse(&["dbctx", "init", "--force"]).command else {
            panic!("expected init");
        };
        assert!(forced.force);
    }

    #[test]
    fn every_documented_example_parses() {
        for example in [
            vec!["dbctx", "inspect"],
            vec!["dbctx", "inspect", "--analyze"],
            vec!["dbctx", "inspect", "--llm"],
            vec!["dbctx", "inspect", "--compose-service", "mariadb"],
            vec!["dbctx", "inspect", "--output", "docs/database"],
            vec![
                "dbctx",
                "diff",
                "previous/schema.json",
                "current/schema.json",
            ],
            vec!["dbctx", "validate"],
            vec!["dbctx", "graph", "--output", "graph.mmd"],
        ] {
            Cli::try_parse_from(&example).unwrap_or_else(|error| panic!("{example:?}: {error}"));
        }
    }
}
