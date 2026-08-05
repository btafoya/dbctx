//! End-to-end test of `dbctx mcp` over stdio against a local SQLite database:
//! no Docker required. Spawns the real binary, speaks JSON-RPC on its stdin
//! and stdout, and checks the responses rather than any internal API, since
//! that is what an MCP client actually sees.

use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Executor};

async fn create_sqlite_file(path: &std::path::Path) {
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .connect()
        .await
        .expect("sqlite file creates");
    conn.execute("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        .await
        .expect("statement runs");
    conn.execute("INSERT INTO customers (id, name) VALUES (1, 'Ada')")
        .await
        .expect("statement runs");
}

fn send(stdin: &mut ChildStdin, value: serde_json::Value) {
    writeln!(stdin, "{value}").expect("write to dbctx mcp stdin");
}

fn recv(reader: &mut BufReader<ChildStdout>) -> serde_json::Value {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read a line from dbctx mcp stdout");
    serde_json::from_str(&line).unwrap_or_else(|error| panic!("{line:?} is not JSON: {error}"))
}

#[tokio::test]
async fn mcp_server_serves_resources_and_tools_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.db");
    create_sqlite_file(&path).await;

    let mut child = Command::new(env!("CARGO_BIN_EXE_dbctx"))
        .args([
            "mcp",
            "--driver",
            "sqlite",
            "--database",
            path.to_str().unwrap(),
        ])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("dbctx mcp starts");

    let mut stdin = child.stdin.take().expect("stdin is piped");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout is piped"));

    send(
        &mut stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "dbctx-test", "version": "0.0.1"}
            }
        }),
    );
    let initialized = recv(&mut reader);
    assert_eq!(initialized["id"], serde_json::json!(1));
    assert!(initialized["result"]["capabilities"]["tools"].is_object());

    send(
        &mut stdin,
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );

    send(
        &mut stdin,
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "resources/list"}),
    );
    let resources = recv(&mut reader);
    let uris: Vec<String> = resources["result"]["resources"]
        .as_array()
        .expect("resources array")
        .iter()
        .map(|r| r["uri"].as_str().unwrap().to_string())
        .collect();
    assert!(uris.contains(&"dbctx://schema".to_string()), "{uris:?}");
    assert!(uris.contains(&"dbctx://graph".to_string()), "{uris:?}");
    assert!(
        uris.contains(&"dbctx://tables/main.customers".to_string()),
        "{uris:?}"
    );

    send(
        &mut stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "execute-statement",
                "arguments": {"sql": "SELECT * FROM customers"}
            }
        }),
    );
    let called = recv(&mut reader);
    assert_eq!(called["result"]["isError"], serde_json::json!(false));
    let text = called["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    let result: serde_json::Value = serde_json::from_str(text).expect("result is JSON");
    assert_eq!(result["rows"][0][1], serde_json::json!("Ada"));

    send(
        &mut stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "execute-statement",
                "arguments": {"sql": "DROP TABLE customers"}
            }
        }),
    );
    let rejected = recv(&mut reader);
    assert_eq!(rejected["result"]["isError"], serde_json::json!(true));

    send(
        &mut stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "prompts/get",
            "params": {"name": "summarize-schema"}
        }),
    );
    let prompt = recv(&mut reader);
    let summary = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("prompt text");
    assert!(summary.contains("customers"), "{summary}");

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
}
