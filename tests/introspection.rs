//! End-to-end introspection against real Docker databases.
//!
//! Each test starts a disposable container, creates a small schema, calls
//! `dbctx::database::inspect`, and asserts on the returned canonical model.
//! Tests skip gracefully when docker or the required image is not available.

mod common;

use std::process::Command;

use common::*;

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
    // MariaDB 11+ images ship `mariadb` rather than `mysql`.
    let client = if container.contains("mariadb-11-") || container.contains("mariadb-12-") {
        "mariadb"
    } else {
        "mysql"
    };
    let output = run(Command::new("docker")
        .args([
            "exec",
            "-i",
            container,
            client,
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

fn create_postgres_schema(container: &Container) {
    let sql = r#"
        CREATE TABLE customers (
            id SERIAL PRIMARY KEY,
            email VARCHAR(255) NOT NULL UNIQUE,
            name VARCHAR(255)
        );
        CREATE TABLE orders (
            id SERIAL PRIMARY KEY,
            customer_id INTEGER NOT NULL REFERENCES customers(id),
            total NUMERIC(10,2)
        );
        CREATE INDEX idx_orders_total ON orders(total);
        CREATE VIEW recent_orders AS SELECT id, customer_id, total FROM orders;
    "#;
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
        "could not create postgres schema: {}",
        exec_stderr(&output)
    );
}

fn assert_common_schema(database: &dbctx::model::Database, primary_key_index_name: &str) {
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

    assert!(
        customers
            .indexes
            .iter()
            .any(|i| i.name == primary_key_index_name)
    );
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
    create_mysql_schema(
        &container.name,
        &container.database,
        &container.user,
        &container.password,
    );
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
    assert_common_schema(&database, "PRIMARY");
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
    create_mysql_schema(
        &container.name,
        &container.database,
        &container.user,
        &container.password,
    );
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
    assert_common_schema(&database, "PRIMARY");
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
    create_sqlserver_schema(&container.name, &container.password);
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
    assert_common_schema(&database, "PRIMARY");
}

#[tokio::test]
async fn postgres_introspection_reads_tables_columns_indexes_foreign_keys_and_views() {
    if !docker_available() {
        eprintln!("skipping: no docker daemon");
        return;
    }
    let image = test_image("DBCTX_TEST_POSTGRES_IMAGE", "postgres:17");
    if !image_available(&image) {
        eprintln!("skipping: {image} image not present");
        return;
    }

    let container =
        start_postgres(&image, "shop", "reader", "hunter2").expect("postgres container starts");
    create_postgres_schema(&container);
    let config = make_config(
        Driver::Postgres,
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
    assert_eq!(database.metadata.engine, dbctx::model::Engine::Postgres);
    assert_common_schema(&database, "customers_pkey");

    let customers = database
        .tables
        .iter()
        .find(|t| t.name == "customers")
        .expect("customers table");
    assert_eq!(customers.schema, "public");
    assert_eq!(
        customers.attributes.get("row_security"),
        Some(&serde_json::json!(false))
    );
}
