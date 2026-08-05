//! CLI entry point for `dbctx mcp`: resolve the connection the same way
//! every other command does, then hand off to [`crate::mcp_server`] for the
//! protocol implementation.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::cli::{ConnectionArgs, McpArgs};
use crate::config::{ConnectionConfig, ConnectionSource, ProjectConfig};
use crate::discovery;
use crate::mcp_server::{DbctxServer, McpServerError};

/// The project configuration file, mirroring every other command.
const CONFIG_FILE: &str = ".dbctx.toml";

/// Default seconds `execute-statement` is allowed to run before the tool
/// call reports a timeout, matching the CLI's own default.
const DEFAULT_EXECUTE_TIMEOUT_SECS: u64 = 30;

/// Resolve the connection, build the server, and serve it: over stdio by
/// default, or over HTTP when `--sse-port` names a port.
///
/// Builds its own tokio runtime, like every other command, so the binary
/// never has to know the server runs indefinitely rather than once.
pub fn run(args: &McpArgs) -> Result<(), McpServerError> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|source| McpServerError::Initialize(format!("could not start tokio: {source}")))?;

    runtime.block_on(run_async(args))
}

async fn run_async(args: &McpArgs) -> Result<(), McpServerError> {
    let config = connect(&args.connection)?;

    let server = DbctxServer::new(
        config,
        Duration::from_secs(args.introspection_timeout),
        Duration::from_secs(DEFAULT_EXECUTE_TIMEOUT_SECS),
    )
    .await?;

    match args.sse_port {
        Some(port) => serve_sse(server, port).await,
        None => serve_stdio(server).await,
    }
}

/// The same connection resolution `main.rs` uses for every other command,
/// reimplemented here because it is private to the binary crate.
fn connect(args: &ConnectionArgs) -> Result<ConnectionConfig, McpServerError> {
    let (env_file, required) = match &args.env {
        Some(path) => (path.clone(), true),
        None => (PathBuf::from(".env"), false),
    };

    let options = discovery::Options {
        cli: args.source(),
        project: ProjectConfig::load(Path::new(CONFIG_FILE))
            .map_err(|error| McpServerError::InitialSchema(error.into()))?
            .connection(),
        dotenv: ConnectionSource::from_dotenv(&env_file, required)
            .map_err(|error| McpServerError::InitialSchema(error.into()))?,
        environment: ConnectionSource::from_env()
            .map_err(|error| McpServerError::InitialSchema(error.into()))?,
        compose_service: args.compose_service.clone(),
        docker_container: args.docker_container.clone(),
        interactive: std::io::stdin().is_terminal(),
    };

    discovery::resolve(&options).map_err(|error| McpServerError::InitialSchema(error.into()))
}

async fn serve_stdio(server: DbctxServer) -> Result<(), McpServerError> {
    let transport = rmcp::transport::io::stdio();
    let running = rmcp::serve_server(server, transport)
        .await
        .map_err(|error| McpServerError::Initialize(error.to_string()))?;
    running
        .waiting()
        .await
        .map_err(|error| McpServerError::Runtime(error.to_string()))?;
    Ok(())
}

async fn serve_sse(server: DbctxServer, port: u16) -> Result<(), McpServerError> {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::tower::{
        StreamableHttpServerConfig, StreamableHttpService,
    };

    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let address = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|source| McpServerError::Listen { address, source })?;

    axum::serve(listener, router)
        .await
        .map_err(|error| McpServerError::Runtime(error.to_string()))
}
