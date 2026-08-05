//! Shared helpers for Docker-backed integration tests.
//!
//! Each test starts a disposable container, creates the schema it needs, and
//! the container is removed when it goes out of scope.

use std::process::{Command, Output};
use std::time::Duration;

pub use dbctx::config::{ConnectionConfig, Driver};

/// A container that is removed however the test ends.
pub struct Container {
    pub name: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "--force", &self.name])
            .output();
    }
}

pub fn docker(args: &[&str]) -> Option<Output> {
    Command::new("docker").args(args).output().ok()
}

pub fn docker_available() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"])
        .is_some_and(|output| output.status.success())
}

pub fn image_available(name: &str) -> bool {
    docker(&["image", "inspect", "--format", "{{.Id}}", name])
        .is_some_and(|output| output.status.success())
}

pub fn run(command: &mut Command) -> Output {
    command.output().expect("command runs")
}

pub fn exec_success(output: &Output) -> bool {
    output.status.success()
}

pub fn exec_stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub fn exec_stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

pub fn start_mysql_like(
    image: &str,
    database: &str,
    user: &str,
    password: &str,
) -> Option<Container> {
    let name = format!(
        "dbctx-test-mysql-{}-{}",
        image.replace([':', '/'], "-"),
        std::process::id()
    );
    let root_password_env = if image.starts_with("mariadb:") {
        "MARIADB_ROOT_PASSWORD"
    } else {
        "MYSQL_ROOT_PASSWORD"
    };
    let user_env = if image.starts_with("mariadb:") {
        "MARIADB_USER"
    } else {
        "MYSQL_USER"
    };
    let password_env = if image.starts_with("mariadb:") {
        "MARIADB_PASSWORD"
    } else {
        "MYSQL_PASSWORD"
    };
    let database_env = if image.starts_with("mariadb:") {
        "MARIADB_DATABASE"
    } else {
        "MYSQL_DATABASE"
    };

    let output = docker(&[
        "run",
        "--detach",
        "--name",
        &name,
        "--publish",
        "127.0.0.1::3306",
        "--env",
        &format!("{}={}", database_env, database),
        "--env",
        &format!("{}={}", user_env, user),
        "--env",
        &format!("{}={}", password_env, password),
        "--env",
        &format!("{}={}", root_password_env, password),
        image,
    ])?;
    if !output.status.success() {
        return None;
    }

    let port_output = docker(&["port", &name, "3306/tcp"]).expect("docker port runs");
    let mapping = exec_stdout(&port_output);
    let port: u16 = mapping
        .lines()
        .next()
        .and_then(|line| line.rsplit(':').next())
        .expect("a published port")
        .trim()
        .parse()
        .expect("port is a number");

    let container = Container {
        name,
        port,
        user: user.to_string(),
        password: password.to_string(),
        database: database.to_string(),
    };

    wait_for_mysql_like(&container, password);
    Some(container)
}

pub fn wait_for_mysql_like(container: &Container, root_password: &str) {
    // MariaDB 11+ images no longer ship a `mysql` client binary; use `mariadb`
    // as the fallback so newer images can be exercised in CI.
    let client = if container.name.contains("mariadb-11-") || container.name.contains("mariadb-12-")
    {
        "mariadb"
    } else {
        "mysql"
    };

    for _ in 0..30 {
        let output = run(Command::new("docker")
            .args([
                "exec",
                &container.name,
                client,
                "-uroot",
                &format!("-p{}", root_password),
                "-e",
                "SELECT 1",
            ])
            .env_clear());
        if exec_success(&output) {
            return;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    panic!("mysql container never became healthy");
}

pub fn start_sqlserver(image: &str, password: &str) -> Option<Container> {
    let name = format!("dbctx-test-sqlserver-{}", std::process::id());
    let output = docker(&[
        "run",
        "--detach",
        "--name",
        &name,
        "--publish",
        "127.0.0.1::1433",
        "--env",
        "ACCEPT_EULA=Y",
        "--env",
        &format!("MSSQL_SA_PASSWORD={}", password),
        image,
    ])?;
    if !output.status.success() {
        return None;
    }

    let port_output = docker(&["port", &name, "1433/tcp"]).expect("docker port runs");
    let mapping = exec_stdout(&port_output);
    let port: u16 = mapping
        .lines()
        .next()
        .and_then(|line| line.rsplit(':').next())
        .expect("a published port")
        .trim()
        .parse()
        .expect("port is a number");

    let container = Container {
        name,
        port,
        user: "sa".to_string(),
        password: password.to_string(),
        database: "master".to_string(),
    };

    wait_for_sqlserver(&container, password);
    Some(container)
}

pub fn wait_for_sqlserver(container: &Container, password: &str) {
    for _ in 0..60 {
        let output = run(Command::new("docker")
            .args([
                "exec",
                &container.name,
                "/opt/mssql-tools18/bin/sqlcmd",
                "-b",
                "-S",
                "localhost",
                "-U",
                "SA",
                "-P",
                password,
                "-C",
                "-Q",
                "SELECT 1",
            ])
            .env_clear());
        if exec_success(&output) {
            return;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    panic!("sqlserver container never became healthy");
}

pub fn start_postgres(
    image: &str,
    database: &str,
    user: &str,
    password: &str,
) -> Option<Container> {
    let name = format!("dbctx-test-postgres-{}", std::process::id());
    let output = docker(&[
        "run",
        "--detach",
        "--name",
        &name,
        "--publish",
        "127.0.0.1::5432",
        "--env",
        &format!("POSTGRES_DB={database}"),
        "--env",
        &format!("POSTGRES_USER={user}"),
        "--env",
        &format!("POSTGRES_PASSWORD={password}"),
        image,
    ])?;
    if !output.status.success() {
        return None;
    }

    let port_output = docker(&["port", &name, "5432/tcp"]).expect("docker port runs");
    let mapping = exec_stdout(&port_output);
    let port: u16 = mapping
        .lines()
        .next()
        .and_then(|line| line.rsplit(':').next())
        .expect("a published port")
        .trim()
        .parse()
        .expect("port is a number");

    let container = Container {
        name,
        port,
        user: user.to_string(),
        password: password.to_string(),
        database: database.to_string(),
    };

    wait_for_postgres(&container);
    Some(container)
}

pub fn wait_for_postgres(container: &Container) {
    for _ in 0..30 {
        let output = run(Command::new("docker")
            .args([
                "exec",
                &container.name,
                "pg_isready",
                "-U",
                &container.user,
                "-d",
                &container.database,
            ])
            .env_clear());
        if exec_success(&output) {
            return;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    panic!("postgres container never became healthy");
}

pub fn test_image(var: &str, default: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| default.to_string())
}

pub fn make_config(
    driver: Driver,
    host: &str,
    port: u16,
    database: &str,
    user: &str,
    password: &str,
) -> ConnectionConfig {
    let source = dbctx::config::ConnectionSource {
        driver: Some(driver),
        host: Some(host.to_string()),
        port: Some(port),
        database: vec![database.to_string()],
        user: Some(user.to_string()),
        password: Some(password.to_string()),
        ..Default::default()
    };
    dbctx::config::ConnectionConfig::resolve(&[source]).expect("test config resolves")
}
