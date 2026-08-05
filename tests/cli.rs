//! End-to-end checks of the command line contract in `CLI.md`: the exit codes,
//! and the connection sources the binary consults.

use std::path::Path;
use std::process::{Command, Output};

/// Run `dbctx` in `dir` with only the environment variables in `env`, so a
/// test never depends on the developer's own `DB_*` settings.
fn dbctx(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dbctx"));
    command.current_dir(dir).args(args).env_clear();
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("dbctx binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("dbctx writes UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("dbctx writes UTF-8")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("dbctx exits normally")
}

#[test]
fn version_reports_the_library_version() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(dir.path(), &["--version"], &[]);

    assert_eq!(code(&output), 0);
    assert_eq!(stdout(&output).trim(), format!("dbctx {}", dbctx::VERSION));
}

#[test]
fn help_lists_every_documented_command() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(dir.path(), &["--help"], &[]);

    assert_eq!(code(&output), 0);
    let help = stdout(&output);
    for command in ["inspect", "validate", "graph", "diff", "stats", "init"] {
        assert!(help.contains(command), "--help omits {command}: {help}");
    }
}

#[test]
fn invalid_usage_exits_64() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(code(&dbctx(dir.path(), &[], &[])), 64);
    assert_eq!(code(&dbctx(dir.path(), &["nonesuch"], &[])), 64);
    assert_eq!(
        code(&dbctx(dir.path(), &["inspect", "--nonesuch"], &[])),
        64
    );
    assert_eq!(
        code(&dbctx(dir.path(), &["diff", "only-one.json"], &[])),
        64
    );
}

#[test]
fn a_missing_database_is_a_configuration_error() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(dir.path(), &["inspect"], &[]);

    assert_eq!(code(&output), 3);
    assert!(
        stderr(&output).contains("no database was configured"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn validate_command_is_wired_and_reports_missing_database() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(dir.path(), &["validate"], &[]);

    assert_eq!(code(&output), 3);
    assert!(
        stderr(&output).contains("no database was configured"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn graph_command_is_wired_and_reports_missing_database() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(dir.path(), &["graph"], &[]);

    assert_eq!(code(&output), 3);
    assert!(
        stderr(&output).contains("no database was configured"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn stats_command_is_wired_and_reports_missing_database() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(dir.path(), &["stats"], &[]);

    assert_eq!(code(&output), 3);
    assert!(
        stderr(&output).contains("no database was configured"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_database_on_the_command_line_resolves() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(
        dir.path(),
        &["inspect", "--database", "shop", "--driver", "mysql"],
        &[],
    );

    assert_eq!(code(&output), 2, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("could not connect"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_dotenv_file_in_the_working_directory_is_read() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".env"),
        "DB_DATABASE=shop\nDB_CONNECTION=mysql\n",
    )
    .unwrap();

    let output = dbctx(dir.path(), &["inspect"], &[]);

    assert_eq!(code(&output), 2, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("could not connect"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn environment_variables_are_read() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(
        dir.path(),
        &["inspect"],
        &[("DB_DATABASE", "shop"), ("DB_CONNECTION", "mysql")],
    );

    assert_eq!(code(&output), 2, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("could not connect"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_project_file_in_the_working_directory_is_read() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".dbctx.toml"),
        "[dbctx]\ndatabase = \"shop\"\ndriver = \"mariadb\"\n",
    )
    .unwrap();

    let output = dbctx(dir.path(), &["-vv", "inspect"], &[]);

    assert_eq!(code(&output), 2, "{}", stderr(&output));
    let logged = stderr(&output);
    assert!(logged.contains("read project configuration"), "{logged}");
    assert!(logged.contains("could not connect"), "{logged}");
}

#[test]
fn a_project_file_is_outranked_by_the_command_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".dbctx.toml"),
        "[dbctx]\ndatabase = \"shop\"\ndriver = \"mariadb\"\nport = 3307\n",
    )
    .unwrap();

    let output = dbctx(dir.path(), &["-vv", "inspect", "--port", "3399"], &[]);

    let logged = stderr(&output);
    assert!(logged.contains("port=3399"), "{logged}");
}

#[test]
fn a_project_file_outranks_the_environment() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".dbctx.toml"),
        "[dbctx]\ndatabase = \"from-project\"\ndriver = \"mysql\"\n",
    )
    .unwrap();

    let output = dbctx(
        dir.path(),
        &["-vv", "inspect"],
        &[("DB_DATABASE", "from-environment")],
    );

    let logged = stderr(&output);
    assert!(logged.contains("from-project"), "{logged}");
    assert!(!logged.contains("from-environment"), "{logged}");
}

#[test]
fn a_password_in_the_project_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".dbctx.toml"),
        "[dbctx]\ndatabase = \"shop\"\npassword = \"hunter2\"\n",
    )
    .unwrap();

    let output = dbctx(dir.path(), &["inspect"], &[]);

    assert_eq!(code(&output), 3);
    assert!(
        stderr(&output).contains("never persists credentials"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("hunter2"), "{}", stderr(&output));
}

#[test]
fn the_file_init_writes_is_one_dbctx_can_read() {
    let dir = tempfile::tempdir().unwrap();

    let created = dbctx(dir.path(), &["init"], &[]);
    assert_eq!(code(&created), 0, "{}", stderr(&created));

    // Every key is commented out, so this proves the shape parses rather
    // than that it configures anything.
    let output = dbctx(dir.path(), &["inspect", "--database", "shop"], &[]);
    assert!(
        stderr(&output).contains("could not determine the database engine"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_engine_that_cannot_be_determined_is_reported_rather_than_guessed() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(dir.path(), &["inspect", "--database", "shop"], &[]);

    assert_eq!(code(&output), 3);
    let message = stderr(&output);
    assert!(
        message.contains("could not determine the database engine"),
        "{message}"
    );
    assert!(message.contains("--driver"), "{message}");
}

#[test]
fn asking_for_both_docker_selectors_is_refused() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(
        dir.path(),
        &[
            "inspect",
            "--database",
            "shop",
            "--compose-service",
            "db",
            "--docker-container",
            "shop-db-1",
        ],
        &[],
    );

    assert_eq!(code(&output), 3);
    assert!(
        stderr(&output).contains("cannot both be given"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_prompt_is_not_offered_when_nothing_is_attached_to_answer_it() {
    let dir = tempfile::tempdir().unwrap();

    // stdin is a pipe here, not a terminal, so discovery must fail rather
    // than block waiting for an answer that will never arrive.
    let output = dbctx(dir.path(), &["inspect"], &[]);

    assert_eq!(code(&output), 3);
    assert!(
        stderr(&output).contains("no database was configured"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_env_file_that_was_asked_for_but_is_missing_is_a_configuration_error() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(
        dir.path(),
        &["inspect", "--env", "nonesuch.env"],
        &[("DB_DATABASE", "shop")],
    );

    assert_eq!(code(&output), 3);
    assert!(
        stderr(&output).contains("nonesuch.env"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_bad_port_in_the_environment_is_a_configuration_error() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(
        dir.path(),
        &["inspect"],
        &[("DB_DATABASE", "shop"), ("DB_PORT", "not-a-port")],
    );

    assert_eq!(code(&output), 3);
    assert!(
        stderr(&output).contains("not-a-port"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn commands_that_need_no_connection_skip_configuration() {
    let dir = tempfile::tempdir().unwrap();

    let schema = r#"{
  "format": "dbctx.schema",
  "format_version": "1.0",
  "generator": { "name": "dbctx", "version": "0.1.0" },
  "generated_at": "2026-01-01T00:00:00Z",
  "metadata": {
    "database": "shop",
    "engine": "mysql",
    "engine_version": "8.4.0",
    "default_charset": "utf8mb4",
    "default_collation": "utf8mb4_0900_ai_ci"
  },
  "tables": [],
  "views": []
}"#;
    std::fs::write(dir.path().join("old.json"), schema).unwrap();
    std::fs::write(dir.path().join("new.json"), schema).unwrap();

    let output = dbctx(dir.path(), &["diff", "old.json", "new.json"], &[]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("summary"), "{}", stdout(&output));
}

#[test]
fn init_writes_a_configuration_file_and_will_not_replace_it() {
    let dir = tempfile::tempdir().unwrap();

    let created = dbctx(dir.path(), &["init"], &[]);
    assert_eq!(code(&created), 0, "{}", stderr(&created));
    assert!(dir.path().join(".dbctx.toml").exists());

    let refused = dbctx(dir.path(), &["init"], &[]);
    assert_eq!(code(&refused), 1);
    assert!(stderr(&refused).contains("--force"), "{}", stderr(&refused));

    let forced = dbctx(dir.path(), &["init", "--force"], &[]);
    assert_eq!(code(&forced), 0, "{}", stderr(&forced));
}

#[test]
fn a_password_never_reaches_the_output() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(
        dir.path(),
        &[
            "-vvv",
            "inspect",
            "--database",
            "shop",
            "--driver",
            "mysql",
            "--password",
            "hunter2",
        ],
        &[],
    );

    // At -vvv the connection is logged, so this proves the redaction rather
    // than the absence of logging.
    assert!(
        stderr(&output).contains("resolved connection"),
        "{}",
        stderr(&output)
    );
    assert!(!stdout(&output).contains("hunter2"));
    assert!(!stderr(&output).contains("hunter2"));
}

#[test]
fn diagnostics_are_quiet_until_asked_for() {
    let dir = tempfile::tempdir().unwrap();

    let default = dbctx(
        dir.path(),
        &["inspect", "--database", "shop", "--driver", "mysql"],
        &[],
    );
    assert!(
        !stderr(&default).contains("resolved connection"),
        "{}",
        stderr(&default)
    );

    let verbose = dbctx(
        dir.path(),
        &["-vv", "inspect", "--database", "shop", "--driver", "mysql"],
        &[],
    );
    assert!(
        stderr(&verbose).contains("resolved connection"),
        "{}",
        stderr(&verbose)
    );
    assert!(stderr(&verbose).contains("DEBUG"), "{}", stderr(&verbose));
}

#[test]
fn diagnostics_go_to_stderr_leaving_stdout_clean() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(
        dir.path(),
        &["-vv", "inspect", "--database", "shop", "--driver", "mysql"],
        &[],
    );

    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
}

#[test]
fn json_logging_emits_one_object_per_record() {
    let dir = tempfile::tempdir().unwrap();

    let output = dbctx(
        dir.path(),
        &[
            "--log-format",
            "json",
            "-vv",
            "inspect",
            "--database",
            "shop",
            "--driver",
            "mysql",
        ],
        &[],
    );

    let logged = stderr(&output);
    let first = logged.lines().next().expect("a log record");
    let record: serde_json::Value = serde_json::from_str(first).expect("each record is JSON");
    assert_eq!(record["level"], "DEBUG");
    assert_eq!(record["target"], "dbctx::config");
    assert!(logged.contains("resolved connection"), "{logged}");
}

#[test]
fn quiet_suppresses_everything_but_errors() {
    let dir = tempfile::tempdir().unwrap();

    let inspect = dbctx(
        dir.path(),
        &[
            "--quiet",
            "inspect",
            "--database",
            "shop",
            "--driver",
            "mysql",
        ],
        &[],
    );
    assert!(
        !stderr(&inspect).contains("resolved connection"),
        "{}",
        stderr(&inspect)
    );
    assert!(stderr(&inspect).contains("error:"), "{}", stderr(&inspect));

    let init = dbctx(dir.path(), &["--quiet", "init"], &[]);
    assert_eq!(code(&init), 0, "{}", stderr(&init));
    assert!(stdout(&init).is_empty(), "{}", stdout(&init));
    assert!(dir.path().join(".dbctx.toml").exists());
}

#[test]
fn colour_follows_the_option_and_defaults_off_when_piped() {
    let dir = tempfile::tempdir().unwrap();

    for args in [
        vec!["-vv", "inspect", "--database", "shop", "--driver", "mysql"],
        vec![
            "--color",
            "never",
            "-vv",
            "inspect",
            "--database",
            "shop",
            "--driver",
            "mysql",
        ],
    ] {
        let output = dbctx(dir.path(), &args, &[]);
        assert!(
            !stderr(&output).contains('\u{1b}'),
            "{args:?}: {}",
            stderr(&output)
        );
    }

    let forced = dbctx(
        dir.path(),
        &["--color", "always", "-vv", "inspect", "--database", "shop"],
        &[],
    );
    assert!(stderr(&forced).contains('\u{1b}'), "{}", stderr(&forced));
}
