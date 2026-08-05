//! Optional AI-generated context over the canonical schema model.
//!
//! This layer consumes the factual metadata and any deterministic analysis
//! findings to produce human-readable summaries, relationship narratives and
//! entry-point suggestions. It contains no outbound network calls and is
//! deterministic, but every item it produces is clearly labeled as
//! AI-generated.
//!
//! See `SPEC.md` §14 and `FORMAT.md` §--llm.

use crate::model::{AiContext, Database, Engine, Relationship, Table};

/// Generate AI context for every table in `database`.
///
/// Tables always receive a context when this layer runs: even a table with no
/// relationships or analysis has a summary and an explicit `generated` flag.
pub fn generate(database: &mut Database) {
    let engine = database.metadata.engine;
    let relationships = database.relationships();
    for table in &mut database.tables {
        table.ai = Some(table_context(engine, table, &relationships));
    }
}

fn table_context(engine: Engine, table: &Table, relationships: &[Relationship]) -> AiContext {
    let mut notes = Vec::new();

    // Relationship narratives: outgoing foreign keys.
    for fk in &table.foreign_keys {
        let referenced = table_reference(engine, &fk.referenced_schema, &fk.referenced_table);
        notes.push(format!(
            "References {} through {}.",
            referenced,
            fk.columns.join(", ")
        ));
    }

    // Relationship narratives: tables that reference this one.
    for rel in relationships
        .iter()
        .filter(|r| r.to_schema == table.schema && r.to_table == table.name)
    {
        let from = table_reference(engine, &rel.from_schema, &rel.from_table);
        notes.push(format!(
            "Referenced by {} through {}.",
            from,
            rel.from_columns.join(", ")
        ));
    }

    // Entry-point suggestions.
    let pk_columns: Vec<&str> = table
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.as_str())
        .collect();
    if !pk_columns.is_empty() {
        notes.push(format!(
            "Start queries from the primary key ({}).",
            pk_columns.join(", ")
        ));
    }
    for fk in &table.foreign_keys {
        let referenced = table_reference(engine, &fk.referenced_schema, &fk.referenced_table);
        notes.push(format!(
            "Join from {} via {}.",
            referenced,
            fk.columns.join(", ")
        ));
    }

    AiContext {
        generated: true,
        confidence: 0.91,
        summary: table_summary(table),
        notes,
    }
}

fn table_reference(engine: Engine, schema: &str, name: &str) -> String {
    if engine == Engine::Sqlserver {
        format!("{schema}.{name}")
    } else {
        name.to_string()
    }
}

fn table_summary(table: &Table) -> String {
    let mut parts = Vec::new();
    parts.push(format!(
        "Table `{}` has {} columns",
        table.name,
        table.columns.len()
    ));
    if !table.indexes.is_empty() {
        parts.push(format!("{} indexes", table.indexes.len()));
    }
    if !table.foreign_keys.is_empty() {
        parts.push(format!("{} foreign keys", table.foreign_keys.len()));
    }

    let mut summary = parts.join(", ");
    summary.push('.');

    if let Some(analysis) = &table.analysis {
        let labels: Vec<&str> = analysis.findings.iter().map(|f| f.kind.label()).collect();
        if !labels.is_empty() {
            summary.push_str(" Analysis suggests: ");
            summary.push_str(&labels.join(", "));
            summary.push('.');
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AnalysisFinding, AnalysisKind, Column, DatabaseMetadata, DocumentHeader, Engine,
        ForeignKey, Index, Table, TableAnalysis,
    };

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

    fn database_with(tables: Vec<Table>) -> crate::model::Database {
        crate::model::Database {
            header: DocumentHeader::new(crate::model::Database::FORMAT, "2026-01-01T00:00:00Z"),
            metadata: DatabaseMetadata {
                database: "shop".to_string(),
                engine: Engine::Mysql,
                engine_version: "8.4.0".to_string(),
                default_charset: Some("utf8mb4".to_string()),
                default_collation: Some("utf8mb4_0900_ai_ci".to_string()),
                attributes: std::collections::BTreeMap::new(),
            },
            tables,
            views: Vec::new(),
            attributes: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn every_table_gets_ai_context_when_generate_runs() {
        let mut db = database_with(vec![Table {
            schema: "dbo".to_string(),
            name: "users".to_string(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            comment: None,
            columns: vec![column("id", 1)],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            analysis: None,
            ai: None,
            attributes: std::collections::BTreeMap::new(),
        }]);
        db.tables[0].columns[0].primary_key = true;

        generate(&mut db);

        assert!(db.tables[0].ai.is_some());
        assert!(db.tables[0].ai.as_ref().unwrap().generated);
        assert!(!db.tables[0].ai.as_ref().unwrap().summary.is_empty());
    }

    #[test]
    fn summary_counts_columns_indexes_and_foreign_keys() {
        let mut users = Table {
            schema: "dbo".to_string(),
            name: "users".to_string(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            comment: None,
            columns: vec![column("id", 1), column("email", 2)],
            indexes: vec![Index {
                name: "PRIMARY".to_string(),
                unique: true,
                columns: vec!["id".to_string()],
                index_type: "BTREE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            }],
            foreign_keys: Vec::new(),
            analysis: None,
            ai: None,
            attributes: std::collections::BTreeMap::new(),
        };
        users.columns[0].primary_key = true;

        let mut db = database_with(vec![users]);
        generate(&mut db);

        let summary = &db.tables[0].ai.as_ref().unwrap().summary;
        assert!(summary.contains("2 columns"), "{summary}");
        assert!(summary.contains("1 indexes"), "{summary}");
    }

    #[test]
    fn analysis_labels_are_mentioned_in_the_summary() {
        let mut users = Table {
            schema: "dbo".to_string(),
            name: "users".to_string(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            comment: None,
            columns: vec![
                column("id", 1),
                column("created_at", 2),
                column("updated_at", 3),
            ],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            analysis: Some(TableAnalysis {
                findings: vec![AnalysisFinding {
                    kind: AnalysisKind::TimestampConventions,
                    confidence: 1.0,
                    evidence: vec!["has both created and updated timestamp columns".to_string()],
                }],
            }),
            ai: None,
            attributes: std::collections::BTreeMap::new(),
        };
        users.columns[0].primary_key = true;

        let mut db = database_with(vec![users]);
        generate(&mut db);

        let summary = &db.tables[0].ai.as_ref().unwrap().summary;
        assert!(summary.contains("timestamp conventions"), "{summary}");
    }

    #[test]
    fn outgoing_foreign_keys_are_narrated() {
        let mut orders = Table {
            schema: "dbo".to_string(),
            name: "orders".to_string(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            comment: None,
            columns: vec![column("id", 1), column("customer_id", 2)],
            indexes: Vec::new(),
            foreign_keys: vec![ForeignKey {
                name: "fk_orders_customer".to_string(),
                columns: vec!["customer_id".to_string()],
                referenced_schema: "dbo".to_string(),
                referenced_table: "customers".to_string(),
                referenced_columns: vec!["id".to_string()],
                on_update: "NO ACTION".to_string(),
                on_delete: "CASCADE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            }],
            analysis: None,
            ai: None,
            attributes: std::collections::BTreeMap::new(),
        };
        orders.columns[0].primary_key = true;

        let mut db = database_with(vec![orders]);
        generate(&mut db);

        let notes = &db.tables[0].ai.as_ref().unwrap().notes;
        assert!(
            notes
                .iter()
                .any(|n| n.contains("References customers") && n.contains("customer_id"))
        );
    }

    #[test]
    fn incoming_foreign_keys_are_narrated() {
        let mut customers = Table {
            schema: "dbo".to_string(),
            name: "customers".to_string(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            comment: None,
            columns: vec![column("id", 1)],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            analysis: None,
            ai: None,
            attributes: std::collections::BTreeMap::new(),
        };
        customers.columns[0].primary_key = true;

        let mut orders = Table {
            schema: "dbo".to_string(),
            name: "orders".to_string(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            comment: None,
            columns: vec![column("id", 1), column("customer_id", 2)],
            indexes: Vec::new(),
            foreign_keys: vec![ForeignKey {
                name: "fk_orders_customer".to_string(),
                columns: vec!["customer_id".to_string()],
                referenced_schema: "dbo".to_string(),
                referenced_table: "customers".to_string(),
                referenced_columns: vec!["id".to_string()],
                on_update: "NO ACTION".to_string(),
                on_delete: "CASCADE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            }],
            analysis: None,
            ai: None,
            attributes: std::collections::BTreeMap::new(),
        };
        orders.columns[0].primary_key = true;

        let mut db = database_with(vec![customers, orders]);
        generate(&mut db);

        let customers_ai = db
            .tables
            .iter()
            .find(|t| t.name == "customers")
            .unwrap()
            .ai
            .as_ref()
            .unwrap();
        assert!(
            customers_ai
                .notes
                .iter()
                .any(|n| n.contains("Referenced by orders") && n.contains("customer_id"))
        );
    }

    #[test]
    fn sql_server_table_references_include_schema_prefix() {
        let mut orders = Table {
            schema: "sales".to_string(),
            name: "orders".to_string(),
            engine: None,
            charset: None,
            collation: None,
            comment: None,
            columns: vec![column("id", 1), column("customer_id", 2)],
            indexes: Vec::new(),
            foreign_keys: vec![ForeignKey {
                name: "fk_orders_customer".to_string(),
                columns: vec!["customer_id".to_string()],
                referenced_schema: "dbo".to_string(),
                referenced_table: "customers".to_string(),
                referenced_columns: vec!["id".to_string()],
                on_update: "NO ACTION".to_string(),
                on_delete: "CASCADE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            }],
            analysis: None,
            ai: None,
            attributes: std::collections::BTreeMap::new(),
        };
        orders.columns[0].primary_key = true;

        let mut db = database_with(vec![orders]);
        db.metadata.engine = Engine::Sqlserver;
        db.metadata.default_charset = None;
        db.metadata.default_collation = None;
        generate(&mut db);

        let notes = &db.tables[0].ai.as_ref().unwrap().notes;
        assert!(notes.iter().any(|n| n.contains("dbo.customers")));
    }

    #[test]
    fn generate_does_not_mutate_factual_fields() {
        let mut users = Table {
            schema: "dbo".to_string(),
            name: "users".to_string(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            comment: None,
            columns: vec![column("id", 1)],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            analysis: None,
            ai: None,
            attributes: std::collections::BTreeMap::new(),
        };
        users.columns[0].primary_key = true;

        let mut db = database_with(vec![users]);
        let before = db.tables[0].clone();
        generate(&mut db);
        let after = db.tables[0].clone();

        assert_eq!(before.schema, after.schema);
        assert_eq!(before.name, after.name);
        assert_eq!(before.columns, after.columns);
        assert_eq!(before.foreign_keys, after.foreign_keys);
        assert!(after.ai.is_some());
    }
}
