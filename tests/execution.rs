//! End-to-end read-only SQL execution against real Docker databases.
//!
//! Tests verify that `dbctx::execution::execute` runs allowed statements,
//! rejects mutating and multi-statement queries, and returns JSON-shaped
//! results. They skip gracefully when docker or the required image is not
//! available.

mod common;

use std::process::Command;
use std::time::Duration;

use common::*;

fn seed_mysql(container: &Container) {
    let sql = format!(
        r#"
        CREATE DATABASE IF NOT EXISTS {db};
        USE {db};
        CREATE TABLE widgets (
            id INT PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        );
        INSERT INTO widgets (id, name) VALUES (1, 'alpha'), (2, 'beta');
        "#,
        db = container.database
    );
    // MariaDB 11+ images ship `mariadb` rather than `mysql`.
    let client = if container.name.contains("mariadb-11-") || container.name.contains("mariadb-12-")
    {
        "mariadb"
    } else {
        "mysql"
    };
    let output = run(Command::new("docker")
        .args([
            "exec",
            "-i",
            &container.name,
            client,
            "-uroot",
            &format!("-p{}", container.password),
            "-e",
            &sql,
        ])
        .env_clear());
    assert!(
        exec_success(&output),
        "could not seed mysql: {}",
        exec_stderr(&output)
    );
}

fn seed_sqlserver(container: &Container) {
    let create_db = run(Command::new("docker")
        .args([
            "exec",
            "-i",
            &container.name,
            "/opt/mssql-tools18/bin/sqlcmd",
            "-b",
            "-S",
            "localhost",
            "-U",
            "SA",
            "-P",
            &container.password,
            "-C",
            "-Q",
            "IF NOT EXISTS (SELECT * FROM sys.databases WHERE name = 'exec_test') CREATE DATABASE exec_test;",
        ])
        .env_clear());
    assert!(
        exec_success(&create_db),
        "could not create sqlserver database: stdout={} stderr={}",
        exec_stdout(&create_db),
        exec_stderr(&create_db)
    );

    let sql = "
        CREATE TABLE dbo.widgets (
            id INT PRIMARY KEY,
            name NVARCHAR(255) NOT NULL
        );
        INSERT INTO dbo.widgets (id, name) VALUES (1, 'alpha'), (2, 'beta');
    ";
    let output = run(Command::new("docker")
        .args([
            "exec",
            "-i",
            &container.name,
            "/opt/mssql-tools18/bin/sqlcmd",
            "-b",
            "-S",
            "localhost",
            "-U",
            "SA",
            "-P",
            &container.password,
            "-C",
            "-d",
            "exec_test",
            "-Q",
            sql,
        ])
        .env_clear());
    assert!(
        exec_success(&output),
        "could not seed sqlserver: stdout={} stderr={}",
        exec_stdout(&output),
        exec_stderr(&output)
    );
}

fn seed_postgres(container: &Container) {
    let sql = "
        CREATE TABLE widgets (
            id INTEGER PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        );
        INSERT INTO widgets (id, name) VALUES (1, 'alpha'), (2, 'beta');
    ";
    let output = run(Command::new("docker")
        .args([
            "exec",
            "-i",
            &container.name,
            "psql",
            "-U",
            &container.user,
            "-d",
            &container.database,
            "-v",
            "ON_ERROR_STOP=1",
            "-c",
            sql,
        ])
        .env_clear());
    assert!(
        exec_success(&output),
        "could not seed postgres: {}",
        exec_stderr(&output)
    );
}

fn assert_widgets_result(result: &dbctx::execution::ExecutionResult) {
    assert_eq!(result.columns, vec!["id", "name"]);
    assert_eq!(result.row_count, 2);
    assert_eq!(result.rows[0][0], serde_json::json!(1));
    assert_eq!(result.rows[0][1], serde_json::json!("alpha"));
    assert_eq!(result.rows[1][0], serde_json::json!(2));
    assert_eq!(result.rows[1][1], serde_json::json!("beta"));
}

#[tokio::test]
async fn mysql_execution_runs_selects_and_rejects_writes() {
    if !docker_available() {
        eprintln!("skipping: no docker daemon");
        return;
    }
    let image = test_image("DBCTX_TEST_MYSQL_IMAGE", "mysql:8.4");
    if !image_available(&image) {
        eprintln!("skipping: {image} image not present");
        return;
    }

    let container =
        start_mysql_like(&image, "exec_test", "reader", "hunter2").expect("mysql container starts");
    seed_mysql(&container);
    let config = make_config(
        Driver::Mysql,
        "127.0.0.1",
        container.port,
        &container.database,
        &container.user,
        &container.password,
    );

    let result = dbctx::execution::execute(
        &config,
        "SELECT id, name FROM widgets ORDER BY id",
        Duration::from_secs(30),
    )
    .await
    .expect("select succeeds");
    assert_widgets_result(&result);

    let error = dbctx::execution::execute(
        &config,
        "INSERT INTO widgets (id, name) VALUES (3, 'gamma')",
        Duration::from_secs(30),
    )
    .await
    .expect_err("insert is rejected");
    assert!(
        matches!(error, dbctx::execution::ExecutionError::NotReadOnly { .. }),
        "expected NotReadOnly, got {error:?}"
    );

    let error = dbctx::execution::execute(&config, "SELECT 1; SELECT 2", Duration::from_secs(30))
        .await
        .expect_err("multi-statement is rejected");
    assert!(
        matches!(error, dbctx::execution::ExecutionError::MultipleStatements),
        "expected MultipleStatements, got {error:?}"
    );

    let after_rejection = dbctx::execution::execute(
        &config,
        "SELECT COUNT(*) AS count FROM widgets",
        Duration::from_secs(30),
    )
    .await
    .expect("count still succeeds");
    assert_eq!(after_rejection.row_count, 1);
    assert_eq!(after_rejection.rows[0][0], serde_json::json!(2));
}

#[tokio::test]
async fn mariadb_execution_runs_selects_and_rejects_writes() {
    if !docker_available() {
        eprintln!("skipping: no docker daemon");
        return;
    }
    let image = test_image("DBCTX_TEST_MARIADB_IMAGE", "mariadb:11");
    if !image_available(&image) {
        eprintln!("skipping: {image} image not present");
        return;
    }

    let container = start_mysql_like(&image, "exec_test", "reader", "hunter2")
        .expect("mariadb container starts");
    seed_mysql(&container);
    let config = make_config(
        Driver::Mariadb,
        "127.0.0.1",
        container.port,
        &container.database,
        &container.user,
        &container.password,
    );

    let result = dbctx::execution::execute(
        &config,
        "SELECT id, name FROM widgets ORDER BY id",
        Duration::from_secs(30),
    )
    .await
    .expect("select succeeds");
    assert_widgets_result(&result);

    let error = dbctx::execution::execute(
        &config,
        "UPDATE widgets SET name = 'changed' WHERE id = 1",
        Duration::from_secs(30),
    )
    .await
    .expect_err("update is rejected");
    assert!(
        matches!(error, dbctx::execution::ExecutionError::NotReadOnly { .. }),
        "expected NotReadOnly, got {error:?}"
    );

    let error = dbctx::execution::execute(
        &config,
        "SELECT 1; DROP TABLE widgets",
        Duration::from_secs(30),
    )
    .await
    .expect_err("multi-statement is rejected");
    assert!(
        matches!(error, dbctx::execution::ExecutionError::MultipleStatements),
        "expected MultipleStatements, got {error:?}"
    );
}

#[tokio::test]
async fn sqlserver_execution_runs_selects_and_rejects_writes() {
    if !docker_available() {
        eprintln!("skipping: no docker daemon");
        return;
    }
    let image = test_image(
        "DBCTX_TEST_SQLSERVER_IMAGE",
        "mcr.microsoft.com/mssql/server:2022-latest",
    );
    if !image_available(&image) {
        eprintln!("skipping: {image} image not present");
        return;
    }

    let container = start_sqlserver(&image, "Hunter2hunter2").expect("sqlserver container starts");
    seed_sqlserver(&container);
    let config = make_config(
        Driver::Sqlsrv,
        "127.0.0.1",
        container.port,
        "exec_test",
        &container.user,
        &container.password,
    );

    let result = dbctx::execution::execute(
        &config,
        "SELECT id, name FROM dbo.widgets ORDER BY id",
        Duration::from_secs(30),
    )
    .await
    .expect("select succeeds");
    assert_widgets_result(&result);

    let error = dbctx::execution::execute(
        &config,
        "DELETE FROM dbo.widgets WHERE id = 1",
        Duration::from_secs(30),
    )
    .await
    .expect_err("delete is rejected");
    assert!(
        matches!(error, dbctx::execution::ExecutionError::NotReadOnly { .. }),
        "expected NotReadOnly, got {error:?}"
    );

    let error = dbctx::execution::execute(&config, "SELECT 1; SELECT 2", Duration::from_secs(30))
        .await
        .expect_err("multi-statement is rejected");
    assert!(
        matches!(error, dbctx::execution::ExecutionError::MultipleStatements),
        "expected MultipleStatements, got {error:?}"
    );
}

#[tokio::test]
async fn postgres_execution_runs_selects_and_rejects_writes() {
    if !docker_available() {
        eprintln!("skipping: no docker daemon");
        return;
    }
    let image = test_image("DBCTX_TEST_POSTGRES_IMAGE", "postgres:17");
    if !image_available(&image) {
        eprintln!("skipping: {image} image not present");
        return;
    }

    let container = start_postgres(&image, "exec_test", "reader", "hunter2")
        .expect("postgres container starts");
    seed_postgres(&container);
    let config = make_config(
        Driver::Postgres,
        "127.0.0.1",
        container.port,
        &container.database,
        &container.user,
        &container.password,
    );

    let result = dbctx::execution::execute(
        &config,
        "SELECT id, name FROM widgets ORDER BY id",
        Duration::from_secs(30),
    )
    .await
    .expect("select succeeds");
    assert_widgets_result(&result);

    let error = dbctx::execution::execute(
        &config,
        "DELETE FROM widgets WHERE id = 1",
        Duration::from_secs(30),
    )
    .await
    .expect_err("delete is rejected");
    assert!(
        matches!(error, dbctx::execution::ExecutionError::NotReadOnly { .. }),
        "expected NotReadOnly, got {error:?}"
    );

    let error = dbctx::execution::execute(&config, "SELECT 1; SELECT 2", Duration::from_secs(30))
        .await
        .expect_err("multi-statement is rejected");
    assert!(
        matches!(error, dbctx::execution::ExecutionError::MultipleStatements),
        "expected MultipleStatements, got {error:?}"
    );
}
