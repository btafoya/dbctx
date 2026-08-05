//! The canonical schema model.
//!
//! This is the single source of truth every exporter, validation rule and
//! analysis heuristic consumes. It holds facts read from a database catalog
//! plus the document header that identifies the format they are written in:
//! no engine-specific logic and no derived opinions. `FORMAT.md` defines the
//! fields; `SPEC.md` §8 defines which of them an engine may leave null.
//!
//! Engine-specific fields are `None` when the source engine does not provide
//! them. They are never given placeholder values.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer};

use crate::VERSION;

/// Version of the document format defined by `FORMAT.md`, independent of the
/// application version reported by [`crate::VERSION`].
pub const FORMAT_VERSION: &str = "1.0";

/// The header every dbctx document begins with.
///
/// The header is data, not behavior: [`DocumentHeader::new`] takes the
/// timestamp rather than reading the clock, so the model has no hidden side
/// effects and a caller can reproduce a document exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentHeader {
    /// Document type, for example `dbctx.schema`.
    pub format: String,
    /// Format version the document conforms to.
    pub format_version: String,
    /// Program that wrote the document.
    pub generator: Generator,
    /// Time the document was generated, as an RFC 3339 UTC timestamp.
    pub generated_at: String,
}

/// The program that wrote a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generator {
    /// Program name.
    pub name: String,
    /// Program version.
    pub version: String,
}

impl DocumentHeader {
    /// A header for `format`, generated at `generated_at`, stamped with this
    /// build's format version and generator.
    ///
    /// `generated_at` is an RFC 3339 UTC timestamp such as
    /// `2026-01-01T00:00:00Z`. Documents read back from disk keep whatever
    /// they were written with.
    pub fn new(format: impl Into<String>, generated_at: impl Into<String>) -> Self {
        Self {
            format: format.into(),
            format_version: FORMAT_VERSION.to_string(),
            generator: Generator {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: VERSION.to_string(),
            },
            generated_at: generated_at.into(),
        }
    }
}

/// The database engine a schema was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// MySQL.
    Mysql,
    /// MariaDB.
    Mariadb,
    /// Microsoft SQL Server.
    Sqlserver,
    /// PostgreSQL.
    Postgres,
    /// SQLite.
    Sqlite,
}

/// A whole database: its metadata, tables and views.
///
/// Serializes as a complete [`Database::FORMAT`] document, header first,
/// ending with the `relationships` array.
///
/// Relationships are not a field. They restate the foreign keys the tables
/// already hold, so they are derived by [`Database::relationships`] whenever
/// the document is written and cannot disagree with the keys they came from.
/// A `relationships` array in a document being read is ignored for the same
/// reason.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Database {
    /// Document header, written as the leading fields of the document.
    #[serde(flatten)]
    pub header: DocumentHeader,
    /// Database-level facts.
    pub metadata: DatabaseMetadata,
    /// Tables, sorted by schema then name once [`Database::sort`] has run.
    pub tables: Vec<Table>,
    /// Views, sorted by schema then name once [`Database::sort`] has run.
    pub views: Vec<View>,
    /// Engine-specific facts that do not fit the fields above. Empty for
    /// MySQL, MariaDB and SQL Server.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

/// The serialized shape of a [`Database`], with the relationships derived
/// from the tables' foreign keys.
///
/// Borrowing the stored fields and deriving `Serialize` keeps the document's
/// field names in one place: a field added to [`DocumentHeader`] flows
/// through, and a field added to [`Database`] is caught by
/// `schema_documents_expose_exactly_the_documented_fields`.
#[derive(Serialize)]
struct SchemaDocument<'a> {
    #[serde(flatten)]
    header: &'a DocumentHeader,
    metadata: &'a DatabaseMetadata,
    tables: &'a [Table],
    views: &'a [View],
    relationships: Vec<Relationship>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    attributes: &'a BTreeMap<String, serde_json::Value>,
}

impl Serialize for Database {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SchemaDocument {
            header: &self.header,
            metadata: &self.metadata,
            tables: &self.tables,
            views: &self.views,
            relationships: self.relationships(),
            attributes: &self.attributes,
        }
        .serialize(serializer)
    }
}

/// Database-level facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseMetadata {
    /// Name of the inspected database.
    pub database: String,
    /// Engine the database runs on.
    pub engine: Engine,
    /// Engine version string as the server reports it.
    pub engine_version: String,
    /// Default character set. `None` on SQL Server, which has no
    /// database-level charset.
    pub default_charset: Option<String>,
    /// Default collation. `None` on SQL Server.
    pub default_collation: Option<String>,
    /// Engine-specific facts that do not fit the fields above. Empty for
    /// MySQL, MariaDB and SQL Server.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

/// A table and everything defined on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// Object namespace: the database name on MySQL and MariaDB, the SQL
    /// Server schema (for example `dbo`) on SQL Server.
    pub schema: String,
    /// Table name.
    pub name: String,
    /// Storage engine, for example `InnoDB`. `None` on SQL Server.
    pub engine: Option<String>,
    /// Character set. `None` on SQL Server.
    pub charset: Option<String>,
    /// Collation. `None` on SQL Server.
    pub collation: Option<String>,
    /// Table comment, if the catalog records one.
    pub comment: Option<String>,
    /// Columns, sorted by [`Column::ordinal_position`].
    pub columns: Vec<Column>,
    /// Indexes, sorted by name.
    pub indexes: Vec<Index>,
    /// Foreign keys, sorted by name.
    pub foreign_keys: Vec<ForeignKey>,
    /// Deterministic analysis findings, present only when `--analyze` was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis: Option<TableAnalysis>,

    /// AI-generated context, present only when `--llm` was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiContext>,

    /// Engine-specific facts that do not fit the fields above. Empty for
    /// MySQL, MariaDB and SQL Server.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

/// A view and the columns it exposes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    /// Object namespace, following the same rule as [`Table::schema`].
    pub schema: String,
    /// View name.
    pub name: String,
    /// Columns, sorted by [`Column::ordinal_position`].
    pub columns: Vec<Column>,
    /// Engine-specific facts that do not fit the fields above. Empty for
    /// MySQL, MariaDB and SQL Server.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

/// A column of a table or view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    /// Column name.
    pub name: String,
    /// One-based position within the table, as the catalog reports it.
    pub ordinal_position: u32,
    /// Bare type name, for example `varchar`.
    pub data_type: String,
    /// Full declared type, for example `varchar(255)`.
    pub full_type: String,
    /// Whether the column accepts NULL.
    pub nullable: bool,
    /// Declared default. `None` means no default, which is distinct from a
    /// default of NULL.
    pub default: Option<String>,
    /// Whether the engine assigns values automatically: `AUTO_INCREMENT` on
    /// MySQL and MariaDB, `IDENTITY` on SQL Server.
    pub auto_increment: bool,
    /// Whether the column takes part in the primary key.
    pub primary_key: bool,
    /// Whether a unique constraint covers the column on its own.
    pub unique: bool,
    /// Column comment, if the catalog records one.
    pub comment: Option<String>,
    /// Whether the column is computed rather than stored input.
    pub generated: bool,
    /// Expression behind a generated column.
    pub expression: Option<String>,
    /// Engine-specific facts that do not fit the fields above. Empty for
    /// MySQL, MariaDB and SQL Server.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

/// An index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Index {
    /// Index name.
    pub name: String,
    /// Whether the index enforces uniqueness.
    pub unique: bool,
    /// Indexed columns, in index order, which is significant.
    pub columns: Vec<String>,
    /// Type as the engine reports it: `BTREE` or `HASH` on MySQL and MariaDB,
    /// `CLUSTERED` or `NONCLUSTERED` on SQL Server. Never normalized across
    /// engines.
    pub index_type: String,
    /// Engine-specific facts that do not fit the fields above. Empty for
    /// MySQL, MariaDB and SQL Server.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

/// A foreign key constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKey {
    /// Constraint name.
    pub name: String,
    /// Constrained columns, in constraint order, which is significant.
    pub columns: Vec<String>,
    /// Namespace of the referenced table. Always populated, so a reference
    /// stays unambiguous where table names repeat across schemas.
    pub referenced_schema: String,
    /// Referenced table name.
    pub referenced_table: String,
    /// Referenced columns, positionally matching [`ForeignKey::columns`].
    pub referenced_columns: Vec<String>,
    /// Referential action on update, for example `CASCADE`.
    pub on_update: String,
    /// Referential action on delete, for example `NO ACTION`.
    pub on_delete: String,
    /// Engine-specific facts that do not fit the fields above. Empty for
    /// MySQL, MariaDB and SQL Server.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

/// Deterministic classification of a table produced by the analysis layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisKind {
    /// Many-to-many join table with foreign keys forming the primary key.
    JunctionTable,
    /// Small code/value table with no foreign keys.
    LookupTable,
    /// Table whose name or columns suggest change auditing.
    AuditTable,
    /// Table has a flag or timestamp column marking deleted rows.
    SoftDeletes,
    /// Table has both `created_at` and `updated_at` style columns.
    TimestampConventions,
}

impl AnalysisKind {
    /// Human-readable label for this classification.
    pub fn label(&self) -> &'static str {
        match self {
            AnalysisKind::JunctionTable => "junction table",
            AnalysisKind::LookupTable => "lookup table",
            AnalysisKind::AuditTable => "audit table",
            AnalysisKind::SoftDeletes => "soft deletes",
            AnalysisKind::TimestampConventions => "timestamp conventions",
        }
    }
}

/// One deterministic observation about a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisFinding {
    /// Classification produced by the heuristic.
    pub kind: AnalysisKind,
    /// Strength of the match. Deterministic heuristics report 1.0 when they
    /// match and the table is omitted otherwise, so confidence is always 1.0
    /// for any emitted finding.
    pub confidence: f64,
    /// Human-readable rules that matched, for example
    /// "table name ends with '_audit'".
    pub evidence: Vec<String>,
}

/// All deterministic analysis findings attached to a single table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableAnalysis {
    /// Findings, sorted by kind and then evidence for stability.
    pub findings: Vec<AnalysisFinding>,
}

/// AI-generated context attached to a single table when `--llm` is enabled.
///
/// Every field here is generated from the canonical model and any analysis
/// findings; it never overwrites factual metadata and is clearly labeled as
/// generated content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiContext {
    /// Always `true`, marking this section as AI-generated.
    pub generated: bool,
    /// Confidence score for the generated context. Deterministic generation
    /// uses a fixed value so the output is stable.
    pub confidence: f64,
    /// Brief summary of the table's role and shape.
    pub summary: String,
    /// Relationship narratives and entry-point suggestions for this table.
    pub notes: Vec<String>,
}

/// One foreign key expressed as a directed relationship between two tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    /// Namespace of the referencing table.
    pub from_schema: String,
    /// Referencing table.
    pub from_table: String,
    /// Referencing columns.
    pub from_columns: Vec<String>,
    /// Namespace of the referenced table.
    pub to_schema: String,
    /// Referenced table.
    pub to_table: String,
    /// Referenced columns.
    pub to_columns: Vec<String>,
    /// Name of the foreign key constraint.
    pub constraint: String,
}

impl Database {
    /// Document type this model serializes as.
    pub const FORMAT: &'static str = "dbctx.schema";

    /// Sort every collection into the deterministic order `FORMAT.md`
    /// requires, so output is stable regardless of the order introspection
    /// happened to collect facts in.
    ///
    /// Column order follows the catalog ordinal position, which is
    /// semantically meaningful. Index and foreign key column lists keep their
    /// declared order for the same reason.
    pub fn sort(&mut self) {
        self.tables
            .sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
        self.views
            .sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
        for table in &mut self.tables {
            table.columns.sort_by_key(|c| c.ordinal_position);
            table.indexes.sort_by(|a, b| a.name.cmp(&b.name));
            table.foreign_keys.sort_by(|a, b| a.name.cmp(&b.name));
        }
        for view in &mut self.views {
            view.columns.sort_by_key(|c| c.ordinal_position);
        }
    }

    /// Every foreign key as a [`Relationship`], sorted by source namespace,
    /// source table, target namespace, target table, then constraint name.
    pub fn relationships(&self) -> Vec<Relationship> {
        let mut relationships: Vec<Relationship> = self
            .tables
            .iter()
            .flat_map(|table| {
                table.foreign_keys.iter().map(|fk| Relationship {
                    from_schema: table.schema.clone(),
                    from_table: table.name.clone(),
                    from_columns: fk.columns.clone(),
                    to_schema: fk.referenced_schema.clone(),
                    to_table: fk.referenced_table.clone(),
                    to_columns: fk.referenced_columns.clone(),
                    constraint: fk.name.clone(),
                })
            })
            .collect();
        relationships.sort_by(|a, b| {
            (
                &a.from_schema,
                &a.from_table,
                &a.to_schema,
                &a.to_table,
                &a.constraint,
            )
                .cmp(&(
                    &b.from_schema,
                    &b.from_table,
                    &b.to_schema,
                    &b.to_table,
                    &b.constraint,
                ))
        });
        relationships
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            attributes: std::collections::BTreeMap::new(),
        }
    }

    fn foreign_key(name: &str, to_schema: &str, to_table: &str) -> ForeignKey {
        ForeignKey {
            name: name.to_string(),
            columns: vec!["b".to_string(), "a".to_string()],
            referenced_schema: to_schema.to_string(),
            referenced_table: to_table.to_string(),
            referenced_columns: vec!["y".to_string(), "x".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
            attributes: std::collections::BTreeMap::new(),
        }
    }

    fn table(schema: &str, name: &str) -> Table {
        Table {
            schema: schema.to_string(),
            name: name.to_string(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            comment: None,
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            analysis: None,
            ai: None,
            attributes: std::collections::BTreeMap::new(),
        }
    }

    fn database(tables: Vec<Table>, views: Vec<View>) -> Database {
        Database {
            header: DocumentHeader::new(Database::FORMAT, "2026-01-01T00:00:00Z"),
            metadata: DatabaseMetadata {
                database: "shop".to_string(),
                engine: Engine::Mysql,
                engine_version: "8.4.0".to_string(),
                default_charset: Some("utf8mb4".to_string()),
                default_collation: Some("utf8mb4_0900_ai_ci".to_string()),
                attributes: std::collections::BTreeMap::new(),
            },
            tables,
            views,
            attributes: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn sorting_orders_tables_by_schema_then_name() {
        let mut db = database(
            vec![
                table("sales", "customers"),
                table("dbo", "orders"),
                table("dbo", "customers"),
            ],
            Vec::new(),
        );

        db.sort();

        let order: Vec<_> = db
            .tables
            .iter()
            .map(|t| (t.schema.as_str(), t.name.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("dbo", "customers"),
                ("dbo", "orders"),
                ("sales", "customers")
            ]
        );
    }

    #[test]
    fn sorting_orders_columns_by_ordinal_position_not_name() {
        let mut orders = table("dbo", "orders");
        orders.columns = vec![
            column("id", 3),
            column("total", 1),
            column("customer_id", 2),
        ];
        let mut db = database(vec![orders], Vec::new());

        db.sort();

        let order: Vec<_> = db.tables[0]
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(order, ["total", "customer_id", "id"]);
    }

    #[test]
    fn sorting_orders_indexes_and_foreign_keys_by_name() {
        let mut orders = table("dbo", "orders");
        orders.indexes = vec![
            Index {
                name: "idx_total".to_string(),
                unique: false,
                columns: vec!["total".to_string()],
                index_type: "BTREE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            },
            Index {
                name: "PRIMARY".to_string(),
                unique: true,
                columns: vec!["id".to_string()],
                index_type: "BTREE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            },
        ];
        orders.foreign_keys = vec![
            foreign_key("fk_orders_store", "dbo", "stores"),
            foreign_key("fk_orders_customer", "dbo", "customers"),
        ];
        let mut db = database(vec![orders], Vec::new());

        db.sort();

        let indexes: Vec<_> = db.tables[0]
            .indexes
            .iter()
            .map(|i| i.name.as_str())
            .collect();
        assert_eq!(indexes, ["PRIMARY", "idx_total"]);
        let keys: Vec<_> = db.tables[0]
            .foreign_keys
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(keys, ["fk_orders_customer", "fk_orders_store"]);
    }

    #[test]
    fn sorting_orders_views_and_their_columns() {
        let mut db = database(
            Vec::new(),
            vec![
                View {
                    schema: "dbo".to_string(),
                    name: "recent_orders".to_string(),
                    columns: vec![column("total", 2), column("id", 1)],
                    attributes: std::collections::BTreeMap::new(),
                },
                View {
                    schema: "dbo".to_string(),
                    name: "active_customers".to_string(),
                    columns: Vec::new(),
                    attributes: std::collections::BTreeMap::new(),
                },
            ],
        );

        db.sort();

        let names: Vec<_> = db.views.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["active_customers", "recent_orders"]);
        let columns: Vec<_> = db.views[1]
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(columns, ["id", "total"]);
    }

    #[test]
    fn relationships_derive_from_foreign_keys_in_declared_column_order() {
        let mut orders = table("dbo", "orders");
        orders.foreign_keys = vec![foreign_key("fk_orders_customer", "sales", "customers")];
        let db = database(vec![orders], Vec::new());

        let relationships = db.relationships();

        assert_eq!(
            relationships,
            [Relationship {
                from_schema: "dbo".to_string(),
                from_table: "orders".to_string(),
                from_columns: vec!["b".to_string(), "a".to_string()],
                to_schema: "sales".to_string(),
                to_table: "customers".to_string(),
                to_columns: vec!["y".to_string(), "x".to_string()],
                constraint: "fk_orders_customer".to_string(),
            }]
        );
    }

    #[test]
    fn relationships_sort_by_source_then_target_then_constraint() {
        let mut orders = table("dbo", "orders");
        orders.foreign_keys = vec![
            foreign_key("fk_b", "dbo", "stores"),
            foreign_key("fk_a", "dbo", "stores"),
            foreign_key("fk_c", "dbo", "customers"),
        ];
        let mut shipments = table("dbo", "shipments");
        shipments.foreign_keys = vec![foreign_key("fk_d", "archive", "orders")];
        let db = database(vec![shipments, orders], Vec::new());

        let order: Vec<_> = db
            .relationships()
            .iter()
            .map(|r| {
                (
                    r.from_table.clone(),
                    r.to_table.clone(),
                    r.constraint.clone(),
                )
            })
            .collect();

        assert_eq!(
            order,
            [
                (
                    "orders".to_string(),
                    "customers".to_string(),
                    "fk_c".to_string()
                ),
                (
                    "orders".to_string(),
                    "stores".to_string(),
                    "fk_a".to_string()
                ),
                (
                    "orders".to_string(),
                    "stores".to_string(),
                    "fk_b".to_string()
                ),
                (
                    "shipments".to_string(),
                    "orders".to_string(),
                    "fk_d".to_string()
                ),
            ]
        );
    }

    #[test]
    fn model_round_trips_through_json() {
        let mut orders = table("dbo", "orders");
        orders.comment = Some("customer orders".to_string());
        orders.columns = vec![column("id", 1)];
        orders.columns[0].primary_key = true;
        orders.columns[0].auto_increment = true;
        orders.columns[0].unique = true;
        orders.columns[0].generated = true;
        orders.columns[0].expression = Some("1 + 1".to_string());
        orders.columns[0].default = Some("0".to_string());
        orders.columns[0].comment = Some("surrogate key".to_string());
        orders.indexes = vec![Index {
            name: "PRIMARY".to_string(),
            unique: true,
            columns: vec!["id".to_string()],
            index_type: "BTREE".to_string(),
            attributes: std::collections::BTreeMap::new(),
        }];
        orders.foreign_keys = vec![foreign_key("fk_orders_customer", "dbo", "customers")];

        let mut sqlserver = database(
            vec![orders],
            vec![View {
                schema: "dbo".to_string(),
                name: "recent_orders".to_string(),
                columns: vec![column("id", 1)],
                attributes: std::collections::BTreeMap::new(),
            }],
        );
        sqlserver.metadata.engine = Engine::Sqlserver;
        sqlserver.metadata.default_charset = None;
        sqlserver.metadata.default_collation = None;
        sqlserver.tables[0].engine = None;
        sqlserver.tables[0].charset = None;
        sqlserver.tables[0].collation = None;

        let json = serde_json::to_string(&sqlserver).unwrap();
        assert_eq!(serde_json::from_str::<Database>(&json).unwrap(), sqlserver);
    }

    #[test]
    fn null_engine_specific_fields_serialize_as_explicit_null() {
        let mut db = database(vec![table("dbo", "orders")], Vec::new());
        db.metadata.engine = Engine::Sqlserver;
        db.metadata.default_charset = None;
        db.metadata.default_collation = None;
        db.tables[0].engine = None;

        let json = serde_json::to_value(&db).unwrap();

        assert_eq!(json["metadata"]["default_charset"], serde_json::Value::Null);
        assert_eq!(
            json["metadata"]["default_collation"],
            serde_json::Value::Null
        );
        assert_eq!(json["tables"][0]["engine"], serde_json::Value::Null);
    }

    #[test]
    fn documents_begin_with_the_required_header_fields() {
        let db = database(Vec::new(), Vec::new());

        let json = serde_json::to_string(&db).unwrap();

        assert!(
            json.starts_with(
                r#"{"format":"dbctx.schema","format_version":"1.0","generator":{"name":"dbctx","version":"#
            ),
            "header must come first: {json}"
        );
        let value = serde_json::to_value(&db).unwrap();
        assert_eq!(value["generator"]["version"], VERSION);
        assert_eq!(value["generated_at"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn schema_documents_expose_exactly_the_documented_fields() {
        let db = database(Vec::new(), Vec::new());

        let value = serde_json::to_value(&db).unwrap();

        let mut fields: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            [
                "format",
                "format_version",
                "generated_at",
                "generator",
                "metadata",
                "relationships",
                "tables",
                "views",
            ]
        );
    }

    #[test]
    fn schema_documents_carry_the_relationships_derived_from_foreign_keys() {
        let mut orders = table("dbo", "orders");
        orders.foreign_keys = vec![foreign_key("fk_orders_customer", "sales", "customers")];
        let db = database(vec![orders], Vec::new());

        let value = serde_json::to_value(&db).unwrap();

        assert_eq!(
            value["relationships"],
            serde_json::to_value(db.relationships()).unwrap()
        );
        assert_eq!(value["relationships"][0]["from_table"], "orders");
        assert_eq!(value["relationships"][0]["to_schema"], "sales");
    }

    #[test]
    fn relationships_in_a_document_being_read_are_ignored() {
        let json = r#"{
            "format": "dbctx.schema",
            "format_version": "1.0",
            "generator": { "name": "dbctx", "version": "0.1.0" },
            "generated_at": "2026-01-01T00:00:00Z",
            "metadata": {
                "database": "shop",
                "engine": "mysql",
                "engine_version": "8.4.0",
                "default_charset": null,
                "default_collation": null
            },
            "tables": [],
            "views": [],
            "relationships": [
                {
                    "from_schema": "dbo",
                    "from_table": "invented",
                    "from_columns": [],
                    "to_schema": "dbo",
                    "to_table": "invented",
                    "to_columns": [],
                    "constraint": "not_a_real_key"
                }
            ]
        }"#;

        let db: Database = serde_json::from_str(json).unwrap();

        assert_eq!(db.relationships(), []);
    }

    #[test]
    fn headers_read_back_keep_the_version_they_were_written_with() {
        let json = r#"{
            "format": "dbctx.schema",
            "format_version": "0.9",
            "generator": { "name": "dbctx", "version": "0.0.1" },
            "generated_at": "2020-01-01T00:00:00Z",
            "metadata": {
                "database": "shop",
                "engine": "mysql",
                "engine_version": "8.4.0",
                "default_charset": null,
                "default_collation": null
            },
            "tables": [],
            "views": []
        }"#;

        let db: Database = serde_json::from_str(json).unwrap();

        assert_eq!(db.header.format_version, "0.9");
        assert_eq!(db.header.generator.version, "0.0.1");
        assert_eq!(db.header.generated_at, "2020-01-01T00:00:00Z");
    }

    #[test]
    fn engines_serialize_as_the_names_the_format_specifies() {
        for (engine, name) in [
            (Engine::Mysql, "mysql"),
            (Engine::Mariadb, "mariadb"),
            (Engine::Sqlserver, "sqlserver"),
            (Engine::Postgres, "postgres"),
            (Engine::Sqlite, "sqlite"),
        ] {
            assert_eq!(
                serde_json::to_value(engine).unwrap(),
                serde_json::Value::String(name.to_string())
            );
            assert_eq!(
                serde_json::from_str::<Engine>(&format!("\"{name}\"")).unwrap(),
                engine
            );
        }
    }

    #[test]
    fn empty_attributes_are_skipped_in_serialized_output() {
        let db = database(vec![table("dbo", "orders")], Vec::new());

        let value = serde_json::to_value(&db).unwrap();

        assert!(value.get("attributes").is_none());
        assert!(value["metadata"].get("attributes").is_none());
        assert!(value["tables"][0].get("attributes").is_none());
    }

    #[test]
    fn attributes_round_trip_through_json() {
        let mut db = database(vec![table("dbo", "orders")], Vec::new());
        db.attributes
            .insert("search_path".to_string(), serde_json::json!(["public"]));
        db.metadata
            .attributes
            .insert("access_method".to_string(), serde_json::json!("heap"));
        db.tables[0]
            .attributes
            .insert("without_rowid".to_string(), serde_json::json!(true));

        let json = serde_json::to_string(&db).unwrap();
        let round_tripped: Database = serde_json::from_str(&json).unwrap();

        assert_eq!(round_tripped.attributes, db.attributes);
        assert_eq!(round_tripped.metadata.attributes, db.metadata.attributes);
        assert_eq!(round_tripped.tables[0].attributes, db.tables[0].attributes);
    }

    #[test]
    fn attributes_default_to_empty_when_absent_from_a_document_being_read() {
        let json = r#"{
            "format": "dbctx.schema",
            "format_version": "1.0",
            "generator": { "name": "dbctx", "version": "0.1.0" },
            "generated_at": "2026-01-01T00:00:00Z",
            "metadata": {
                "database": "shop",
                "engine": "postgres",
                "engine_version": "16.0",
                "default_charset": null,
                "default_collation": null
            },
            "tables": [],
            "views": []
        }"#;

        let db: Database = serde_json::from_str(json).unwrap();

        assert!(db.attributes.is_empty());
        assert!(db.metadata.attributes.is_empty());
    }
}
