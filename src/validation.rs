//! Pure validation rule engine over the canonical schema model.
//!
//! Rules never mutate the schema. They produce a deterministic report of
//! findings that `dbctx validate` prints to stdout.

use serde::Serialize;

use crate::model::{Database, Table};

/// Severity of a validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A structural problem that should block reliance on the schema.
    Error,
    /// A design concern that should be reviewed but may be intentional.
    Warning,
}

/// A validation rule that produced a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// A table has no primary key.
    MissingPrimaryKey,
    /// A foreign key references a table or column that does not exist.
    BrokenForeignKey,
    /// Two or more indexes on a table cover the same columns with the same
    /// uniqueness.
    DuplicateIndex,
    /// Foreign keys form a directed cycle between tables.
    CircularReference,
    /// A required metadata value is empty or invalid.
    InvalidMetadata,
}

/// One thing the validation engine noticed about the schema.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    /// The rule that flagged the finding.
    pub rule: Rule,
    /// How serious the finding is.
    pub severity: Severity,
    /// Schema the finding applies to, if any.
    pub schema: String,
    /// Table the finding applies to, if any.
    pub table: String,
    /// Columns the finding applies to, if any.
    pub columns: Vec<String>,
    /// Human-readable explanation.
    pub message: String,
}

/// The result of validating a schema.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationReport {
    /// Findings sorted deterministically.
    pub findings: Vec<Finding>,
    /// Summary counts.
    pub summary: Summary,
}

/// Counts derived from the validation pass.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    /// Number of tables inspected.
    pub tables: usize,
    /// Number of views inspected.
    pub views: usize,
    /// Total number of findings.
    pub finding_count: usize,
    /// Number of error findings.
    pub error_count: usize,
    /// Number of warning findings.
    pub warning_count: usize,
}

/// Run every validation rule against `database` and return the report.
pub fn validate(database: &Database) -> ValidationReport {
    let mut findings = Vec::new();

    missing_primary_keys(database, &mut findings);
    broken_foreign_keys(database, &mut findings);
    duplicate_indexes(database, &mut findings);
    circular_references(database, &mut findings);
    invalid_metadata(database, &mut findings);

    findings.sort_by(|a, b| {
        (&a.rule, &a.schema, &a.table, &a.columns, &a.message)
            .cmp(&(&b.rule, &b.schema, &b.table, &b.columns, &b.message))
    });

    let (error_count, warning_count) =
        findings.iter().fold((0, 0), |(errors, warnings), finding| {
            match finding.severity {
                Severity::Error => (errors + 1, warnings),
                Severity::Warning => (errors, warnings + 1),
            }
        });

    ValidationReport {
        summary: Summary {
            tables: database.tables.len(),
            views: database.views.len(),
            finding_count: findings.len(),
            error_count,
            warning_count,
        },
        findings,
    }
}

fn missing_primary_keys(database: &Database, findings: &mut Vec<Finding>) {
    for table in &database.tables {
        if table.columns.iter().any(|column| column.primary_key) {
            continue;
        }

        findings.push(Finding {
            rule: Rule::MissingPrimaryKey,
            severity: Severity::Warning,
            schema: table.schema.clone(),
            table: table.name.clone(),
            columns: Vec::new(),
            message: "table has no primary key".to_string(),
        });
    }
}

fn broken_foreign_keys(database: &Database, findings: &mut Vec<Finding>) {
    let table_key = |table: &Table| (table.schema.clone(), table.name.clone());
    let tables_by_key: std::collections::HashMap<(String, String), &Table> = database
        .tables
        .iter()
        .map(|table| (table_key(table), table))
        .collect();

    for table in &database.tables {
        for foreign_key in &table.foreign_keys {
            let target_key = (
                foreign_key.referenced_schema.clone(),
                foreign_key.referenced_table.clone(),
            );
            let Some(target) = tables_by_key.get(&target_key) else {
                findings.push(Finding {
                    rule: Rule::BrokenForeignKey,
                    severity: Severity::Error,
                    schema: table.schema.clone(),
                    table: table.name.clone(),
                    columns: foreign_key.columns.clone(),
                    message: format!(
                        "foreign key references missing table {}.{}",
                        foreign_key.referenced_schema, foreign_key.referenced_table
                    ),
                });
                continue;
            };

            if foreign_key.columns.len() != foreign_key.referenced_columns.len() {
                findings.push(Finding {
                    rule: Rule::BrokenForeignKey,
                    severity: Severity::Error,
                    schema: table.schema.clone(),
                    table: table.name.clone(),
                    columns: foreign_key.columns.clone(),
                    message: format!(
                        "foreign key column count ({}) does not match referenced column count ({})",
                        foreign_key.columns.len(),
                        foreign_key.referenced_columns.len()
                    ),
                });
                continue;
            }

            for column in &foreign_key.referenced_columns {
                if !target.columns.iter().any(|c| c.name == *column) {
                    findings.push(Finding {
                        rule: Rule::BrokenForeignKey,
                        severity: Severity::Error,
                        schema: table.schema.clone(),
                        table: table.name.clone(),
                        columns: foreign_key.columns.clone(),
                        message: format!(
                            "foreign key references column `{column}` which does not exist in {}.{}",
                            target.schema, target.name
                        ),
                    });
                }
            }
        }
    }
}

fn duplicate_indexes(database: &Database, findings: &mut Vec<Finding>) {
    for table in &database.tables {
        let mut indexes_by_signature: std::collections::HashMap<(bool, Vec<String>), Vec<String>> =
            std::collections::HashMap::new();

        for index in &table.indexes {
            indexes_by_signature
                .entry((index.unique, index.columns.clone()))
                .or_default()
                .push(index.name.clone());
        }

        for ((unique, columns), names) in indexes_by_signature {
            if names.len() < 2 {
                continue;
            }

            let mut sorted_names = names;
            sorted_names.sort();
            findings.push(Finding {
                rule: Rule::DuplicateIndex,
                severity: Severity::Warning,
                schema: table.schema.clone(),
                table: table.name.clone(),
                columns: columns.clone(),
                message: format!(
                    "duplicate {}index on columns ({}): {}",
                    if unique { "unique " } else { "" },
                    columns.join(", "),
                    sorted_names.join(", ")
                ),
            });
        }
    }
}

fn circular_references(database: &Database, findings: &mut Vec<Finding>) {
    let node_id = |table: &Table| (table.schema.clone(), table.name.clone());
    let node_ids: Vec<(String, String)> = database.tables.iter().map(node_id).collect();
    let index_by_id: std::collections::HashMap<(String, String), usize> = node_ids
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();

    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); node_ids.len()];
    for table in &database.tables {
        let Some(from) = index_by_id.get(&node_id(table)) else {
            continue;
        };
        for foreign_key in &table.foreign_keys {
            let to_id = (
                foreign_key.referenced_schema.clone(),
                foreign_key.referenced_table.clone(),
            );
            if let Some(to) = index_by_id.get(&to_id) {
                adjacency[*from].push(*to);
            }
        }
    }

    let sccs = tarjan_scc(&adjacency);

    for scc in sccs {
        let self_loop = scc.len() == 1 && adjacency[scc[0]].contains(&scc[0]);
        if scc.len() < 2 && !self_loop {
            continue;
        }

        let mut tables: Vec<String> = scc
            .iter()
            .map(|id| {
                let (schema, name) = &node_ids[*id];
                if database.metadata.engine == crate::model::Engine::Sqlserver {
                    format!("{schema}.{name}")
                } else {
                    name.clone()
                }
            })
            .collect();
        tables.sort();

        findings.push(Finding {
            rule: Rule::CircularReference,
            severity: Severity::Error,
            schema: String::new(),
            table: String::new(),
            columns: Vec::new(),
            message: format!("circular reference involving: {}", tables.join(", ")),
        });
    }
}

/// Tarjan's strongly connected components algorithm.
///
/// Returns components in reverse topological order. Each component is a list
/// of node indices. Cycles correspond to components of size > 1, or size 1 with
/// a self-edge.
fn tarjan_scc(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adjacency.len();
    let mut state = TarjanState {
        index: 0,
        stack: Vec::new(),
        on_stack: vec![false; n],
        indices: vec![None; n],
        lowlinks: vec![0; n],
        components: Vec::new(),
    };

    for node in 0..n {
        if state.indices[node].is_none() {
            tarjan_strongconnect(node, adjacency, &mut state);
        }
    }

    state.components
}

/// Mutable state carried through Tarjan's recursive traversal.
struct TarjanState {
    index: usize,
    stack: Vec<usize>,
    on_stack: Vec<bool>,
    indices: Vec<Option<usize>>,
    lowlinks: Vec<usize>,
    components: Vec<Vec<usize>>,
}

fn tarjan_strongconnect(node: usize, adjacency: &[Vec<usize>], state: &mut TarjanState) {
    state.indices[node] = Some(state.index);
    state.lowlinks[node] = state.index;
    state.index += 1;
    state.stack.push(node);
    state.on_stack[node] = true;

    for &neighbor in &adjacency[node] {
        if state.indices[neighbor].is_none() {
            tarjan_strongconnect(neighbor, adjacency, state);
            state.lowlinks[node] = state.lowlinks[node].min(state.lowlinks[neighbor]);
        } else if state.on_stack[neighbor] {
            state.lowlinks[node] = state.lowlinks[node].min(state.indices[neighbor].unwrap_or(0));
        }
    }

    if state.lowlinks[node] == state.indices[node].unwrap_or(0) {
        let mut component = Vec::new();
        loop {
            let member = state.stack.pop().expect("component contains at least node");
            state.on_stack[member] = false;
            component.push(member);
            if member == node {
                break;
            }
        }
        state.components.push(component);
    }
}

fn invalid_metadata(database: &Database, findings: &mut Vec<Finding>) {
    if database.metadata.database.trim().is_empty() {
        findings.push(Finding {
            rule: Rule::InvalidMetadata,
            severity: Severity::Error,
            schema: String::new(),
            table: String::new(),
            columns: Vec::new(),
            message: "database name is empty".to_string(),
        });
    }

    if database.metadata.engine_version.trim().is_empty() {
        findings.push(Finding {
            rule: Rule::InvalidMetadata,
            severity: Severity::Error,
            schema: String::new(),
            table: String::new(),
            columns: Vec::new(),
            message: "engine version is empty".to_string(),
        });
    }

    for table in &database.tables {
        if table.schema.trim().is_empty() {
            findings.push(Finding {
                rule: Rule::InvalidMetadata,
                severity: Severity::Error,
                schema: table.schema.clone(),
                table: table.name.clone(),
                columns: Vec::new(),
                message: "table schema is empty".to_string(),
            });
        }
        if table.name.trim().is_empty() {
            findings.push(Finding {
                rule: Rule::InvalidMetadata,
                severity: Severity::Error,
                schema: table.schema.clone(),
                table: table.name.clone(),
                columns: Vec::new(),
                message: "table name is empty".to_string(),
            });
        }

        for column in &table.columns {
            if column.name.trim().is_empty() {
                findings.push(Finding {
                    rule: Rule::InvalidMetadata,
                    severity: Severity::Error,
                    schema: table.schema.clone(),
                    table: table.name.clone(),
                    columns: vec![column.name.clone()],
                    message: "column name is empty".to_string(),
                });
            }
            if column.ordinal_position == 0 {
                findings.push(Finding {
                    rule: Rule::InvalidMetadata,
                    severity: Severity::Error,
                    schema: table.schema.clone(),
                    table: table.name.clone(),
                    columns: vec![column.name.clone()],
                    message: "column ordinal position is zero".to_string(),
                });
            }
        }
    }

    for view in &database.views {
        if view.schema.trim().is_empty() {
            findings.push(Finding {
                rule: Rule::InvalidMetadata,
                severity: Severity::Error,
                schema: view.schema.clone(),
                table: view.name.clone(),
                columns: Vec::new(),
                message: "view schema is empty".to_string(),
            });
        }
        if view.name.trim().is_empty() {
            findings.push(Finding {
                rule: Rule::InvalidMetadata,
                severity: Severity::Error,
                schema: view.schema.clone(),
                table: view.name.clone(),
                columns: Vec::new(),
                message: "view name is empty".to_string(),
            });
        }

        for column in &view.columns {
            if column.name.trim().is_empty() {
                findings.push(Finding {
                    rule: Rule::InvalidMetadata,
                    severity: Severity::Error,
                    schema: view.schema.clone(),
                    table: view.name.clone(),
                    columns: vec![column.name.clone()],
                    message: "view column name is empty".to_string(),
                });
            }
            if column.ordinal_position == 0 {
                findings.push(Finding {
                    rule: Rule::InvalidMetadata,
                    severity: Severity::Error,
                    schema: view.schema.clone(),
                    table: view.name.clone(),
                    columns: vec![column.name.clone()],
                    message: "view column ordinal position is zero".to_string(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Column, DatabaseMetadata, DocumentHeader, Engine, ForeignKey, Index, Table, View,
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
            },
            tables,
            views,
        }
    }

    fn finding(
        rule: Rule,
        severity: Severity,
        schema: &str,
        table: &str,
        columns: &[&str],
        message: &str,
    ) -> Finding {
        Finding {
            rule,
            severity,
            schema: schema.to_string(),
            table: table.to_string(),
            columns: columns.iter().map(|c| c.to_string()).collect(),
            message: message.to_string(),
        }
    }

    #[test]
    fn valid_schema_has_no_findings() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1), column("email", 2)];
        customers.columns[0].primary_key = true;
        customers.indexes.push(Index {
            name: "PRIMARY".to_string(),
            unique: true,
            columns: vec!["id".to_string()],
            index_type: "BTREE".to_string(),
        });

        let db = database(vec![customers], Vec::new());
        let report = validate(&db);

        assert_eq!(report.findings, []);
        assert_eq!(report.summary.finding_count, 0);
        assert_eq!(report.summary.error_count, 0);
        assert_eq!(report.summary.warning_count, 0);
    }

    #[test]
    fn missing_primary_key_is_reported() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("email", 1)];

        let db = database(vec![customers], Vec::new());
        let report = validate(&db);

        assert_eq!(
            report.findings,
            [finding(
                Rule::MissingPrimaryKey,
                Severity::Warning,
                "dbo",
                "customers",
                &[],
                "table has no primary key",
            )]
        );
    }

    #[test]
    fn a_table_with_a_primary_key_column_is_not_flagged() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1)];
        customers.columns[0].primary_key = true;

        let db = database(vec![customers], Vec::new());
        let report = validate(&db);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.rule == Rule::MissingPrimaryKey)
        );
    }

    #[test]
    fn broken_foreign_key_to_missing_table_is_reported() {
        let mut orders = table("dbo", "orders");
        orders.columns = vec![column("id", 1), column("customer_id", 2)];
        orders.columns[0].primary_key = true;
        orders.foreign_keys.push(ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        });

        let db = database(vec![orders], Vec::new());
        let report = validate(&db);

        assert!(report.findings.iter().any(|f| {
            f.rule == Rule::BrokenForeignKey
                && f.table == "orders"
                && f.message.contains("missing table dbo.customers")
        }));
    }

    #[test]
    fn broken_foreign_key_to_missing_column_is_reported() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1)];
        customers.columns[0].primary_key = true;

        let mut orders = table("dbo", "orders");
        orders.columns = vec![column("id", 1), column("customer_id", 2)];
        orders.columns[0].primary_key = true;
        orders.foreign_keys.push(ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["email".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        });

        let db = database(vec![customers, orders], Vec::new());
        let report = validate(&db);

        assert!(report.findings.iter().any(|f| {
            f.rule == Rule::BrokenForeignKey
                && f.table == "orders"
                && f.message.contains("email")
                && f.message.contains("does not exist")
        }));
    }

    #[test]
    fn foreign_key_with_mismatched_column_count_is_reported() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1), column("tenant_id", 2)];
        customers.columns[0].primary_key = true;

        let mut orders = table("dbo", "orders");
        orders.columns = vec![column("id", 1), column("customer_id", 2)];
        orders.columns[0].primary_key = true;
        orders.foreign_keys.push(ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["tenant_id".to_string(), "id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        });

        let db = database(vec![customers, orders], Vec::new());
        let report = validate(&db);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == Rule::BrokenForeignKey
                    && f.message.contains("column count")
                    && f.message.contains("does not match"))
        );
    }

    #[test]
    fn valid_foreign_key_is_not_reported() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1)];
        customers.columns[0].primary_key = true;

        let mut orders = table("dbo", "orders");
        orders.columns = vec![column("id", 1), column("customer_id", 2)];
        orders.columns[0].primary_key = true;
        orders.foreign_keys.push(ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        });

        let db = database(vec![customers, orders], Vec::new());
        let report = validate(&db);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.rule == Rule::BrokenForeignKey)
        );
    }

    #[test]
    fn duplicate_indexes_are_reported() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1), column("email", 2)];
        customers.columns[0].primary_key = true;
        customers.indexes = vec![
            Index {
                name: "idx_email".to_string(),
                unique: false,
                columns: vec!["email".to_string()],
                index_type: "BTREE".to_string(),
            },
            Index {
                name: "idx_email_2".to_string(),
                unique: false,
                columns: vec!["email".to_string()],
                index_type: "BTREE".to_string(),
            },
        ];

        let db = database(vec![customers], Vec::new());
        let report = validate(&db);

        assert!(report.findings.iter().any(|f| {
            f.rule == Rule::DuplicateIndex
                && f.table == "customers"
                && f.columns == vec!["email".to_string()]
                && f.message.contains("idx_email")
                && f.message.contains("idx_email_2")
        }));
    }

    #[test]
    fn indexes_with_same_columns_in_different_order_are_not_duplicates() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("a", 1), column("b", 2)];
        customers.columns[0].primary_key = true;
        customers.indexes = vec![
            Index {
                name: "idx_ab".to_string(),
                unique: false,
                columns: vec!["a".to_string(), "b".to_string()],
                index_type: "BTREE".to_string(),
            },
            Index {
                name: "idx_ba".to_string(),
                unique: false,
                columns: vec!["b".to_string(), "a".to_string()],
                index_type: "BTREE".to_string(),
            },
        ];

        let db = database(vec![customers], Vec::new());
        let report = validate(&db);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.rule == Rule::DuplicateIndex)
        );
    }

    #[test]
    fn unique_and_non_unique_indexes_with_same_columns_are_not_duplicates() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1), column("email", 2)];
        customers.columns[0].primary_key = true;
        customers.indexes = vec![
            Index {
                name: "idx_email".to_string(),
                unique: false,
                columns: vec!["email".to_string()],
                index_type: "BTREE".to_string(),
            },
            Index {
                name: "uq_email".to_string(),
                unique: true,
                columns: vec!["email".to_string()],
                index_type: "BTREE".to_string(),
            },
        ];

        let db = database(vec![customers], Vec::new());
        let report = validate(&db);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.rule == Rule::DuplicateIndex)
        );
    }

    #[test]
    fn circular_reference_between_two_tables_is_reported() {
        let mut a = table("dbo", "a");
        a.columns = vec![column("id", 1), column("b_id", 2)];
        a.columns[0].primary_key = true;
        a.foreign_keys.push(ForeignKey {
            name: "fk_a_b".to_string(),
            columns: vec!["b_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "b".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        });

        let mut b = table("dbo", "b");
        b.columns = vec![column("id", 1), column("a_id", 2)];
        b.columns[0].primary_key = true;
        b.foreign_keys.push(ForeignKey {
            name: "fk_b_a".to_string(),
            columns: vec!["a_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "a".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        });

        let db = database(vec![a, b], Vec::new());
        let report = validate(&db);

        assert!(report.findings.iter().any(|f| {
            f.rule == Rule::CircularReference && f.message.contains("a") && f.message.contains("b")
        }));
    }

    #[test]
    fn self_referencing_table_is_reported_as_circular() {
        let mut employees = table("dbo", "employees");
        employees.columns = vec![column("id", 1), column("manager_id", 2)];
        employees.columns[0].primary_key = true;
        employees.foreign_keys.push(ForeignKey {
            name: "fk_employees_manager".to_string(),
            columns: vec!["manager_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "employees".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        });

        let db = database(vec![employees], Vec::new());
        let report = validate(&db);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == Rule::CircularReference && f.message.contains("employees"))
        );
    }

    #[test]
    fn acyclic_references_are_not_flagged() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1)];
        customers.columns[0].primary_key = true;

        let mut orders = table("dbo", "orders");
        orders.columns = vec![column("id", 1), column("customer_id", 2)];
        orders.columns[0].primary_key = true;
        orders.foreign_keys.push(ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        });

        let db = database(vec![customers, orders], Vec::new());
        let report = validate(&db);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.rule == Rule::CircularReference)
        );
    }

    #[test]
    fn empty_table_name_is_reported() {
        let mut customers = table("dbo", "");
        customers.columns = vec![column("id", 1)];

        let db = database(vec![customers], Vec::new());
        let report = validate(&db);

        assert!(report.findings.iter().any(|f| {
            f.rule == Rule::InvalidMetadata
                && f.message == "table name is empty"
                && f.schema == "dbo"
                && f.table.is_empty()
        }));
    }

    #[test]
    fn zero_ordinal_position_is_reported() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 0)];

        let db = database(vec![customers], Vec::new());
        let report = validate(&db);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == Rule::InvalidMetadata
                    && f.table == "customers"
                    && f.message.contains("ordinal position is zero"))
        );
    }

    #[test]
    fn empty_database_name_is_reported() {
        let mut db = database(vec![table("dbo", "customers")], Vec::new());
        db.metadata.database = String::new();

        let report = validate(&db);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == Rule::InvalidMetadata && f.message == "database name is empty")
        );
    }

    #[test]
    fn validation_report_snapshot_is_stable() {
        let mut customers = table("dbo", "customers");
        customers.columns = vec![column("id", 1), column("email", 2)];
        customers.columns[0].primary_key = true;
        customers.indexes = vec![
            Index {
                name: "idx_email".to_string(),
                unique: false,
                columns: vec!["email".to_string()],
                index_type: "BTREE".to_string(),
            },
            Index {
                name: "idx_email_2".to_string(),
                unique: false,
                columns: vec!["email".to_string()],
                index_type: "BTREE".to_string(),
            },
        ];

        let mut orders = table("dbo", "orders");
        orders.columns = vec![column("id", 1), column("customer_id", 2)];
        orders.columns[0].primary_key = true;
        orders.foreign_keys.push(ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "missing".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        });

        let mut tags = table("dbo", "tags");
        tags.columns = vec![column("id", 1), column("name", 2)];
        // No primary key, no indexes.

        let db = database(vec![customers, orders, tags], Vec::new());
        let report = validate(&db);

        insta::assert_json_snapshot!(report);
    }
}
