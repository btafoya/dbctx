//! Schema statistics derived from the canonical model.
//!
//! Statistics never query a database; they count the facts already in the
//! [`Database`] model. `dbctx stats` prints them as a short human-readable
//! summary.

use std::fmt;

use serde::Serialize;

use crate::model::Database;

/// Counts of schema objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Statistics {
    /// Number of tables.
    pub tables: usize,
    /// Number of views.
    pub views: usize,
    /// Number of columns across tables and views.
    pub columns: usize,
    /// Number of indexes across tables.
    pub indexes: usize,
    /// Number of foreign key constraints across tables.
    pub foreign_keys: usize,
}

impl From<&Database> for Statistics {
    fn from(database: &Database) -> Self {
        let tables = database.tables.len();
        let views = database.views.len();
        let columns: usize = database
            .tables
            .iter()
            .map(|t| t.columns.len())
            .sum::<usize>()
            + database
                .views
                .iter()
                .map(|v| v.columns.len())
                .sum::<usize>();
        let indexes: usize = database.tables.iter().map(|t| t.indexes.len()).sum();
        let foreign_keys: usize = database.tables.iter().map(|t| t.foreign_keys.len()).sum();

        Self {
            tables,
            views,
            columns,
            indexes,
            foreign_keys,
        }
    }
}

impl fmt::Display for Statistics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Longest documented label is "Foreign Keys:". Pad every label to
        // that width so the counts line up deterministically.
        const LABEL_WIDTH: usize = 14;
        writeln!(f, "{:<LABEL_WIDTH$}{:>5}", "Tables:", self.tables)?;
        writeln!(f, "{:<LABEL_WIDTH$}{:>5}", "Views:", self.views)?;
        writeln!(f, "{:<LABEL_WIDTH$}{:>5}", "Columns:", self.columns)?;
        writeln!(f, "{:<LABEL_WIDTH$}{:>5}", "Indexes:", self.indexes)?;
        write!(
            f,
            "{:<LABEL_WIDTH$}{:>5}",
            "Foreign Keys:", self.foreign_keys
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, Database, DatabaseMetadata, DocumentHeader, Engine, Table, View};

    fn column(name: &str, ordinal: u32) -> Column {
        Column {
            name: name.to_string(),
            ordinal_position: ordinal,
            data_type: "int".to_string(),
            full_type: "int".to_string(),
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

    #[test]
    fn empty_database_has_zero_counts() {
        let db = Database {
            header: DocumentHeader::new(Database::FORMAT, "2026-01-01T00:00:00Z"),
            metadata: DatabaseMetadata {
                database: "empty".to_string(),
                engine: Engine::Mysql,
                engine_version: "8.0".to_string(),
                default_charset: None,
                default_collation: None,
            },
            tables: Vec::new(),
            views: Vec::new(),
        };

        let stats = Statistics::from(&db);

        assert_eq!(stats.tables, 0);
        assert_eq!(stats.views, 0);
        assert_eq!(stats.columns, 0);
        assert_eq!(stats.indexes, 0);
        assert_eq!(stats.foreign_keys, 0);
    }

    #[test]
    fn counts_aggregate_tables_views_columns_indexes_and_foreign_keys() {
        let db = Database {
            header: DocumentHeader::new(Database::FORMAT, "2026-01-01T00:00:00Z"),
            metadata: DatabaseMetadata {
                database: "shop".to_string(),
                engine: Engine::Mysql,
                engine_version: "8.0".to_string(),
                default_charset: None,
                default_collation: None,
            },
            tables: vec![
                Table {
                    schema: "shop".to_string(),
                    name: "customers".to_string(),
                    engine: None,
                    charset: None,
                    collation: None,
                    comment: None,
                    columns: vec![column("id", 1), column("email", 2)],
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    analysis: None,
                    ai: None,
                },
                Table {
                    schema: "shop".to_string(),
                    name: "orders".to_string(),
                    engine: None,
                    charset: None,
                    collation: None,
                    comment: None,
                    columns: vec![
                        column("id", 1),
                        column("customer_id", 2),
                        column("total", 3),
                    ],
                    indexes: vec![crate::model::Index {
                        name: "idx_total".to_string(),
                        unique: false,
                        columns: vec!["total".to_string()],
                        index_type: "BTREE".to_string(),
                    }],
                    foreign_keys: vec![crate::model::ForeignKey {
                        name: "fk_orders_customer".to_string(),
                        columns: vec!["customer_id".to_string()],
                        referenced_schema: "shop".to_string(),
                        referenced_table: "customers".to_string(),
                        referenced_columns: vec!["id".to_string()],
                        on_update: "NO ACTION".to_string(),
                        on_delete: "NO ACTION".to_string(),
                    }],
                    analysis: None,
                    ai: None,
                },
            ],
            views: vec![View {
                schema: "shop".to_string(),
                name: "recent_orders".to_string(),
                columns: vec![column("id", 1), column("total", 2)],
            }],
        };

        let stats = Statistics::from(&db);

        assert_eq!(stats.tables, 2);
        assert_eq!(stats.views, 1);
        assert_eq!(stats.columns, 7); // 2 + 3 + 2
        assert_eq!(stats.indexes, 1);
        assert_eq!(stats.foreign_keys, 1);
    }

    #[test]
    fn display_matches_cli_example() {
        let stats = Statistics {
            tables: 42,
            views: 3,
            columns: 615,
            indexes: 108,
            foreign_keys: 67,
        };

        let output = stats.to_string();
        let expected = "Tables:          42
Views:            3
Columns:        615
Indexes:        108
Foreign Keys:    67";

        assert_eq!(output, expected);
    }
}
