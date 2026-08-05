//! Export of the canonical schema model.
//!
//! Every artifact is derived from the model in `src/model.rs`; this module
//! never queries a database. `FORMAT.md` defines the file layout and the
//! shape of each document.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::Result;
use crate::model::{
    Column, Database, DatabaseMetadata, DocumentHeader, Engine, Relationship, Table, View,
};

/// Why an export could not complete.
#[derive(Debug, Error)]
pub enum ExportError {
    /// A value could not be serialized to JSON.
    #[error("could not serialize JSON: {0}")]
    Serialization(#[from] serde_json::Error),

    /// A file could not be written.
    #[error("could not write `{path}`: {source}")]
    Io {
        /// File the export was writing.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The destination already exists and `--overwrite` was not given.
    #[error("`{path}` already exists; use --overwrite to replace it")]
    OutputExists {
        /// File or directory that would be overwritten.
        path: PathBuf,
    },
}

impl ExportError {
    /// Build an [`ExportError::Io`] for `path`.
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// Build an [`ExportError::OutputExists`] for `path`.
    fn exists(path: impl AsRef<Path>) -> Self {
        Self::OutputExists {
            path: path.as_ref().to_path_buf(),
        }
    }
}

/// What an export writes and where.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Directory to write artifacts into.
    pub output: PathBuf,
    /// Write `schema.json` to stdout instead of the directory.
    pub stdout: bool,
    /// Replace existing files.
    pub overwrite: bool,
    /// Skip the JSON documents.
    pub no_json: bool,
    /// Skip the Markdown document.
    pub no_markdown: bool,
    /// Skip per-table files.
    pub no_tables: bool,
    /// Skip the Mermaid ER diagram.
    pub no_mermaid: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            output: PathBuf::from(".ai/dbctx"),
            stdout: false,
            overwrite: false,
            no_json: false,
            no_markdown: false,
            no_tables: false,
            no_mermaid: false,
        }
    }
}

/// Write every enabled artifact for `database`.
pub fn export(database: &Database, options: &ExportOptions) -> Result<(), ExportError> {
    if options.no_json && options.no_markdown && options.no_tables && options.no_mermaid {
        return Ok(());
    }

    if options.stdout {
        if !options.no_json {
            return write_stdout(&serialize(database)?);
        }
        if !options.no_markdown {
            return write_stdout(&render_markdown(database));
        }
        if !options.no_mermaid {
            return write_stdout(&render_mermaid(database));
        }
        return Ok(());
    }

    fs::create_dir_all(&options.output).map_err(|e| ExportError::io(&options.output, e))?;

    if !options.overwrite && any_output_exists(options)? {
        return Err(ExportError::exists(&options.output));
    }

    if !options.no_json {
        let schema_path = options.output.join("schema.json");
        write_file(&schema_path, &serialize(database)?)?;

        let relationships_path = options.output.join("relationships.json");
        write_file(
            &relationships_path,
            &serialize(&RelationshipsDocument::from(database))?,
        )?;

        let metadata_path = options.output.join("metadata.json");
        write_file(
            &metadata_path,
            &serialize(&MetadataDocument::from(database))?,
        )?;

        if !options.no_tables {
            let tables_dir = options.output.join("tables");
            fs::create_dir_all(&tables_dir).map_err(|e| ExportError::io(&tables_dir, e))?;
            for table in &database.tables {
                let table_path =
                    tables_dir.join(format!("{}.json", table_file_name(database, table)));
                write_file(
                    &table_path,
                    &serialize(&TableDocument::from(database, table))?,
                )?;
            }
        }
    }

    if !options.no_markdown {
        let markdown_path = options.output.join("schema.md");
        write_file(&markdown_path, &render_markdown(database))?;
    }

    if !options.no_mermaid {
        let mermaid_path = options.output.join("graph.mmd");
        write_file(&mermaid_path, &render_mermaid(database))?;
    }

    Ok(())
}

fn any_output_exists(options: &ExportOptions) -> Result<bool, ExportError> {
    let paths = [
        options.output.join("schema.json"),
        options.output.join("schema.md"),
        options.output.join("graph.mmd"),
        options.output.join("relationships.json"),
        options.output.join("metadata.json"),
        options.output.join("tables"),
    ];
    Ok(paths.iter().any(|p| p.exists()))
}

fn serialize<T: Serialize>(value: &T) -> Result<String, ExportError> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value
        .serialize(&mut serializer)
        .map_err(ExportError::from)?;
    buf.push(b'\n');
    Ok(String::from_utf8(buf).expect("serde_json serializes valid UTF-8"))
}

fn write_file(path: &Path, contents: &str) -> Result<(), ExportError> {
    fs::write(path, contents).map_err(|e| ExportError::io(path, e))
}

fn write_stdout(contents: &str) -> Result<(), ExportError> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(contents.as_bytes())
        .map_err(|e| ExportError::io("stdout", e))
}

fn table_file_name(database: &Database, table: &Table) -> String {
    if database.metadata.engine == Engine::Sqlserver {
        format!("{}.{}", table.schema, table.name)
    } else {
        table.name.clone()
    }
}

/// Render the canonical schema model as a compact Markdown document.
///
/// Tables and views are sorted by schema then name, columns keep their
/// catalog ordinal positions, and foreign keys keep their declared column
/// order, so the output is deterministic.
fn render_markdown(database: &Database) -> String {
    let mut md = String::new();

    md.push_str("# ");
    md.push_str(&database.metadata.database);
    md.push('\n');

    md.push_str("\n## Summary\n\n");
    md.push_str("- Engine: ");
    md.push_str(&format!("{:?}", database.metadata.engine).to_lowercase());
    md.push(' ');
    md.push_str(&database.metadata.engine_version);
    md.push('\n');
    md.push_str("- Tables: ");
    md.push_str(&database.tables.len().to_string());
    md.push('\n');
    md.push_str("- Views: ");
    md.push_str(&database.views.len().to_string());
    md.push('\n');
    md.push_str("- Foreign keys: ");
    md.push_str(&database.relationships().len().to_string());
    md.push('\n');

    if !database.tables.is_empty() {
        md.push_str("\n## Tables\n");
        for table in &database.tables {
            render_table_markdown(database, table, &mut md);
        }
    }

    if !database.views.is_empty() {
        md.push_str("\n## Views\n");
        for view in &database.views {
            render_view_markdown(database, view, &mut md);
        }
    }

    md.push('\n');
    md
}

fn render_table_markdown(database: &Database, table: &Table, md: &mut String) {
    md.push_str("\n### ");
    md.push_str(&table_display_name(database, table));
    md.push('\n');

    if let Some(comment) = &table.comment {
        md.push('\n');
        md.push_str(&escape_markdown_paragraph(comment));
        md.push('\n');
    }

    if table.columns.is_empty() {
        md.push_str("\nNo columns.\n");
    } else {
        md.push_str("\n#### Columns\n\n");
        md.push_str("| Name | Type | Nullable | Default | Attributes | Comment |\n");
        md.push_str("|---|---|---|---|---|---|\n");
        for column in &table.columns {
            let attributes = column_attributes(column);
            md.push_str("| ");
            md.push_str(&escape_markdown_cell(&column.name));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&column.full_type));
            md.push_str(" | ");
            md.push_str(if column.nullable { "YES" } else { "NO" });
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(
                column.default.as_deref().unwrap_or(""),
            ));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&attributes));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(
                column.comment.as_deref().unwrap_or(""),
            ));
            md.push_str(" |\n");
        }
    }

    if table.indexes.is_empty() {
        md.push_str("\nNo indexes.\n");
    } else {
        md.push_str("\n#### Indexes\n\n");
        md.push_str("| Name | Type | Unique | Columns |\n");
        md.push_str("|---|---|---|---|\n");
        for index in &table.indexes {
            md.push_str("| ");
            md.push_str(&escape_markdown_cell(&index.name));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&index.index_type));
            md.push_str(" | ");
            md.push_str(if index.unique { "Yes" } else { "No" });
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&index.columns.join(", ")));
            md.push_str(" |\n");
        }
    }

    if table.foreign_keys.is_empty() {
        md.push_str("\nNo foreign keys.\n");
    } else {
        md.push_str("\n#### Foreign Keys\n\n");
        md.push_str("| Name | Columns | Referenced | On Update | On Delete |\n");
        md.push_str("|---|---|---|---|---|\n");
        for fk in &table.foreign_keys {
            let referenced = if database.metadata.engine == Engine::Sqlserver {
                format!(
                    "{}.{}({})",
                    fk.referenced_schema,
                    fk.referenced_table,
                    fk.referenced_columns.join(", ")
                )
            } else {
                format!(
                    "{}({})",
                    fk.referenced_table,
                    fk.referenced_columns.join(", ")
                )
            };
            md.push_str("| ");
            md.push_str(&escape_markdown_cell(&fk.name));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&fk.columns.join(", ")));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&referenced));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&fk.on_update));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&fk.on_delete));
            md.push_str(" |\n");
        }
    }

    if let Some(analysis) = &table.analysis {
        md.push_str("\n#### Analysis\n\n");
        for finding in &analysis.findings {
            md.push_str("- **");
            md.push_str(&escape_markdown_cell(finding.kind.label()));
            md.push_str("** (confidence: ");
            md.push_str(&finding.confidence.to_string());
            md.push_str(")\n");
            for evidence in &finding.evidence {
                md.push_str("  - ");
                md.push_str(&escape_markdown_cell(evidence));
                md.push('\n');
            }
        }
    }
}

fn render_view_markdown(database: &Database, view: &View, md: &mut String) {
    md.push_str("\n### ");
    md.push_str(&view_display_name(database, view));
    md.push('\n');

    if view.columns.is_empty() {
        md.push_str("\nNo columns.\n");
    } else {
        md.push_str("\n#### Columns\n\n");
        md.push_str("| Name | Type | Nullable | Default | Attributes | Comment |\n");
        md.push_str("|---|---|---|---|---|---|\n");
        for column in &view.columns {
            let attributes = column_attributes(column);
            md.push_str("| ");
            md.push_str(&escape_markdown_cell(&column.name));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&column.full_type));
            md.push_str(" | ");
            md.push_str(if column.nullable { "YES" } else { "NO" });
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(
                column.default.as_deref().unwrap_or(""),
            ));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(&attributes));
            md.push_str(" | ");
            md.push_str(&escape_markdown_cell(
                column.comment.as_deref().unwrap_or(""),
            ));
            md.push_str(" |\n");
        }
    }
}

fn table_display_name(database: &Database, table: &Table) -> String {
    if database.metadata.engine == Engine::Sqlserver {
        format!("{}.{}", table.schema, table.name)
    } else {
        table.name.clone()
    }
}

fn view_display_name(database: &Database, view: &View) -> String {
    if database.metadata.engine == Engine::Sqlserver {
        format!("{}.{}", view.schema, view.name)
    } else {
        view.name.clone()
    }
}

fn column_attributes(column: &Column) -> String {
    let mut attrs = Vec::new();
    if column.primary_key {
        attrs.push("PK");
    }
    if column.auto_increment {
        attrs.push("AI");
    }
    if column.unique {
        attrs.push("Unique");
    }
    if column.generated {
        attrs.push("Generated");
    }
    attrs.join(", ")
}

fn escape_markdown_paragraph(text: &str) -> String {
    text.replace('\r', "")
        .replace('\n', " ")
        .replace('|', "\\|")
}

fn escape_markdown_cell(text: &str) -> String {
    text.replace('\r', "")
        .replace('\n', " ")
        .replace('|', "\\|")
}

/// Render the canonical schema model as a deterministic Mermaid ER diagram.
///
/// Tables become entities, columns become attributes with PK/FK labels, and
/// foreign keys become relationships from the referenced table to the
/// referencing table. Identifiers are double-quoted so schema-qualified SQL
/// Server names and unusual identifiers stay valid Mermaid syntax.
pub fn render_mermaid(database: &Database) -> String {
    let mut mmd = String::new();

    mmd.push_str("erDiagram\n");

    for table in &database.tables {
        let entity = mermaid_quote(&table_display_name(database, table));
        mmd.push_str("    ");
        mmd.push_str(&entity);
        mmd.push_str(" {\n");

        let fk_columns: HashSet<&str> = table
            .foreign_keys
            .iter()
            .flat_map(|fk| fk.columns.iter().map(String::as_str))
            .collect();

        for column in &table.columns {
            mmd.push_str("        ");
            mmd.push_str(&column.data_type);
            mmd.push(' ');
            mmd.push_str(&mermaid_quote(&column.name));

            let mut labels = Vec::new();
            if column.primary_key {
                labels.push("PK");
            }
            if fk_columns.contains(column.name.as_str()) {
                labels.push("FK");
            }
            match labels.as_slice() {
                [label] => {
                    mmd.push(' ');
                    mmd.push_str(label);
                }
                _ if !labels.is_empty() => {
                    mmd.push_str(" \"");
                    mmd.push_str(&labels.join(","));
                    mmd.push('"');
                }
                _ => {}
            }
            mmd.push('\n');
        }

        mmd.push_str("    }\n");
    }

    for relationship in database.relationships() {
        mmd.push_str("    ");
        mmd.push_str(&mermaid_quote(&relationship_display_name(
            database,
            &relationship.to_schema,
            &relationship.to_table,
        )));
        mmd.push_str(" ||--o{ ");
        mmd.push_str(&mermaid_quote(&relationship_display_name(
            database,
            &relationship.from_schema,
            &relationship.from_table,
        )));
        mmd.push_str(" : ");
        mmd.push_str(&mermaid_quote(&relationship.constraint));
        mmd.push('\n');
    }

    mmd.push('\n');
    mmd
}

/// Return the table display name used inside a relationship line.
///
/// Relationships already carry explicit schema names, so the display name
/// follows the same schema-qualification rule as table output.
fn relationship_display_name(database: &Database, schema: &str, table: &str) -> String {
    if database.metadata.engine == Engine::Sqlserver {
        format!("{schema}.{table}")
    } else {
        table.to_string()
    }
}

/// Quote a Mermaid identifier, escaping internal double quotes.
fn mermaid_quote(text: &str) -> String {
    if text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        text.to_string()
    } else {
        format!("\"{}\"", text.replace('"', "\\\""))
    }
}

/// The standalone relationships document.
#[derive(Serialize)]
struct RelationshipsDocument {
    #[serde(flatten)]
    header: DocumentHeader,
    relationships: Vec<Relationship>,
}

impl From<&Database> for RelationshipsDocument {
    fn from(database: &Database) -> Self {
        Self {
            header: DocumentHeader::new(
                "dbctx.relationships",
                database.header.generated_at.clone(),
            ),
            relationships: database.relationships(),
        }
    }
}

/// The standalone metadata document.
#[derive(Serialize)]
struct MetadataDocument {
    #[serde(flatten)]
    header: DocumentHeader,
    database: String,
    engine: crate::model::Engine,
    engine_version: String,
    table_count: usize,
    view_count: usize,
    foreign_key_count: usize,
}

impl From<&Database> for MetadataDocument {
    fn from(database: &Database) -> Self {
        let foreign_key_count: usize = database.tables.iter().map(|t| t.foreign_keys.len()).sum();
        Self {
            header: DocumentHeader::new("dbctx.metadata", database.header.generated_at.clone()),
            database: database.metadata.database.clone(),
            engine: database.metadata.engine,
            engine_version: database.metadata.engine_version.clone(),
            table_count: database.tables.len(),
            view_count: database.views.len(),
            foreign_key_count,
        }
    }
}

/// A single table exported on its own, prefixed with the document header.
#[derive(Serialize)]
struct TableDocument<'a> {
    #[serde(flatten)]
    header: DocumentHeader,
    metadata: &'a DatabaseMetadata,
    table: &'a Table,
}

impl<'a> TableDocument<'a> {
    fn from(database: &'a Database, table: &'a Table) -> Self {
        Self {
            header: DocumentHeader::new("dbctx.table", database.header.generated_at.clone()),
            metadata: &database.metadata,
            table,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AnalysisFinding, AnalysisKind, Column, DatabaseMetadata, DocumentHeader, Engine,
        ForeignKey, Index, Table, TableAnalysis, View,
    };
    use serde_json::Value;

    fn column(name: &str, ordinal: u32) -> Column {
        Column {
            name: name.to_string(),
            ordinal_position: ordinal,
            data_type: "int".to_string(),
            full_type: "int(11)".to_string(),
            nullable: false,
            default: None,
            auto_increment: false,
            primary_key: false,
            unique: false,
            comment: None,
            generated: false,
            expression: None,
        }
    }

    fn database() -> Database {
        let mut orders = Table {
            schema: "dbo".to_string(),
            name: "orders".to_string(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            comment: None,
            columns: vec![column("id", 1), column("customer_id", 2)],
            indexes: vec![Index {
                name: "PRIMARY".to_string(),
                unique: true,
                columns: vec!["id".to_string()],
                index_type: "BTREE".to_string(),
            }],
            foreign_keys: vec![ForeignKey {
                name: "fk_orders_customer".to_string(),
                columns: vec!["customer_id".to_string()],
                referenced_schema: "dbo".to_string(),
                referenced_table: "customers".to_string(),
                referenced_columns: vec!["id".to_string()],
                on_update: "NO ACTION".to_string(),
                on_delete: "CASCADE".to_string(),
            }],
            analysis: None,
        };
        orders.columns[0].primary_key = true;
        orders.columns[0].auto_increment = true;
        orders.columns[0].unique = true;

        Database {
            header: DocumentHeader::new(Database::FORMAT, "2026-01-01T00:00:00Z"),
            metadata: DatabaseMetadata {
                database: "shop".to_string(),
                engine: Engine::Mysql,
                engine_version: "8.4.0".to_string(),
                default_charset: Some("utf8mb4".to_string()),
                default_collation: Some("utf8mb4_0900_ai_ci".to_string()),
            },
            tables: vec![orders],
            views: vec![View {
                schema: "dbo".to_string(),
                name: "recent_orders".to_string(),
                columns: vec![column("id", 1)],
            }],
        }
    }

    #[test]
    fn export_writes_schema_relationships_metadata_and_table_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(dir.path().join("schema.json").exists());
        assert!(dir.path().join("schema.md").exists());
        assert!(dir.path().join("graph.mmd").exists());
        assert!(dir.path().join("relationships.json").exists());
        assert!(dir.path().join("metadata.json").exists());
        assert!(dir.path().join("tables").exists());
        assert!(dir.path().join("tables").join("orders.json").exists());
    }

    #[test]
    fn export_refuses_to_overwrite_existing_output_without_flag() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("schema.json"), "{}").unwrap();
        let db = database();

        let error = export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(matches!(error, ExportError::OutputExists { .. }));
    }

    #[test]
    fn export_overwrites_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("schema.json"), "{}").unwrap();
        let db = database();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                overwrite: true,
                ..Default::default()
            },
        )
        .unwrap();

        let contents = fs::read_to_string(dir.path().join("schema.json")).unwrap();
        assert!(contents.contains("dbctx.schema"));
    }

    #[test]
    fn sqlserver_table_files_include_schema_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.metadata.engine = Engine::Sqlserver;
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(dir.path().join("tables").join("dbo.orders.json").exists());
    }

    #[test]
    fn exported_json_uses_two_space_indent_and_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap();

        let contents = fs::read_to_string(dir.path().join("schema.json")).unwrap();
        assert!(
            contents.contains("\n  \"format\""),
            "expected 2-space indent"
        );
        assert!(
            !contents.contains("\n    \"format\""),
            "expected no 4-space indent"
        );
        assert!(contents.ends_with('\n'), "expected trailing newline");
    }

    #[test]
    fn schema_json_snapshot_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap();

        let schema_json: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("schema.json")).unwrap())
                .unwrap();

        insta::assert_json_snapshot!(
            schema_json,
            {
                ".generated_at" => "[GENERATED_AT]",
                ".generator.version" => "[VERSION]",
            }
        );
    }

    #[test]
    fn export_writes_schema_md() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap();

        let path = dir.path().join("schema.md");
        assert!(path.exists());
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains("# shop"));
        assert!(contents.contains("## Tables"));
        assert!(contents.contains("### orders"));
        assert!(contents.contains("#### Columns"));
        assert!(contents.contains("#### Indexes"));
        assert!(contents.contains("#### Foreign Keys"));
        assert!(contents.contains("## Views"));
    }

    #[test]
    fn schema_json_includes_analysis_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.tables[0].analysis = Some(TableAnalysis {
            findings: vec![AnalysisFinding {
                kind: AnalysisKind::LookupTable,
                confidence: 1.0,
                evidence: vec!["has a primary key".to_string()],
            }],
        });
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap();

        let schema_json: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("schema.json")).unwrap())
                .unwrap();

        let table = &schema_json["tables"][0];
        assert!(
            table["analysis"].is_object(),
            "analysis should be an object"
        );
        assert_eq!(table["analysis"]["findings"][0]["kind"], "lookup_table");
        assert_eq!(table["analysis"]["findings"][0]["confidence"], 1.0);
    }

    #[test]
    fn schema_md_renders_analysis_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.tables[0].analysis = Some(TableAnalysis {
            findings: vec![AnalysisFinding {
                kind: AnalysisKind::LookupTable,
                confidence: 1.0,
                evidence: vec!["has a primary key".to_string()],
            }],
        });
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap();

        let contents = fs::read_to_string(dir.path().join("schema.md")).unwrap();
        assert!(contents.contains("#### Analysis"));
        assert!(contents.contains("lookup table"));
        assert!(contents.contains("has a primary key"));
    }

    #[test]
    fn schema_json_omits_analysis_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                ..Default::default()
            },
        )
        .unwrap();

        let schema_json: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("schema.json")).unwrap())
                .unwrap();

        assert!(
            schema_json["tables"][0]["analysis"].is_null(),
            "analysis should be omitted when absent"
        );
    }

    #[test]
    fn no_markdown_skips_schema_md() {
        let dir = tempfile::tempdir().unwrap();
        let db = database();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                no_markdown: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(!dir.path().join("schema.md").exists());
        assert!(dir.path().join("schema.json").exists());
    }

    #[test]
    fn format_markdown_writes_only_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let db = database();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                no_json: true,
                no_tables: true,
                no_mermaid: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(dir.path().join("schema.md").exists());
        assert!(!dir.path().join("schema.json").exists());
        assert!(!dir.path().join("graph.mmd").exists());
        assert!(!dir.path().join("tables").exists());
    }

    #[test]
    fn render_markdown_ends_with_a_trailing_newline() {
        let mut db = database();
        db.sort();

        let md = render_markdown(&db);

        assert!(md.ends_with('\n'));
        assert!(md.contains("# shop"));
        assert!(md.contains("### orders"));
    }

    #[test]
    fn schema_md_snapshot_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                no_json: true,
                ..Default::default()
            },
        )
        .unwrap();

        let contents = fs::read_to_string(dir.path().join("schema.md")).unwrap();
        insta::assert_snapshot!(contents);
    }

    #[test]
    fn export_writes_graph_mmd() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                no_json: true,
                no_markdown: true,
                ..Default::default()
            },
        )
        .unwrap();

        let path = dir.path().join("graph.mmd");
        assert!(path.exists());
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.starts_with("erDiagram\n"));
        assert!(contents.contains("orders {\n"));
        assert!(contents.contains("int id PK"));
        assert!(contents.contains("int customer_id FK"));
        assert!(contents.contains("customers ||--o{ orders : fk_orders_customer"));
    }

    #[test]
    fn no_mermaid_skips_graph_mmd() {
        let dir = tempfile::tempdir().unwrap();
        let db = database();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                no_mermaid: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(!dir.path().join("graph.mmd").exists());
        assert!(dir.path().join("schema.json").exists());
    }

    #[test]
    fn render_mermaid_ends_with_a_trailing_newline() {
        let mut db = database();
        db.sort();

        let mmd = render_mermaid(&db);

        assert!(mmd.ends_with('\n'));
        assert!(mmd.starts_with("erDiagram\n"));
    }

    #[test]
    fn graph_mmd_snapshot_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = database();
        db.sort();

        export(
            &db,
            &ExportOptions {
                output: dir.path().to_path_buf(),
                no_json: true,
                no_markdown: true,
                ..Default::default()
            },
        )
        .unwrap();

        let contents = fs::read_to_string(dir.path().join("graph.mmd")).unwrap();
        insta::assert_snapshot!(contents);
    }

    mod validation {
        use std::collections::HashMap;

        use super::*;
        use jsonschema::{Retrieve, Uri};
        use serde_json::Value;

        struct SchemaRetriever(HashMap<String, Value>);

        impl Retrieve for SchemaRetriever {
            fn retrieve(
                &self,
                uri: &Uri<String>,
            ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
                self.0
                    .get(uri.as_str())
                    .cloned()
                    .ok_or_else(|| format!("schema not found: {uri}").into())
            }
        }

        fn make_retriever() -> SchemaRetriever {
            let schemas: HashMap<String, Value> = [
                (
                    "https://github.com/btafoya/dbctx/schemas/schema-1.0.json",
                    include_str!("../schemas/schema-1.0.json"),
                ),
                (
                    "https://github.com/btafoya/dbctx/schemas/relationships-1.0.json",
                    include_str!("../schemas/relationships-1.0.json"),
                ),
                (
                    "https://github.com/btafoya/dbctx/schemas/metadata-1.0.json",
                    include_str!("../schemas/metadata-1.0.json"),
                ),
                (
                    "https://github.com/btafoya/dbctx/schemas/table-1.0.json",
                    include_str!("../schemas/table-1.0.json"),
                ),
            ]
            .into_iter()
            .map(|(uri, source)| (uri.to_string(), serde_json::from_str(source).unwrap()))
            .collect();
            SchemaRetriever(schemas)
        }

        fn validator_for(schema_value: &Value) -> jsonschema::Validator {
            jsonschema::options()
                .with_retriever(make_retriever())
                .build(schema_value)
                .expect("schema compiles")
        }

        fn schema_validator(name: &str) -> jsonschema::Validator {
            let schemas = make_retriever().0;
            let schema = schemas
                .get(&format!("https://github.com/btafoya/dbctx/schemas/{name}"))
                .expect("known schema")
                .clone();
            validator_for(&schema)
        }

        fn assert_validates(instance: &Value, schema_name: &str) {
            let validator = schema_validator(schema_name);
            if let Some(error) = validator.iter_errors(instance).next() {
                panic!(
                    "instance does not validate against {schema_name}: {error} at {}",
                    error.instance_path
                );
            }
        }

        #[test]
        fn schema_json_validates_against_schema_1_0() {
            let dir = tempfile::tempdir().unwrap();
            let mut db = database();
            db.sort();

            export(
                &db,
                &ExportOptions {
                    output: dir.path().to_path_buf(),
                    ..Default::default()
                },
            )
            .unwrap();

            let schema_json: Value =
                serde_json::from_str(&fs::read_to_string(dir.path().join("schema.json")).unwrap())
                    .unwrap();
            assert_validates(&schema_json, "schema-1.0.json");
        }

        #[test]
        fn schema_json_with_analysis_validates_against_schema_1_0() {
            let dir = tempfile::tempdir().unwrap();
            let mut db = database();
            db.tables[0].analysis = Some(TableAnalysis {
                findings: vec![AnalysisFinding {
                    kind: AnalysisKind::LookupTable,
                    confidence: 1.0,
                    evidence: vec!["has a primary key".to_string()],
                }],
            });
            db.sort();

            export(
                &db,
                &ExportOptions {
                    output: dir.path().to_path_buf(),
                    ..Default::default()
                },
            )
            .unwrap();

            let schema_json: Value =
                serde_json::from_str(&fs::read_to_string(dir.path().join("schema.json")).unwrap())
                    .unwrap();
            assert_validates(&schema_json, "schema-1.0.json");
        }

        #[test]
        fn relationships_json_validates_against_relationships_1_0() {
            let dir = tempfile::tempdir().unwrap();
            let mut db = database();
            db.sort();

            export(
                &db,
                &ExportOptions {
                    output: dir.path().to_path_buf(),
                    ..Default::default()
                },
            )
            .unwrap();

            let relationships_json: Value = serde_json::from_str(
                &fs::read_to_string(dir.path().join("relationships.json")).unwrap(),
            )
            .unwrap();
            assert_validates(&relationships_json, "relationships-1.0.json");
        }

        #[test]
        fn metadata_json_validates_against_metadata_1_0() {
            let dir = tempfile::tempdir().unwrap();
            let mut db = database();
            db.sort();

            export(
                &db,
                &ExportOptions {
                    output: dir.path().to_path_buf(),
                    ..Default::default()
                },
            )
            .unwrap();

            let metadata_json: Value = serde_json::from_str(
                &fs::read_to_string(dir.path().join("metadata.json")).unwrap(),
            )
            .unwrap();
            assert_validates(&metadata_json, "metadata-1.0.json");
        }

        #[test]
        fn table_json_validates_against_table_1_0() {
            let dir = tempfile::tempdir().unwrap();
            let mut db = database();
            db.sort();

            export(
                &db,
                &ExportOptions {
                    output: dir.path().to_path_buf(),
                    ..Default::default()
                },
            )
            .unwrap();

            let table_json: Value = serde_json::from_str(
                &fs::read_to_string(dir.path().join("tables").join("orders.json")).unwrap(),
            )
            .unwrap();
            assert_validates(&table_json, "table-1.0.json");
        }
    }
}
