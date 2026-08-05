//! Deterministic analysis heuristics over the canonical schema model.
//!
//! Analysis is optional and never mutates factual metadata. It produces
//! deterministic classifications such as "junction table" or "lookup table"
//! based only on the shape of the schema that introspection already recorded.
//!
//! See `SPEC.md` §13 and `FORMAT.md` §--analyze.

use crate::model::{AnalysisFinding, AnalysisKind, Database, Table, TableAnalysis};

/// Run every deterministic heuristic against `database` and attach findings to
/// each table.
///
/// Tables with no matching findings keep `analysis: None`, so the field is
/// omitted from exported documents unless it carries real content.
pub fn analyze(database: &mut Database) {
    for table in &mut database.tables {
        let mut findings: Vec<AnalysisFinding> = Vec::new();

        if let Some(finding) = junction_table(table) {
            findings.push(finding);
        }
        if let Some(finding) = lookup_table(table) {
            findings.push(finding);
        }
        if let Some(finding) = audit_table(table) {
            findings.push(finding);
        }
        if let Some(finding) = soft_deletes(table) {
            findings.push(finding);
        }
        if let Some(finding) = timestamp_conventions(table) {
            findings.push(finding);
        }

        if !findings.is_empty() {
            findings.sort_by(|a, b| {
                let kind_order = format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind));
                kind_order.then(a.evidence.cmp(&b.evidence))
            });
            table.analysis = Some(TableAnalysis { findings });
        }
    }
}

fn junction_table(table: &Table) -> Option<AnalysisFinding> {
    if table.foreign_keys.len() != 2 {
        return None;
    }

    let first_target = &table.foreign_keys[0].referenced_table;
    let second_target = &table.foreign_keys[1].referenced_table;
    if first_target == second_target {
        return None;
    }

    let pk_columns: Vec<&str> = table
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.as_str())
        .collect();

    let fk_columns: Vec<&str> = table
        .foreign_keys
        .iter()
        .flat_map(|fk| fk.columns.iter().map(|c| c.as_str()))
        .collect();

    if pk_columns.is_empty() || pk_columns.len() != fk_columns.len() {
        return None;
    }

    let mut pk_sorted: Vec<&str> = pk_columns.clone();
    let mut fk_sorted: Vec<&str> = fk_columns.clone();
    pk_sorted.sort();
    fk_sorted.sort();

    if pk_sorted != fk_sorted {
        return None;
    }

    Some(AnalysisFinding {
        kind: AnalysisKind::JunctionTable,
        confidence: 1.0,
        evidence: vec![
            format!(
                "two foreign keys reference different tables: {first_target} and {second_target}"
            ),
            "primary key consists of the foreign key columns".to_string(),
        ],
    })
}

fn lookup_table(table: &Table) -> Option<AnalysisFinding> {
    if !table.foreign_keys.is_empty() {
        return None;
    }

    if !table.columns.iter().any(|c| c.primary_key) {
        return None;
    }

    if table.columns.len() > 3 {
        return None;
    }

    let name_like = ["name", "label", "value", "code", "title", "description"];
    if !table
        .columns
        .iter()
        .any(|c| name_like.contains(&c.name.to_lowercase().as_str()))
    {
        return None;
    }

    Some(AnalysisFinding {
        kind: AnalysisKind::LookupTable,
        confidence: 1.0,
        evidence: vec![
            "no foreign keys".to_string(),
            "has a primary key".to_string(),
            format!(
                "{} columns, suggesting a small code/value table",
                table.columns.len()
            ),
            "has a name/value column".to_string(),
        ],
    })
}

fn audit_table(table: &Table) -> Option<AnalysisFinding> {
    let lower_name = table.name.to_lowercase();
    let name_suggests_audit = lower_name.contains("audit")
        || lower_name.starts_with("audit_")
        || lower_name.ends_with("_audit");

    let has_timestamp = table
        .columns
        .iter()
        .any(|c| is_timestamp_type(&c.data_type) || is_timestamp_type(&c.name));

    if !name_suggests_audit && !has_timestamp {
        return None;
    }

    let mut evidence = Vec::new();
    if name_suggests_audit {
        evidence.push("table name suggests auditing".to_string());
    }
    if has_timestamp {
        evidence.push("has timestamp columns".to_string());
    }

    // Require at least the name hint to call it an audit table; timestamp-only
    // tables are covered by timestamp_conventions instead.
    if !name_suggests_audit {
        return None;
    }

    Some(AnalysisFinding {
        kind: AnalysisKind::AuditTable,
        confidence: 1.0,
        evidence,
    })
}

fn soft_deletes(table: &Table) -> Option<AnalysisFinding> {
    if let Some(column) = table
        .columns
        .iter()
        .find(|c| c.name.to_lowercase() == "deleted_at")
    {
        return Some(AnalysisFinding {
            kind: AnalysisKind::SoftDeletes,
            confidence: 1.0,
            evidence: vec![format!(
                "has a nullable `deleted_at` {} column",
                column.data_type
            )],
        });
    }

    if let Some(column) = table
        .columns
        .iter()
        .find(|c| c.name.to_lowercase() == "is_deleted")
    {
        return Some(AnalysisFinding {
            kind: AnalysisKind::SoftDeletes,
            confidence: 1.0,
            evidence: vec![format!("has an `is_deleted` {} column", column.data_type)],
        });
    }

    None
}

fn timestamp_conventions(table: &Table) -> Option<AnalysisFinding> {
    let names: std::collections::HashSet<String> = table
        .columns
        .iter()
        .map(|c| c.name.to_lowercase())
        .collect();

    let created = names.contains("created_at")
        || names.contains("created")
        || names.contains("create_time")
        || names.contains("created_on");
    let updated = names.contains("updated_at")
        || names.contains("updated")
        || names.contains("update_time")
        || names.contains("updated_on");

    if created && updated {
        Some(AnalysisFinding {
            kind: AnalysisKind::TimestampConventions,
            confidence: 1.0,
            evidence: vec!["has both created and updated timestamp columns".to_string()],
        })
    } else {
        None
    }
}

fn is_timestamp_type(data_type: &str) -> bool {
    let lower = data_type.to_lowercase();
    matches!(
        lower.as_str(),
        "timestamp" | "datetime" | "datetime2" | "smalldatetime" | "datetimeoffset"
    ) || lower.starts_with("timestamp(")
        || lower.starts_with("datetime(")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Column, DatabaseMetadata, DocumentHeader, Engine, ForeignKey, Index, Table,
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

    fn database(tables: Vec<Table>) -> Database {
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
            views: Vec::new(),
            attributes: std::collections::BTreeMap::new(),
        }
    }

    fn kinds(findings: &[AnalysisFinding]) -> Vec<AnalysisKind> {
        findings.iter().map(|f| f.kind).collect()
    }

    #[test]
    fn junction_table_is_detected_when_two_foreign_keys_form_the_primary_key() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1)];
        customers.columns[0].primary_key = true;

        let mut products = table("dbo", "products");
        products.columns = vec![column("id", 1)];
        products.columns[0].primary_key = true;

        let mut customer_products = table("dbo", "customer_products");
        customer_products.columns = vec![column("customer_id", 1), column("product_id", 2)];
        customer_products.columns[0].primary_key = true;
        customer_products.columns[1].primary_key = true;
        customer_products.foreign_keys = vec![
            ForeignKey {
                name: "fk_cp_customer".to_string(),
                columns: vec!["customer_id".to_string()],
                referenced_schema: "dbo".to_string(),
                referenced_table: "customers".to_string(),
                referenced_columns: vec!["id".to_string()],
                on_update: "NO ACTION".to_string(),
                on_delete: "CASCADE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            },
            ForeignKey {
                name: "fk_cp_product".to_string(),
                columns: vec!["product_id".to_string()],
                referenced_schema: "dbo".to_string(),
                referenced_table: "products".to_string(),
                referenced_columns: vec!["id".to_string()],
                on_update: "NO ACTION".to_string(),
                on_delete: "CASCADE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            },
        ];

        let mut db = database(vec![customers, products, customer_products]);
        analyze(&mut db);

        let cp = db
            .tables
            .iter()
            .find(|t| t.name == "customer_products")
            .unwrap();
        assert_eq!(
            kinds(&cp.analysis.as_ref().unwrap().findings),
            [AnalysisKind::JunctionTable]
        );
    }

    #[test]
    fn non_junction_table_with_two_foreign_keys_to_same_target_is_not_a_junction() {
        let mut a = table("dbo", "a");
        a.columns = vec![column("id", 1)];
        a.columns[0].primary_key = true;

        let mut self_refs = table("dbo", "self_refs");
        self_refs.columns = vec![column("id", 1), column("a_id1", 2), column("a_id2", 3)];
        self_refs.columns[0].primary_key = true;
        self_refs.foreign_keys = vec![
            ForeignKey {
                name: "fk_1".to_string(),
                columns: vec!["a_id1".to_string()],
                referenced_schema: "dbo".to_string(),
                referenced_table: "a".to_string(),
                referenced_columns: vec!["id".to_string()],
                on_update: "NO ACTION".to_string(),
                on_delete: "CASCADE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            },
            ForeignKey {
                name: "fk_2".to_string(),
                columns: vec!["a_id2".to_string()],
                referenced_schema: "dbo".to_string(),
                referenced_table: "a".to_string(),
                referenced_columns: vec!["id".to_string()],
                on_update: "NO ACTION".to_string(),
                on_delete: "CASCADE".to_string(),
                attributes: std::collections::BTreeMap::new(),
            },
        ];

        let mut db = database(vec![a, self_refs]);
        analyze(&mut db);

        let self_refs = db.tables.iter().find(|t| t.name == "self_refs").unwrap();
        assert!(
            self_refs.analysis.is_none(),
            "two foreign keys to the same table should not be classified as a junction"
        );
    }

    #[test]
    fn lookup_table_is_detected() {
        let mut statuses = table("dbo", "statuses");
        statuses.columns = vec![column("id", 1), column("code", 2), column("name", 3)];
        statuses.columns[0].primary_key = true;
        statuses.indexes.push(Index {
            name: "PRIMARY".to_string(),
            unique: true,
            columns: vec!["id".to_string()],
            index_type: "BTREE".to_string(),
            attributes: std::collections::BTreeMap::new(),
        });

        let mut db = database(vec![statuses]);
        analyze(&mut db);

        let statuses = db.tables.iter().find(|t| t.name == "statuses").unwrap();
        assert_eq!(
            kinds(&statuses.analysis.as_ref().unwrap().findings),
            [AnalysisKind::LookupTable]
        );
    }

    #[test]
    fn wide_table_is_not_a_lookup_table() {
        let mut statuses = table("dbo", "statuses");
        statuses.columns = vec![
            column("id", 1),
            column("code", 2),
            column("name", 3),
            column("description", 4),
        ];
        statuses.columns[0].primary_key = true;

        let mut db = database(vec![statuses]);
        analyze(&mut db);

        let statuses = db.tables.iter().find(|t| t.name == "statuses").unwrap();
        assert!(statuses.analysis.is_none());
    }

    #[test]
    fn audit_table_is_detected_by_name() {
        let mut audit = table("dbo", "orders_audit");
        audit.columns = vec![
            column("id", 1),
            column("action", 2),
            column("changed_at", 3),
        ];
        audit.columns[0].primary_key = true;
        audit.columns[2].data_type = "timestamp".to_string();

        let mut db = database(vec![audit]);
        analyze(&mut db);

        let audit = db.tables.iter().find(|t| t.name == "orders_audit").unwrap();
        assert_eq!(
            kinds(&audit.analysis.as_ref().unwrap().findings),
            [AnalysisKind::AuditTable]
        );
    }

    #[test]
    fn soft_deletes_are_detected() {
        let mut products = table("dbo", "products");
        products.columns = vec![
            column("id", 1),
            column("name", 2),
            column("archived", 3),
            column("deleted_at", 4),
        ];
        products.columns[0].primary_key = true;
        products.columns[3].data_type = "datetime".to_string();
        products.columns[3].nullable = true;

        let mut db = database(vec![products]);
        analyze(&mut db);

        let products = db.tables.iter().find(|t| t.name == "products").unwrap();
        assert_eq!(
            kinds(&products.analysis.as_ref().unwrap().findings),
            [AnalysisKind::SoftDeletes]
        );
    }

    #[test]
    fn timestamp_conventions_are_detected() {
        let mut users = table("dbo", "users");
        users.columns = vec![
            column("id", 1),
            column("email", 2),
            column("created_at", 3),
            column("updated_at", 4),
        ];
        users.columns[0].primary_key = true;
        users.columns[2].data_type = "timestamp".to_string();
        users.columns[3].data_type = "timestamp".to_string();

        let mut db = database(vec![users]);
        analyze(&mut db);

        let users = db.tables.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(
            kinds(&users.analysis.as_ref().unwrap().findings),
            [AnalysisKind::TimestampConventions]
        );
    }

    #[test]
    fn multiple_findings_are_sorted() {
        let mut order_logs = table("dbo", "orders_audit");
        order_logs.columns = vec![
            column("id", 1),
            column("created_at", 2),
            column("updated_at", 3),
        ];
        order_logs.columns[0].primary_key = true;
        order_logs.columns[1].data_type = "timestamp".to_string();
        order_logs.columns[2].data_type = "timestamp".to_string();

        let mut db = database(vec![order_logs]);
        analyze(&mut db);

        let order_logs = db.tables.iter().find(|t| t.name == "orders_audit").unwrap();
        let findings = &order_logs.analysis.as_ref().unwrap().findings;
        assert_eq!(
            kinds(findings),
            [AnalysisKind::AuditTable, AnalysisKind::TimestampConventions]
        );
    }

    #[test]
    fn tables_with_no_matching_heuristics_have_no_analysis() {
        let mut weird = table("dbo", "weird");
        weird.columns = vec![
            column("a", 1),
            column("b", 2),
            column("c", 3),
            column("d", 4),
        ];
        weird.columns[0].primary_key = true;

        let mut db = database(vec![weird]);
        analyze(&mut db);

        let weird = db.tables.iter().find(|t| t.name == "weird").unwrap();
        assert!(weird.analysis.is_none());
    }

    #[test]
    fn analysis_does_not_change_factual_fields() {
        let mut users = table("dbo", "users");
        users.columns = vec![
            column("id", 1),
            column("created_at", 2),
            column("updated_at", 3),
        ];
        users.columns[0].primary_key = true;
        users.columns[1].data_type = "timestamp".to_string();
        users.columns[2].data_type = "timestamp".to_string();

        let mut db = database(vec![users]);
        let before = db.tables[0].clone();
        analyze(&mut db);
        let after = db.tables[0].clone();

        assert_eq!(before.schema, after.schema);
        assert_eq!(before.name, after.name);
        assert_eq!(before.columns, after.columns);
    }
}
