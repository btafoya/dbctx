//! Local SQLite introspection, execution and validation tests.
//!
//! SQLite is a file rather than a server, so these run against temp files
//! and need no Docker daemon or external `sqlite3` binary: the schema is
//! created through the same `sqlx` pool dbctx itself uses.

use std::time::Duration;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Executor};

use dbctx::config::{ConnectionConfig, ConnectionSource, Driver};

async fn create_sqlite_file(path: &std::path::Path, statements: &[&str]) {
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .connect()
        .await
        .expect("sqlite file creates");
    for statement in statements {
        conn.execute(*statement).await.expect("statement runs");
    }
}

fn config(databases: Vec<String>) -> ConnectionConfig {
    let source = ConnectionSource {
        driver: Some(Driver::Sqlite),
        database: databases,
        ..ConnectionSource::default()
    };
    ConnectionConfig::resolve(&[source]).expect("test config resolves")
}

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().to_string()
}

#[tokio::test]
async fn sqlite_introspection_reads_tables_columns_indexes_and_foreign_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.db");
    create_sqlite_file(
        &path,
        &[
            "CREATE TABLE customers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT NOT NULL UNIQUE,
                name TEXT
            )",
            "CREATE TABLE orders (
                id INTEGER PRIMARY KEY,
                customer_id INTEGER NOT NULL REFERENCES customers(id),
                total REAL
            )",
            "CREATE INDEX idx_orders_total ON orders(total)",
            "CREATE VIEW recent_orders AS SELECT id, customer_id, total FROM orders",
        ],
    )
    .await;

    let config = config(vec![path_string(&path)]);
    let database = dbctx::database::inspect(&config)
        .await
        .expect("introspection succeeds");

    assert_eq!(database.metadata.engine, dbctx::model::Engine::Sqlite);
    assert_eq!(database.tables.len(), 2);
    assert_eq!(database.views.len(), 1);

    let customers = database
        .tables
        .iter()
        .find(|t| t.name == "customers")
        .expect("customers table");
    assert_eq!(customers.schema, "main");
    assert!(
        customers
            .columns
            .iter()
            .any(|c| c.name == "id" && c.primary_key && c.auto_increment)
    );
    assert!(
        customers
            .columns
            .iter()
            .any(|c| c.name == "email" && c.unique)
    );

    let orders = database
        .tables
        .iter()
        .find(|t| t.name == "orders")
        .expect("orders table");
    assert_eq!(orders.foreign_keys.len(), 1);
    assert_eq!(orders.foreign_keys[0].referenced_table, "customers");
    assert!(orders.indexes.iter().any(|i| i.name == "idx_orders_total"));

    assert!(database.views.iter().any(|v| v.name == "recent_orders"));
}

#[tokio::test]
async fn sqlite_introspection_attaches_additional_databases_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.db");
    let archive_path = dir.path().join("archive.db");
    create_sqlite_file(
        &main_path,
        &["CREATE TABLE customers (id INTEGER PRIMARY KEY)"],
    )
    .await;
    create_sqlite_file(
        &archive_path,
        &["CREATE TABLE old_orders (id INTEGER PRIMARY KEY)"],
    )
    .await;

    let config = config(vec![path_string(&main_path), path_string(&archive_path)]);
    let database = dbctx::database::inspect(&config)
        .await
        .expect("introspection succeeds");

    assert!(
        database
            .tables
            .iter()
            .any(|t| t.schema == "main" && t.name == "customers")
    );
    assert!(
        database
            .tables
            .iter()
            .any(|t| t.schema == "attach1" && t.name == "old_orders")
    );
}

#[tokio::test]
async fn sqlite_without_rowid_and_strict_are_flagged_and_validated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.db");
    create_sqlite_file(
        &path,
        &[
            "CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT) WITHOUT ROWID",
            "CREATE TABLE strict_items (id INTEGER PRIMARY KEY, name TEXT NOT NULL) STRICT",
        ],
    )
    .await;

    let config = config(vec![path_string(&path)]);
    let database = dbctx::database::inspect(&config)
        .await
        .expect("introspection succeeds");

    let kv = database
        .tables
        .iter()
        .find(|t| t.name == "kv")
        .expect("kv table");
    assert_eq!(
        kv.attributes.get("without_rowid"),
        Some(&serde_json::json!(true))
    );

    let strict_items = database
        .tables
        .iter()
        .find(|t| t.name == "strict_items")
        .expect("strict_items table");
    assert_eq!(
        strict_items.attributes.get("strict"),
        Some(&serde_json::json!(true))
    );

    let report = dbctx::validation::validate(&database);
    assert!(report.findings.iter().any(|f| {
        f.rule == dbctx::validation::Rule::SqliteStrictMissingDefaultOnNotNull
            && f.table == "strict_items"
            && f.columns == ["name"]
    }));
}

#[tokio::test]
async fn sqlite_execution_runs_selects_and_rejects_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.db");
    create_sqlite_file(
        &path,
        &[
            "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            "INSERT INTO widgets (id, name) VALUES (1, 'alpha'), (2, 'beta')",
        ],
    )
    .await;

    let config = config(vec![path_string(&path)]);

    let result = dbctx::execution::execute(
        &config,
        "SELECT id, name FROM widgets ORDER BY id",
        Duration::from_secs(30),
    )
    .await
    .expect("select succeeds");
    assert_eq!(result.row_count, 2);
    assert_eq!(result.rows[0][1], serde_json::json!("alpha"));
    assert_eq!(result.rows[1][1], serde_json::json!("beta"));

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
