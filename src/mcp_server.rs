//! rmcp-based MCP server exposing the cached canonical schema model.
//!
//! The schema is read once at startup and served entirely from memory, per
//! `REQUIREMENTS_v0.3.md` §3.5; `refresh-schema` is the only thing that
//! re-queries the database. `execute-statement` is the one exception that
//! always talks to the database directly, since a cached result set would be
//! a contradiction in terms.

use std::sync::Arc;
use std::time::Duration;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
    GetPromptRequestParams, GetPromptResponse, GetPromptResult, JsonObject, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    Prompt, PromptMessage, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    Resource, ResourceContents, Role as PromptRole, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::json;
use thiserror::Error;
use tokio::sync::RwLock;

use crate::config::ConnectionConfig;
use crate::model::Database;

/// Why the MCP server could not start or keep running.
#[derive(Debug, Error)]
pub enum McpServerError {
    /// The initial schema could not be read.
    #[error("could not read the schema: {0}")]
    InitialSchema(#[from] crate::Error),

    /// Reading the schema took longer than the configured introspection
    /// timeout.
    #[error("introspection timed out after {0} seconds")]
    IntrospectionTimeout(u64),

    /// The rmcp server failed to initialize.
    #[error("could not start the MCP server: {0}")]
    Initialize(String),

    /// The server loop ended with an error.
    #[error("MCP server exited with an error: {0}")]
    Runtime(String),

    /// The SSE/HTTP transport's listener could not be bound.
    #[error("could not listen on {address}: {source}")]
    Listen {
        /// The address that was attempted.
        address: String,
        /// Underlying IO error.
        source: std::io::Error,
    },
}

/// The four fixed-URI resources, independent of the schema's tables.
const SCHEMA_URI: &str = "dbctx://schema";
const METADATA_URI: &str = "dbctx://metadata";
const GRAPH_URI: &str = "dbctx://graph";
const RELATIONSHIPS_URI: &str = "dbctx://relationships";
const TABLES_PREFIX: &str = "dbctx://tables/";

/// Shared server state: the connection to re-query on `refresh-schema` and
/// `execute-statement`, and the schema cached from the last successful read.
#[derive(Clone)]
pub struct DbctxServer {
    config: Arc<ConnectionConfig>,
    execute_timeout: Duration,
    introspection_timeout: Duration,
    cache: Arc<RwLock<Database>>,
}

impl DbctxServer {
    /// Build a server, reading the schema once before returning so the first
    /// request never waits on introspection.
    pub async fn new(
        config: ConnectionConfig,
        introspection_timeout: Duration,
        execute_timeout: Duration,
    ) -> Result<Self, McpServerError> {
        let database = read_schema(&config, introspection_timeout).await?;
        Ok(Self {
            config: Arc::new(config),
            execute_timeout,
            introspection_timeout,
            cache: Arc::new(RwLock::new(database)),
        })
    }
}

async fn read_schema(
    config: &ConnectionConfig,
    timeout: Duration,
) -> Result<Database, McpServerError> {
    tokio::time::timeout(timeout, crate::database::inspect(config))
        .await
        .map_err(|_| McpServerError::IntrospectionTimeout(timeout.as_secs()))?
        .map_err(McpServerError::InitialSchema)
}

impl ServerHandler for DbctxServer {
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_resources()
            .enable_tools()
            .enable_prompts()
            .build();
        ServerInfo::new(capabilities).with_instructions(
            "Deterministic database schema context from dbctx. Resources serve the cached \
             schema; call the refresh-schema tool after a migration to re-read it.",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let database = self.cache.read().await;

        let mut resources = vec![
            resource(SCHEMA_URI, "schema", "application/json"),
            resource(METADATA_URI, "metadata", "application/json"),
            resource(GRAPH_URI, "graph", "text/vnd.mermaid"),
            resource(RELATIONSHIPS_URI, "relationships", "application/json"),
        ];
        for table in &database.tables {
            resources.push(resource(
                &format!("{TABLES_PREFIX}{}.{}", table.schema, table.name),
                &format!("{}.{}", table.schema, table.name),
                "application/json",
            ));
        }

        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(Vec::new()))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let database = self.cache.read().await;

        if request.uri == GRAPH_URI {
            return Ok(ReadResourceResult::new(vec![
                ResourceContents::text(crate::export::graph_mmd(&database), GRAPH_URI)
                    .with_mime_type("text/vnd.mermaid"),
            ])
            .into());
        }

        let text = if request.uri == SCHEMA_URI {
            crate::export::schema_json(&database)
        } else if request.uri == METADATA_URI {
            crate::export::metadata_json(&database)
        } else if request.uri == RELATIONSHIPS_URI {
            crate::export::relationships_json(&database)
        } else if let Some(qualified) = request.uri.strip_prefix(TABLES_PREFIX) {
            let table = qualified
                .split_once('.')
                .and_then(|(schema, name)| {
                    database
                        .tables
                        .iter()
                        .find(|t| t.schema == schema && t.name == name)
                })
                .ok_or_else(|| {
                    ErrorData::resource_not_found(
                        format!("no table resource for `{qualified}`"),
                        None,
                    )
                })?;
            crate::export::table_json(&database, table)
        } else {
            return Err(ErrorData::resource_not_found(
                format!("no resource for `{}`", request.uri),
                None,
            ));
        };

        let text = text.map_err(|error| {
            ErrorData::internal_error(format!("could not render resource: {error}"), None)
        })?;

        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, request.uri.clone()).with_mime_type("application/json"),
        ])
        .into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(ListPromptsResult::with_all_items(vec![
            Prompt::new(
                "summarize-schema",
                Some(
                    "A short summary of the database: engine, table and view counts, and key relationships.",
                ),
                None,
            ),
            Prompt::new(
                "describe-table",
                Some("A description of every table: columns, primary key and comment."),
                None,
            ),
            Prompt::new(
                "explain-relationships",
                Some("A narrative of every foreign key relationship in the schema."),
                None,
            ),
        ]))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        let database = self.cache.read().await;

        let text = match request.name.as_str() {
            "summarize-schema" => summarize_schema(&database),
            "describe-table" => describe_tables(&database),
            "explain-relationships" => explain_relationships(&database),
            other => {
                return Err(ErrorData::invalid_params(
                    format!("unknown prompt `{other}`"),
                    None,
                ));
            }
        };

        Ok(GetPromptResult::new(vec![PromptMessage::new_text(PromptRole::User, text)]).into())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![
            Tool::new(
                "execute-statement",
                "Execute a single read-only SQL statement and return the result as JSON. \
                 Mutating and multi-statement queries are rejected before reaching the \
                 database.",
                json_object(json!({
                    "type": "object",
                    "properties": {
                        "sql": {"type": "string", "description": "The SQL statement to run."},
                        "timeout": {
                            "type": "number",
                            "description": "Seconds before the statement is cancelled."
                        }
                    },
                    "required": ["sql"]
                })),
            ),
            Tool::new(
                "refresh-schema",
                "Re-read the database schema and replace the cached copy every resource \
                 is served from.",
                json_object(json!({"type": "object", "properties": {}})),
            ),
        ]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        match request.name.as_ref() {
            "execute-statement" => self.call_execute_statement(request).await,
            "refresh-schema" => self.call_refresh_schema().await,
            other => Err(ErrorData::invalid_params(
                format!("unknown tool `{other}`"),
                None,
            )),
        }
    }
}

impl DbctxServer {
    async fn call_execute_statement(
        &self,
        request: CallToolRequestParams,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = request.arguments.unwrap_or_default();
        let sql = arguments
            .get("sql")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ErrorData::invalid_params("`sql` is required", None))?;
        let timeout = arguments
            .get("timeout")
            .and_then(|value| value.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(self.execute_timeout);

        match crate::execution::execute(&self.config, sql, timeout).await {
            Ok(result) => {
                let json = serde_json::to_string(&result).map_err(|error| {
                    ErrorData::internal_error(format!("could not serialize result: {error}"), None)
                })?;
                Ok(CallToolResult::success(vec![ContentBlock::text(json)]).into())
            }
            Err(error) => {
                Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into())
            }
        }
    }

    async fn call_refresh_schema(&self) -> Result<CallToolResponse, ErrorData> {
        match read_schema(&self.config, self.introspection_timeout).await {
            Ok(database) => {
                let summary = format!(
                    "schema refreshed: {} tables, {} views",
                    database.tables.len(),
                    database.views.len()
                );
                *self.cache.write().await = database;
                Ok(CallToolResult::success(vec![ContentBlock::text(summary)]).into())
            }
            Err(error) => {
                Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into())
            }
        }
    }
}

fn resource(uri: &str, name: &str, mime_type: &str) -> Resource {
    Resource::new(uri, name).with_mime_type(mime_type)
}

fn json_object(value: serde_json::Value) -> JsonObject {
    value.as_object().cloned().unwrap_or_default()
}

fn summarize_schema(database: &Database) -> String {
    let relationships = database.relationships();
    let mut text = format!(
        "Database `{}` runs {:?} {}: {} tables, {} views, {} relationships.\n\nTables:\n",
        database.metadata.database,
        database.metadata.engine,
        database.metadata.engine_version,
        database.tables.len(),
        database.views.len(),
        relationships.len(),
    );
    for table in &database.tables {
        text.push_str(&format!(
            "- {}.{} ({} columns)\n",
            table.schema,
            table.name,
            table.columns.len()
        ));
    }
    text
}

fn describe_tables(database: &Database) -> String {
    let mut text = String::new();
    for table in &database.tables {
        let primary_key: Vec<&str> = table
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.as_str())
            .collect();
        text.push_str(&format!("## {}.{}\n", table.schema, table.name));
        if let Some(comment) = &table.comment {
            text.push_str(&format!("{comment}\n"));
        }
        text.push_str(&format!(
            "- {} columns, primary key: {}\n\n",
            table.columns.len(),
            if primary_key.is_empty() {
                "none".to_string()
            } else {
                primary_key.join(", ")
            }
        ));
    }
    if text.is_empty() {
        "The schema has no tables.".to_string()
    } else {
        text
    }
}

fn explain_relationships(database: &Database) -> String {
    let relationships = database.relationships();
    if relationships.is_empty() {
        return "The schema has no foreign key relationships.".to_string();
    }

    let mut text = String::new();
    for relationship in relationships {
        text.push_str(&format!(
            "{}.{}({}) references {}.{}({}) via `{}`.\n",
            relationship.from_schema,
            relationship.from_table,
            relationship.from_columns.join(", "),
            relationship.to_schema,
            relationship.to_table,
            relationship.to_columns.join(", "),
            relationship.constraint,
        ));
    }
    text
}
