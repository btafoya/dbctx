//! End-to-end introspection against real Docker databases.
//!
//! Each test starts a disposable container, creates a small schema, calls
//! `dbctx::inspect`, and asserts on the returned canonical model. Tests skip
//! gracefully when docker or the required image is not available.

use std::process::{Command, Output};
use std::time::Duration;

use dbctx::config::{ConnectionConfig, Driver};

/// A container that is removed however the test ends.
struct Container {
    name: String,
    port: u16,
    user: String,
    password: String,
    database: String,
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "--force", &self.name])
            .output();
    }
}

fn docker(args: &[&str]) -> Option<Output> {
    Command::new("docker").args(args).output().ok()
}

fn docker_available() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"])
        .is_some_and(|output| output.status.success())
}

fn image_available(name: &str) -> bool {
    docker(&["image", "inspect", "--format", "{{.Id}}", name])
        .is_some_and(|output| output.status.success())
}

fn run(command: &mut Command) -> Output {
    command.output().expect("command runs")
}

fn exec_success(output: &Output) -> bool {
    output.status.success()
}

fn exec_stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn exec_stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

fn start_mysql_like(image: &str, database: &str, user: &str, password: &str) -> Option<Container> {
    let name = format!(
        "dbctx-introspection-mysql-{}-{}",
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

    wait_for_mysql_like(&container, image, database, user, password, password);
    Some(container)
}

fn wait_for_mysql_like(
    container: &Container,
    image: &str,
    database: &str,
    user: &str,
    password: &str,
    root_password: &str,
) {
    let _ = (image, database, user, password);
    for _ in 0..30 {
        let output = run(Command::new("docker")
            .args([
                "exec",
                &container.name,
                "mysql",
                "-uroot",
                &format!("-p{}", root_password),
                "-e",
                "SELECT 1",
            ])
            .env_clear());
        if exec_success(&output) {
            create_mysql_schema(
                &container.name,
                &container.database,
                &container.user,
                root_password,
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    panic!("mysql container never became healthy");
}

fn create_mysql_schema(container: &str, database: &str, user: &str, root_password: &str) {
    let sql = format!(
        r#"
        CREATE DATABASE IF NOT EXISTS {database};
        USE {database};
        CREATE TABLE customers (
            id INT AUTO_INCREMENT PRIMARY KEY,
            email VARCHAR(255) NOT NULL UNIQUE,
            name VARCHAR(255)
        );
        CREATE TABLE orders (
            id INT AUTO_INCREMENT PRIMARY KEY,
            customer_id INT NOT NULL,
            total DECIMAL(10,2),
            FOREIGN KEY (customer_id) REFERENCES customers(id)
        );
        CREATE INDEX idx_orders_total ON orders(total);
        CREATE VIEW recent_orders AS SELECT id, customer_id, total FROM orders;
        GRANT ALL PRIVILEGES ON {database}.* TO '{user}'@'%';
        FLUSH PRIVILEGES;
        "#
    );
    let output = run(Command::new("docker")
        .args([
            "exec",
            "-i",
            container,
            "mysql",
            "-uroot",
            &format!("-p{}", root_password),
            "-e",
            &sql,
        ])
        .env_clear());
    assert!(
        exec_success(&output),
        "could not create mysql schema: {}",
        exec_stderr(&output)
    );
}

fn start_sqlserver(image: &str, password: &str) -> Option<Container> {
    let name = format!("dbctx-introspection-sqlserver-{}", std::process::id());
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

fn wait_for_sqlserver(container: &Container, password: &str) {
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
            create_sqlserver_schema(&container.name, password);
            return;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    panic!("sqlserver container never became healthy");
}

fn create_sqlserver_schema(container: &str, password: &str) {
    let create_db_output = run(Command::new("docker")
        .args([
            "exec",
            "-i",
            container,
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
            "IF NOT EXISTS (SELECT * FROM sys.databases WHERE name = 'shop') CREATE DATABASE shop;",
        ])
        .env_clear());
    assert!(
        exec_success(&create_db_output),
        "could not create sqlserver database: stdout={} stderr={}",
        exec_stdout(&create_db_output),
        exec_stderr(&create_db_output)
    );

    let sql = r#"
        CREATE TABLE dbo.customers (
            id INT IDENTITY(1,1) CONSTRAINT [PRIMARY] PRIMARY KEY,
            email NVARCHAR(255) NOT NULL CONSTRAINT [UQ_email] UNIQUE,
            name NVARCHAR(255)
        );
        CREATE TABLE dbo.orders (
            id INT IDENTITY(1,1) PRIMARY KEY,
            customer_id INT NOT NULL,
            total DECIMAL(10,2),
            FOREIGN KEY (customer_id) REFERENCES dbo.customers(id)
        );
        CREATE INDEX idx_orders_total ON dbo.orders(total);
        GO
        CREATE VIEW dbo.recent_orders AS SELECT id, customer_id, total FROM dbo.orders;
    "#;
    let output = run(Command::new("docker")
        .args([
            "exec",
            "-i",
            container,
            "/opt/mssql-tools18/bin/sqlcmd",
            "-b",
            "-S",
            "localhost",
            "-U",
            "SA",
            "-P",
            password,
            "-C",
            "-d",
            "shop",
            "-Q",
            sql,
        ])
        .env_clear());
    assert!(
        exec_success(&output),
        "could not create sqlserver schema: stdout={} stderr={}",
        exec_stdout(&output),
        exec_stderr(&output)
    );
}

fn test_image(var: &str, default: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| default.to_string())
}

fn make_config(
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
        database: Some(database.to_string()),
        user: Some(user.to_string()),
        password: Some(password.to_string()),
        ..Default::default()
    };
    dbctx::config::ConnectionConfig::resolve(&[source]).expect("test config resolves")
}

fn assert_common_schema(database: &dbctx::model::Database) {
    assert_eq!(database.tables.len(), 2);
    assert_eq!(database.views.len(), 1);

    let customers = database
        .tables
        .iter()
        .find(|t| t.name == "customers")
        .expect("customers table");
    let orders = database
        .tables
        .iter()
        .find(|t| t.name == "orders")
        .expect("orders table");

    assert!(
        customers
            .columns
            .iter()
            .any(|c| c.name == "id" && c.primary_key)
    );
    assert!(
        customers
            .columns
            .iter()
            .any(|c| c.name == "email" && c.unique)
    );
    assert!(
        orders
            .columns
            .iter()
            .any(|c| c.name == "customer_id" && !c.nullable)
    );

    assert!(customers.indexes.iter().any(|i| i.name == "PRIMARY"));
    assert!(orders.indexes.iter().any(|i| i.name == "idx_orders_total"));

    assert_eq!(orders.foreign_keys.len(), 1);
    assert_eq!(orders.foreign_keys[0].referenced_table, "customers");

    let relationships = database.relationships();
    assert_eq!(relationships.len(), 1);
    assert_eq!(relationships[0].from_table, "orders");
    assert_eq!(relationships[0].to_table, "customers");

    assert!(database.views.iter().any(|v| v.name == "recent_orders"));
}

#[tokio::test]
async fn mysql_introspection_reads_tables_columns_indexes_foreign_keys_and_views() {
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
        start_mysql_like(&image, "shop", "reader", "hunter2").expect("mysql container starts");
    let config = make_config(
        Driver::Mysql,
        "127.0.0.1",
        container.port,
        &container.database,
        &container.user,
        &container.password,
    );

    let database = dbctx::database::inspect(&config)
        .await
        .expect("introspection succeeds");

    assert_eq!(database.metadata.database, "shop");
    assert_eq!(database.metadata.engine, dbctx::model::Engine::Mysql);
    assert_common_schema(&database);
}

#[tokio::test]
async fn mariadb_introspection_reads_tables_columns_indexes_foreign_keys_and_views() {
    if !docker_available() {
        eprintln!("skipping: no docker daemon");
        return;
    }
    let image = test_image("DBCTX_TEST_MARIADB_IMAGE", "mariadb:11");
    if !image_available(&image) {
        eprintln!("skipping: {image} image not present");
        return;
    }

    let container =
        start_mysql_like(&image, "shop", "reader", "hunter2").expect("mariadb container starts");
    let config = make_config(
        Driver::Mariadb,
        "127.0.0.1",
        container.port,
        &container.database,
        &container.user,
        &container.password,
    );

    let database = dbctx::database::inspect(&config)
        .await
        .expect("introspection succeeds");

    assert_eq!(database.metadata.database, "shop");
    assert_eq!(database.metadata.engine, dbctx::model::Engine::Mariadb);
    assert_common_schema(&database);
}

#[tokio::test]
async fn sqlserver_introspection_reads_tables_columns_indexes_foreign_keys_and_views() {
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
    let config = make_config(
        Driver::Sqlsrv,
        "127.0.0.1",
        container.port,
        "shop",
        &container.user,
        &container.password,
    );

    let database = dbctx::database::inspect(&config)
        .await
        .expect("introspection succeeds");

    assert_eq!(database.metadata.database, "shop");
    assert_eq!(database.metadata.engine, dbctx::model::Engine::Sqlserver);
    assert_common_schema(&database);
}
