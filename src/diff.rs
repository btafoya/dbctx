//! Compare two exported schema models.
//!
//! Diff never queries a database; it reads JSON schema documents and compares
//! the canonical [`Database`] models they contain. `CLI.md` defines the public
//! command shape and the `10` exit code used when differences are found.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Column, Database, ForeignKey, Index, Table, View};

/// Why a diff could not be performed.
#[derive(Debug, Error)]
pub enum DiffError {
    /// A file could not be read.
    #[error("could not read `{path}`: {source}")]
    Io {
        /// File that could not be read.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// A file could not be parsed as a schema document.
    #[error("could not parse `{path}` as a schema document: {source}")]
    Parse {
        /// File that could not be parsed.
        path: PathBuf,
        /// Underlying parse error.
        source: serde_json::Error,
    },
}

impl DiffError {
    fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    fn parse(path: impl AsRef<Path>, source: serde_json::Error) -> Self {
        Self::Parse {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
}

/// The result of comparing an old schema document to a new one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffReport {
    /// Counts of detected changes.
    pub summary: DiffSummary,
    /// Table-level changes.
    pub tables: TableChanges,
    /// View-level changes.
    pub views: ViewChanges,
}

/// Counts of changes at the table and view level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Tables present only in the new schema.
    pub added_tables: usize,
    /// Tables present only in the old schema.
    pub removed_tables: usize,
    /// Tables present in both schemas but with different columns, indexes, or keys.
    pub modified_tables: usize,
    /// Views present only in the new schema.
    pub added_views: usize,
    /// Views present only in the old schema.
    pub removed_views: usize,
    /// Views present in both schemas but with different columns.
    pub modified_views: usize,
    /// Sum of all top-level change entries.
    pub total_changes: usize,
}

/// Table-level change categories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableChanges {
    /// Tables added in the new schema.
    pub added: Vec<Table>,
    /// Tables removed in the new schema.
    pub removed: Vec<Table>,
    /// Tables that exist in both schemas but differ.
    pub modified: Vec<TableDiff>,
}

/// View-level change categories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewChanges {
    /// Views added in the new schema.
    pub added: Vec<View>,
    /// Views removed in the new schema.
    pub removed: Vec<View>,
    /// Views that exist in both schemas but differ.
    pub modified: Vec<ViewDiff>,
}

/// The differences inside a single table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableDiff {
    /// Object namespace of the table.
    pub schema: String,
    /// Table name.
    pub name: String,
    /// Columns added in the new schema.
    pub added_columns: Vec<Column>,
    /// Columns removed in the new schema.
    pub removed_columns: Vec<Column>,
    /// Columns that exist in both schemas but differ.
    pub changed_columns: Vec<ColumnChange>,
    /// Indexes added in the new schema.
    pub added_indexes: Vec<Index>,
    /// Indexes removed in the new schema.
    pub removed_indexes: Vec<Index>,
    /// Indexes that exist in both schemas but differ.
    pub changed_indexes: Vec<IndexChange>,
    /// Foreign keys added in the new schema.
    pub added_foreign_keys: Vec<ForeignKey>,
    /// Foreign keys removed in the new schema.
    pub removed_foreign_keys: Vec<ForeignKey>,
    /// Foreign keys that exist in both schemas but differ.
    pub changed_foreign_keys: Vec<ForeignKeyChange>,
}

/// The differences inside a single view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewDiff {
    /// Object namespace of the view.
    pub schema: String,
    /// View name.
    pub name: String,
    /// Columns added in the new schema.
    pub added_columns: Vec<Column>,
    /// Columns removed in the new schema.
    pub removed_columns: Vec<Column>,
    /// Columns that exist in both schemas but differ.
    pub changed_columns: Vec<ColumnChange>,
}

/// A column whose definition changed between two schemas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnChange {
    /// Column definition in the old schema.
    pub old: Column,
    /// Column definition in the new schema.
    pub new: Column,
}

/// An index whose definition changed between two schemas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexChange {
    /// Index definition in the old schema.
    pub old: Index,
    /// Index definition in the new schema.
    pub new: Index,
}

/// A foreign key whose definition changed between two schemas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKeyChange {
    /// Foreign key definition in the old schema.
    pub old: ForeignKey,
    /// Foreign key definition in the new schema.
    pub new: ForeignKey,
}

/// Read two schema documents from disk and compare them.
pub fn diff_files(old_path: &Path, new_path: &Path) -> Result<DiffReport, DiffError> {
    let old_json = std::fs::read_to_string(old_path).map_err(|e| DiffError::io(old_path, e))?;
    let new_json = std::fs::read_to_string(new_path).map_err(|e| DiffError::io(new_path, e))?;

    let old =
        serde_json::from_str::<Database>(&old_json).map_err(|e| DiffError::parse(old_path, e))?;
    let new =
        serde_json::from_str::<Database>(&new_json).map_err(|e| DiffError::parse(new_path, e))?;

    Ok(diff(&old, &new))
}

/// Compare two schema models and return a deterministic report.
pub fn diff(old: &Database, new: &Database) -> DiffReport {
    let table_changes = diff_tables(&old.tables, &new.tables);
    let view_changes = diff_views(&old.views, &new.views);

    let summary = DiffSummary {
        added_tables: table_changes.added.len(),
        removed_tables: table_changes.removed.len(),
        modified_tables: table_changes.modified.len(),
        added_views: view_changes.added.len(),
        removed_views: view_changes.removed.len(),
        modified_views: view_changes.modified.len(),
        total_changes: table_changes.added.len()
            + table_changes.removed.len()
            + table_changes.modified.len()
            + view_changes.added.len()
            + view_changes.removed.len()
            + view_changes.modified.len(),
    };

    DiffReport {
        summary,
        tables: table_changes,
        views: view_changes,
    }
}

fn diff_tables(old: &[Table], new: &[Table]) -> TableChanges {
    let old_by_key: BTreeMap<(String, String), &Table> = old
        .iter()
        .map(|table| ((table.schema.clone(), table.name.clone()), table))
        .collect();
    let new_by_key: BTreeMap<(String, String), &Table> = new
        .iter()
        .map(|table| ((table.schema.clone(), table.name.clone()), table))
        .collect();

    let mut added: Vec<Table> = Vec::new();
    let mut removed: Vec<Table> = Vec::new();
    let mut modified: Vec<TableDiff> = Vec::new();

    for (key, table) in &new_by_key {
        match old_by_key.get(key) {
            Some(old_table) => {
                if let Some(table_diff) = diff_table(old_table, table) {
                    modified.push(table_diff);
                }
            }
            None => added.push((*table).clone()),
        }
    }

    for (key, table) in &old_by_key {
        if !new_by_key.contains_key(key) {
            removed.push((*table).clone());
        }
    }

    added.sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
    removed.sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
    modified.sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));

    TableChanges {
        added,
        removed,
        modified,
    }
}

fn diff_table(old: &Table, new: &Table) -> Option<TableDiff> {
    let column_changes = diff_columns(&old.columns, &new.columns);
    let index_changes = diff_indexes(&old.indexes, &new.indexes);
    let foreign_key_changes = diff_foreign_keys(&old.foreign_keys, &new.foreign_keys);

    if column_changes.is_empty() && index_changes.is_empty() && foreign_key_changes.is_empty() {
        return None;
    }

    Some(TableDiff {
        schema: new.schema.clone(),
        name: new.name.clone(),
        added_columns: column_changes.added,
        removed_columns: column_changes.removed,
        changed_columns: column_changes.changed,
        added_indexes: index_changes.added,
        removed_indexes: index_changes.removed,
        changed_indexes: index_changes.changed,
        added_foreign_keys: foreign_key_changes.added,
        removed_foreign_keys: foreign_key_changes.removed,
        changed_foreign_keys: foreign_key_changes.changed,
    })
}

fn diff_views(old: &[View], new: &[View]) -> ViewChanges {
    let old_by_key: BTreeMap<(String, String), &View> = old
        .iter()
        .map(|view| ((view.schema.clone(), view.name.clone()), view))
        .collect();
    let new_by_key: BTreeMap<(String, String), &View> = new
        .iter()
        .map(|view| ((view.schema.clone(), view.name.clone()), view))
        .collect();

    let mut added: Vec<View> = Vec::new();
    let mut removed: Vec<View> = Vec::new();
    let mut modified: Vec<ViewDiff> = Vec::new();

    for (key, view) in &new_by_key {
        match old_by_key.get(key) {
            Some(old_view) => {
                if let Some(view_diff) = diff_view(old_view, view) {
                    modified.push(view_diff);
                }
            }
            None => added.push((*view).clone()),
        }
    }

    for (key, view) in &old_by_key {
        if !new_by_key.contains_key(key) {
            removed.push((*view).clone());
        }
    }

    added.sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
    removed.sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
    modified.sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));

    ViewChanges {
        added,
        removed,
        modified,
    }
}

fn diff_view(old: &View, new: &View) -> Option<ViewDiff> {
    let column_changes = diff_columns(&old.columns, &new.columns);

    if column_changes.is_empty() {
        return None;
    }

    Some(ViewDiff {
        schema: new.schema.clone(),
        name: new.name.clone(),
        added_columns: column_changes.added,
        removed_columns: column_changes.removed,
        changed_columns: column_changes.changed,
    })
}

#[derive(Default)]
struct ItemChanges<T, C> {
    added: Vec<T>,
    removed: Vec<T>,
    changed: Vec<C>,
}

impl<T, C> ItemChanges<T, C> {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

fn diff_columns(old: &[Column], new: &[Column]) -> ItemChanges<Column, ColumnChange> {
    let old_by_name: BTreeMap<String, &Column> = old
        .iter()
        .map(|column| (column.name.clone(), column))
        .collect();
    let new_by_name: BTreeMap<String, &Column> = new
        .iter()
        .map(|column| (column.name.clone(), column))
        .collect();

    let mut added: Vec<Column> = Vec::new();
    let mut removed: Vec<Column> = Vec::new();
    let mut changed: Vec<ColumnChange> = Vec::new();

    for (name, column) in &new_by_name {
        match old_by_name.get(name) {
            Some(old_column) => {
                if *old_column != *column {
                    changed.push(ColumnChange {
                        old: (*old_column).clone(),
                        new: (*column).clone(),
                    });
                }
            }
            None => added.push((*column).clone()),
        }
    }

    for (name, column) in &old_by_name {
        if !new_by_name.contains_key(name) {
            removed.push((*column).clone());
        }
    }

    ItemChanges {
        added,
        removed,
        changed,
    }
}

fn diff_indexes(old: &[Index], new: &[Index]) -> ItemChanges<Index, IndexChange> {
    let old_by_name: BTreeMap<String, &Index> = old
        .iter()
        .map(|index| (index.name.clone(), index))
        .collect();
    let new_by_name: BTreeMap<String, &Index> = new
        .iter()
        .map(|index| (index.name.clone(), index))
        .collect();

    let mut added: Vec<Index> = Vec::new();
    let mut removed: Vec<Index> = Vec::new();
    let mut changed: Vec<IndexChange> = Vec::new();

    for (name, index) in &new_by_name {
        match old_by_name.get(name) {
            Some(old_index) => {
                if *old_index != *index {
                    changed.push(IndexChange {
                        old: (*old_index).clone(),
                        new: (*index).clone(),
                    });
                }
            }
            None => added.push((*index).clone()),
        }
    }

    for (name, index) in &old_by_name {
        if !new_by_name.contains_key(name) {
            removed.push((*index).clone());
        }
    }

    ItemChanges {
        added,
        removed,
        changed,
    }
}

fn diff_foreign_keys(
    old: &[ForeignKey],
    new: &[ForeignKey],
) -> ItemChanges<ForeignKey, ForeignKeyChange> {
    let old_by_name: BTreeMap<String, &ForeignKey> =
        old.iter().map(|fk| (fk.name.clone(), fk)).collect();
    let new_by_name: BTreeMap<String, &ForeignKey> =
        new.iter().map(|fk| (fk.name.clone(), fk)).collect();

    let mut added: Vec<ForeignKey> = Vec::new();
    let mut removed: Vec<ForeignKey> = Vec::new();
    let mut changed: Vec<ForeignKeyChange> = Vec::new();

    for (name, fk) in &new_by_name {
        match old_by_name.get(name) {
            Some(old_fk) => {
                if *old_fk != *fk {
                    changed.push(ForeignKeyChange {
                        old: (*old_fk).clone(),
                        new: (*fk).clone(),
                    });
                }
            }
            None => added.push((*fk).clone()),
        }
    }

    for (name, fk) in &old_by_name {
        if !new_by_name.contains_key(name) {
            removed.push((*fk).clone());
        }
    }

    ItemChanges {
        added,
        removed,
        changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Column, DatabaseMetadata, DocumentHeader, Engine, ForeignKey, Index, Table, View,
    };
    use std::io::Write;

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
            analysis: None,
            ai: None,
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

    #[test]
    fn identical_schemas_produce_an_empty_report() {
        let db = database(vec![table("dbo", "orders")], Vec::new());
        let report = diff(&db, &db);

        assert_eq!(report.summary.total_changes, 0);
        assert!(report.tables.added.is_empty());
        assert!(report.tables.removed.is_empty());
        assert!(report.tables.modified.is_empty());
        assert!(report.views.added.is_empty());
        assert!(report.views.removed.is_empty());
        assert!(report.views.modified.is_empty());
    }

    #[test]
    fn added_and_removed_tables_are_detected() {
        let old = database(vec![table("dbo", "customers")], Vec::new());
        let new = database(vec![table("dbo", "orders")], Vec::new());

        let report = diff(&old, &new);

        assert_eq!(report.tables.added.len(), 1);
        assert_eq!(report.tables.added[0].name, "orders");
        assert_eq!(report.tables.removed.len(), 1);
        assert_eq!(report.tables.removed[0].name, "customers");
        assert_eq!(report.summary.added_tables, 1);
        assert_eq!(report.summary.removed_tables, 1);
        assert_eq!(report.summary.total_changes, 2);
    }

    #[test]
    fn added_and_removed_columns_are_detected() {
        let mut old_table = table("dbo", "orders");
        old_table.columns = vec![column("id", 1)];
        let mut new_table = table("dbo", "orders");
        new_table.columns = vec![column("id", 1), column("total", 2)];

        let report = diff(
            &database(vec![old_table], Vec::new()),
            &database(vec![new_table], Vec::new()),
        );

        assert_eq!(report.tables.modified.len(), 1);
        let table_diff = &report.tables.modified[0];
        assert_eq!(table_diff.added_columns.len(), 1);
        assert_eq!(table_diff.added_columns[0].name, "total");
        assert!(table_diff.removed_columns.is_empty());
        assert!(table_diff.changed_columns.is_empty());
    }

    #[test]
    fn changed_columns_are_detected() {
        let mut old_table = table("dbo", "orders");
        old_table.columns = vec![column("total", 1)];
        let mut new_table = table("dbo", "orders");
        new_table.columns = vec![Column {
            full_type: "decimal(10,2)".to_string(),
            ..column("total", 1)
        }];

        let report = diff(
            &database(vec![old_table], Vec::new()),
            &database(vec![new_table], Vec::new()),
        );

        let table_diff = &report.tables.modified[0];
        assert_eq!(table_diff.changed_columns.len(), 1);
        assert_eq!(table_diff.changed_columns[0].old.full_type, "int(11)");
        assert_eq!(table_diff.changed_columns[0].new.full_type, "decimal(10,2)");
    }

    #[test]
    fn added_and_removed_indexes_are_detected() {
        let mut old_table = table("dbo", "orders");
        old_table.indexes = vec![Index {
            name: "PRIMARY".to_string(),
            unique: true,
            columns: vec!["id".to_string()],
            index_type: "BTREE".to_string(),
        }];
        let mut new_table = table("dbo", "orders");
        new_table.indexes = vec![
            Index {
                name: "PRIMARY".to_string(),
                unique: true,
                columns: vec!["id".to_string()],
                index_type: "BTREE".to_string(),
            },
            Index {
                name: "idx_total".to_string(),
                unique: false,
                columns: vec!["total".to_string()],
                index_type: "BTREE".to_string(),
            },
        ];

        let report = diff(
            &database(vec![old_table], Vec::new()),
            &database(vec![new_table], Vec::new()),
        );

        let table_diff = &report.tables.modified[0];
        assert_eq!(table_diff.added_indexes.len(), 1);
        assert_eq!(table_diff.added_indexes[0].name, "idx_total");
        assert!(table_diff.removed_indexes.is_empty());
        assert!(table_diff.changed_indexes.is_empty());
    }

    #[test]
    fn changed_indexes_are_detected() {
        let mut old_table = table("dbo", "orders");
        old_table.indexes = vec![Index {
            name: "idx_total".to_string(),
            unique: false,
            columns: vec!["total".to_string()],
            index_type: "BTREE".to_string(),
        }];
        let mut new_table = table("dbo", "orders");
        new_table.indexes = vec![Index {
            name: "idx_total".to_string(),
            unique: true,
            columns: vec!["total".to_string()],
            index_type: "BTREE".to_string(),
        }];

        let report = diff(
            &database(vec![old_table], Vec::new()),
            &database(vec![new_table], Vec::new()),
        );

        let table_diff = &report.tables.modified[0];
        assert_eq!(table_diff.changed_indexes.len(), 1);
        assert!(!table_diff.changed_indexes[0].old.unique);
        assert!(table_diff.changed_indexes[0].new.unique);
    }

    #[test]
    fn added_and_removed_foreign_keys_are_detected() {
        let mut old_table = table("dbo", "orders");
        old_table.columns = vec![column("id", 1), column("customer_id", 2)];
        let mut new_table = table("dbo", "orders");
        new_table.columns = vec![column("id", 1), column("customer_id", 2)];
        new_table.foreign_keys = vec![ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        }];

        let report = diff(
            &database(vec![old_table], Vec::new()),
            &database(vec![new_table], Vec::new()),
        );

        let table_diff = &report.tables.modified[0];
        assert_eq!(table_diff.added_foreign_keys.len(), 1);
        assert_eq!(table_diff.added_foreign_keys[0].name, "fk_orders_customer");
        assert!(table_diff.removed_foreign_keys.is_empty());
        assert!(table_diff.changed_foreign_keys.is_empty());
    }

    #[test]
    fn changed_foreign_keys_are_detected() {
        let mut old_table = table("dbo", "orders");
        old_table.columns = vec![column("id", 1), column("customer_id", 2)];
        old_table.foreign_keys = vec![ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        }];
        let mut new_table = table("dbo", "orders");
        new_table.columns = vec![column("id", 1), column("customer_id", 2)];
        new_table.foreign_keys = vec![ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "CASCADE".to_string(),
            on_delete: "CASCADE".to_string(),
        }];

        let report = diff(
            &database(vec![old_table], Vec::new()),
            &database(vec![new_table], Vec::new()),
        );

        let table_diff = &report.tables.modified[0];
        assert_eq!(table_diff.changed_foreign_keys.len(), 1);
        assert_eq!(
            table_diff.changed_foreign_keys[0].old.on_update,
            "NO ACTION"
        );
        assert_eq!(table_diff.changed_foreign_keys[0].new.on_update, "CASCADE");
    }

    #[test]
    fn added_and_removed_views_are_detected() {
        let old = database(
            Vec::new(),
            vec![View {
                schema: "dbo".to_string(),
                name: "old_view".to_string(),
                columns: Vec::new(),
            }],
        );
        let new = database(
            Vec::new(),
            vec![View {
                schema: "dbo".to_string(),
                name: "new_view".to_string(),
                columns: Vec::new(),
            }],
        );

        let report = diff(&old, &new);

        assert_eq!(report.views.added.len(), 1);
        assert_eq!(report.views.added[0].name, "new_view");
        assert_eq!(report.views.removed.len(), 1);
        assert_eq!(report.views.removed[0].name, "old_view");
        assert_eq!(report.summary.added_views, 1);
        assert_eq!(report.summary.removed_views, 1);
    }

    #[test]
    fn changed_view_columns_are_detected() {
        let old = database(
            Vec::new(),
            vec![View {
                schema: "dbo".to_string(),
                name: "recent_orders".to_string(),
                columns: vec![column("id", 1)],
            }],
        );
        let new = database(
            Vec::new(),
            vec![View {
                schema: "dbo".to_string(),
                name: "recent_orders".to_string(),
                columns: vec![column("id", 1), column("total", 2)],
            }],
        );

        let report = diff(&old, &new);

        assert_eq!(report.views.modified.len(), 1);
        assert_eq!(report.views.modified[0].added_columns.len(), 1);
        assert_eq!(report.views.modified[0].added_columns[0].name, "total");
    }

    #[test]
    fn diff_files_loads_and_compares_documents() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("old.json");
        let new_path = dir.path().join("new.json");

        let mut old = database(vec![table("dbo", "customers")], Vec::new());
        let mut new = database(vec![table("dbo", "orders")], Vec::new());
        old.sort();
        new.sort();

        std::fs::File::create(&old_path)
            .unwrap()
            .write_all(serde_json::to_string_pretty(&old).unwrap().as_bytes())
            .unwrap();
        std::fs::File::create(&new_path)
            .unwrap()
            .write_all(serde_json::to_string_pretty(&new).unwrap().as_bytes())
            .unwrap();

        let report = diff_files(&old_path, &new_path).unwrap();

        assert_eq!(report.tables.added.len(), 1);
        assert_eq!(report.tables.removed.len(), 1);
    }

    #[test]
    fn diff_files_reports_a_parse_error_with_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();

        let error = diff_files(&path, &path).unwrap_err();
        assert!(format!("{error}").contains("bad.json"));
    }

    #[test]
    fn diff_report_snapshot_is_stable() {
        let mut old_table = table("dbo", "orders");
        old_table.columns = vec![column("id", 1), column("customer_id", 2)];
        old_table.indexes = vec![Index {
            name: "PRIMARY".to_string(),
            unique: true,
            columns: vec!["id".to_string()],
            index_type: "BTREE".to_string(),
        }];

        let mut new_table = table("dbo", "orders");
        new_table.columns = vec![
            column("id", 1),
            column("customer_id", 2),
            column("total", 3),
        ];
        new_table.indexes = vec![
            Index {
                name: "PRIMARY".to_string(),
                unique: true,
                columns: vec!["id".to_string()],
                index_type: "BTREE".to_string(),
            },
            Index {
                name: "idx_total".to_string(),
                unique: false,
                columns: vec!["total".to_string()],
                index_type: "BTREE".to_string(),
            },
        ];
        new_table.foreign_keys = vec![ForeignKey {
            name: "fk_orders_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_schema: "dbo".to_string(),
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        }];

        let old = database(vec![old_table], Vec::new());
        let new = database(
            vec![new_table],
            vec![View {
                schema: "dbo".to_string(),
                name: "recent_orders".to_string(),
                columns: vec![column("id", 1)],
            }],
        );

        let report = diff(&old, &new);
        insta::assert_json_snapshot!(report);
    }
}
