//! MySQL and MariaDB catalog introspection through `INFORMATION_SCHEMA`.

use std::collections::{BTreeMap, HashMap};

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row};

use crate::Result;
use crate::config::ConnectionConfig;
use crate::database::DatabaseError;
use crate::model::{Column, Database, DatabaseMetadata, Engine, ForeignKey, Index, Table, View};

/// Read the schema of the database named in `config`.
pub async fn inspect(config: &ConnectionConfig) -> Result<Database> {
    let mut conn = connect(config).await?;

    let version = version(&mut conn).await?;
    let engine = engine_from_version(&version);
    let (default_charset, default_collation) =
        database_collation(config.database(), &mut conn).await?;

    let table_names = tables(config.database(), &mut conn).await?;
    let all_columns = columns(config.database(), &mut conn).await?;
    let indexes = indexes(config.database(), &mut conn).await?;
    let foreign_keys = foreign_keys(config.database(), &mut conn).await?;
    let view_names = views(config.database(), &mut conn).await?;

    let mut tables: Vec<Table> = table_names
        .into_iter()
        .map(|name| {
            let cols = all_columns
                .get(&name)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|c| to_column(c, &indexes))
                .collect();
            Table {
                schema: config.database().to_string(),
                name: name.clone(),
                engine: None,
                charset: None,
                collation: None,
                comment: None,
                columns: cols,
                indexes: indexes_for_table(&name, &indexes),
                foreign_keys: foreign_keys_for_table(&name, &foreign_keys),
                analysis: None,
            }
        })
        .collect();

    for table in &mut tables {
        if let Some(info) = table_info(&table.name, config.database(), &mut conn).await? {
            table.engine = info.engine;
            table.charset = info.charset;
            table.collation = info.collation;
            table.comment = info.comment;
        }
    }

    let views: Vec<View> = view_names
        .into_iter()
        .map(|name| View {
            schema: config.database().to_string(),
            name: name.clone(),
            columns: all_columns
                .get(&name)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|c| to_column(c, &indexes))
                .collect(),
        })
        .collect();

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let mut database = Database {
        header: crate::model::DocumentHeader::new(Database::FORMAT, generated_at),
        metadata: DatabaseMetadata {
            database: config.database().to_string(),
            engine,
            engine_version: version,
            default_charset,
            default_collation,
        },
        tables,
        views,
    };
    database.sort();

    Ok(database)
}

pub(crate) async fn connect(config: &ConnectionConfig) -> Result<Conn, DatabaseError> {
    let opts: mysql_async::Opts = OptsBuilder::default()
        .user(config.user())
        .pass(config.password())
        .ip_or_hostname(config.host())
        .tcp_port(config.port())
        .db_name(Some(config.database()))
        .into();

    Conn::new(opts).await.map_err(DatabaseError::connection)
}

async fn version(conn: &mut Conn) -> Result<String, DatabaseError> {
    let row: Option<Row> = conn
        .query_first("SELECT VERSION() AS version")
        .await
        .map_err(DatabaseError::connection)?;
    row.and_then(|row| row.get::<String, _>("version"))
        .ok_or_else(|| DatabaseError::Catalog("VERSION() returned no row".to_string()))
}

fn engine_from_version(version: &str) -> Engine {
    if version.to_ascii_lowercase().contains("mariadb") {
        Engine::Mariadb
    } else {
        Engine::Mysql
    }
}

struct TableInfo {
    engine: Option<String>,
    charset: Option<String>,
    collation: Option<String>,
    comment: Option<String>,
}

async fn table_info(
    table: &str,
    schema: &str,
    conn: &mut Conn,
) -> Result<Option<TableInfo>, DatabaseError> {
    let query = r#"
        SELECT ENGINE, TABLE_COLLATION, TABLE_COMMENT
        FROM information_schema.TABLES
        WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
    "#;
    let row: Option<Row> = conn
        .exec_first(query, (schema, table))
        .await
        .map_err(DatabaseError::connection)?;

    Ok(row.map(|row| TableInfo {
        engine: row.get("ENGINE"),
        charset: None,
        collation: row.get("TABLE_COLLATION"),
        comment: row.get("TABLE_COMMENT"),
    }))
}

async fn database_collation(
    schema: &str,
    conn: &mut Conn,
) -> Result<(Option<String>, Option<String>), DatabaseError> {
    let query = r#"
        SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME
        FROM information_schema.SCHEMATA
        WHERE SCHEMA_NAME = ?
    "#;
    let row: Option<Row> = conn
        .exec_first(query, (schema,))
        .await
        .map_err(DatabaseError::connection)?;

    Ok(row.map_or((None, None), |row| {
        (
            row.get("DEFAULT_CHARACTER_SET_NAME"),
            row.get("DEFAULT_COLLATION_NAME"),
        )
    }))
}

async fn tables(schema: &str, conn: &mut Conn) -> Result<Vec<String>, DatabaseError> {
    let query = r#"
        SELECT TABLE_NAME
        FROM information_schema.TABLES
        WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'
        ORDER BY TABLE_NAME
    "#;
    let rows: Vec<Row> = conn
        .exec(query, (schema,))
        .await
        .map_err(DatabaseError::connection)?;

    Ok(rows
        .into_iter()
        .filter_map(|row| row.get::<String, _>("TABLE_NAME"))
        .collect())
}

async fn views(schema: &str, conn: &mut Conn) -> Result<Vec<String>, DatabaseError> {
    let query = r#"
        SELECT TABLE_NAME
        FROM information_schema.VIEWS
        WHERE TABLE_SCHEMA = ?
        ORDER BY TABLE_NAME
    "#;
    let rows: Vec<Row> = conn
        .exec(query, (schema,))
        .await
        .map_err(DatabaseError::connection)?;

    Ok(rows
        .into_iter()
        .filter_map(|row| row.get::<String, _>("TABLE_NAME"))
        .collect())
}

#[derive(Debug, Clone)]
struct RawColumn {
    table_name: String,
    name: String,
    ordinal_position: u32,
    data_type: String,
    full_type: String,
    nullable: bool,
    default: Option<String>,
    extra: String,
    comment: Option<String>,
    generation_expression: Option<String>,
}

async fn columns(
    schema: &str,
    conn: &mut Conn,
) -> Result<BTreeMap<String, Vec<RawColumn>>, DatabaseError> {
    let query = r#"
        SELECT
            TABLE_NAME,
            COLUMN_NAME,
            ORDINAL_POSITION,
            DATA_TYPE,
            COLUMN_TYPE,
            IS_NULLABLE,
            COLUMN_DEFAULT,
            EXTRA,
            COLUMN_COMMENT,
            GENERATION_EXPRESSION
        FROM information_schema.COLUMNS
        WHERE TABLE_SCHEMA = ?
        ORDER BY TABLE_NAME, ORDINAL_POSITION
    "#;
    let rows: Vec<Row> = conn
        .exec(query, (schema,))
        .await
        .map_err(DatabaseError::connection)?;

    let mut by_table: BTreeMap<String, Vec<RawColumn>> = BTreeMap::new();
    for row in rows {
        let table_name: String = row
            .get("TABLE_NAME")
            .ok_or_else(|| DatabaseError::Catalog("TABLE_NAME missing".to_string()))?;
        let column = RawColumn {
            table_name: table_name.clone(),
            name: row
                .get("COLUMN_NAME")
                .ok_or_else(|| DatabaseError::Catalog("COLUMN_NAME missing".to_string()))?,
            ordinal_position: row
                .get("ORDINAL_POSITION")
                .ok_or_else(|| DatabaseError::Catalog("ORDINAL_POSITION missing".to_string()))?,
            data_type: row
                .get("DATA_TYPE")
                .ok_or_else(|| DatabaseError::Catalog("DATA_TYPE missing".to_string()))?,
            full_type: row
                .get("COLUMN_TYPE")
                .ok_or_else(|| DatabaseError::Catalog("COLUMN_TYPE missing".to_string()))?,
            nullable: row
                .get::<String, _>("IS_NULLABLE")
                .map(|v| v == "YES")
                .unwrap_or(false),
            default: row.get("COLUMN_DEFAULT"),
            extra: row.get::<String, _>("EXTRA").unwrap_or_default(),
            comment: row.get("COLUMN_COMMENT"),
            generation_expression: row.get("GENERATION_EXPRESSION"),
        };
        by_table.entry(table_name).or_default().push(column);
    }

    Ok(by_table)
}

fn to_column(raw: RawColumn, indexes: &BTreeMap<String, Vec<RawIndex>>) -> Column {
    let extra = raw.extra.to_ascii_lowercase();
    let generated = raw
        .generation_expression
        .as_ref()
        .is_some_and(|s| !s.is_empty())
        || extra.contains("generated");
    let auto_increment = extra.contains("auto_increment");
    let primary = is_primary_key(&raw.table_name, &raw.name, indexes);
    let unique = is_unique_column(&raw.table_name, &raw.name, indexes);

    Column {
        name: raw.name,
        ordinal_position: raw.ordinal_position,
        data_type: raw.data_type,
        full_type: raw.full_type,
        nullable: raw.nullable,
        default: raw.default,
        auto_increment,
        primary_key: primary,
        unique: unique || primary,
        comment: raw.comment,
        generated,
        expression: raw.generation_expression.filter(|s| !s.is_empty()),
    }
}

#[derive(Debug, Clone)]
struct RawIndex {
    name: String,
    unique: bool,
    column: String,
    sequence: u32,
    index_type: String,
}

async fn indexes(
    schema: &str,
    conn: &mut Conn,
) -> Result<BTreeMap<String, Vec<RawIndex>>, DatabaseError> {
    let query = r#"
        SELECT
            TABLE_NAME,
            INDEX_NAME,
            NON_UNIQUE,
            COLUMN_NAME,
            SEQ_IN_INDEX,
            INDEX_TYPE
        FROM information_schema.STATISTICS
        WHERE TABLE_SCHEMA = ?
        ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX
    "#;
    let rows: Vec<Row> = conn
        .exec(query, (schema,))
        .await
        .map_err(DatabaseError::connection)?;

    let mut by_table: BTreeMap<String, Vec<RawIndex>> = BTreeMap::new();
    for row in rows {
        let table_name: String = row
            .get("TABLE_NAME")
            .ok_or_else(|| DatabaseError::Catalog("TABLE_NAME missing".to_string()))?;
        let index = RawIndex {
            name: row
                .get("INDEX_NAME")
                .ok_or_else(|| DatabaseError::Catalog("INDEX_NAME missing".to_string()))?,
            unique: row
                .get::<u8, _>("NON_UNIQUE")
                .map(|v| v == 0)
                .unwrap_or(false),
            column: row
                .get("COLUMN_NAME")
                .ok_or_else(|| DatabaseError::Catalog("COLUMN_NAME missing".to_string()))?,
            sequence: row
                .get("SEQ_IN_INDEX")
                .ok_or_else(|| DatabaseError::Catalog("SEQ_IN_INDEX missing".to_string()))?,
            index_type: row.get("INDEX_TYPE").unwrap_or_else(|| "BTREE".to_string()),
        };
        by_table.entry(table_name).or_default().push(index);
    }

    Ok(by_table)
}

fn indexes_for_table(table: &str, indexes: &BTreeMap<String, Vec<RawIndex>>) -> Vec<Index> {
    let empty = Vec::new();
    let raw = indexes.get(table).unwrap_or(&empty);

    let mut by_name: BTreeMap<String, Vec<&RawIndex>> = BTreeMap::new();
    for index in raw {
        by_name.entry(index.name.clone()).or_default().push(index);
    }

    by_name
        .into_iter()
        .map(|(name, mut parts)| {
            parts.sort_by_key(|i| i.sequence);
            let unique = parts[0].unique || name == "PRIMARY";
            let index_type = parts[0].index_type.clone();
            Index {
                name,
                unique,
                columns: parts.into_iter().map(|i| i.column.clone()).collect(),
                index_type,
            }
        })
        .collect()
}

fn is_primary_key(table: &str, column: &str, indexes: &BTreeMap<String, Vec<RawIndex>>) -> bool {
    indexes
        .get(table)
        .unwrap_or(&Vec::new())
        .iter()
        .any(|i| i.name == "PRIMARY" && i.column == column)
}

fn is_unique_column(table: &str, column: &str, indexes: &BTreeMap<String, Vec<RawIndex>>) -> bool {
    let empty = Vec::new();
    let raw = indexes.get(table).unwrap_or(&empty);

    let mut by_name: BTreeMap<String, Vec<&RawIndex>> = BTreeMap::new();
    for index in raw {
        by_name.entry(index.name.clone()).or_default().push(index);
    }

    by_name
        .values()
        .any(|parts| parts.len() == 1 && parts[0].column == column && parts[0].unique)
}

#[derive(Debug, Clone)]
struct RawForeignKey {
    name: String,
    column: String,
    sequence: u32,
    referenced_schema: String,
    referenced_table: String,
    referenced_column: String,
    on_update: String,
    on_delete: String,
}

async fn foreign_keys(
    schema: &str,
    conn: &mut Conn,
) -> Result<BTreeMap<String, Vec<RawForeignKey>>, DatabaseError> {
    let query = r#"
        SELECT
            rc.TABLE_NAME,
            rc.CONSTRAINT_NAME,
            kcu.COLUMN_NAME,
            kcu.ORDINAL_POSITION,
            kcu.REFERENCED_TABLE_SCHEMA,
            kcu.REFERENCED_TABLE_NAME,
            kcu.REFERENCED_COLUMN_NAME,
            rc.UPDATE_RULE,
            rc.DELETE_RULE
        FROM information_schema.REFERENTIAL_CONSTRAINTS rc
        JOIN information_schema.KEY_COLUMN_USAGE kcu
            ON rc.CONSTRAINT_SCHEMA = kcu.CONSTRAINT_SCHEMA
            AND rc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME
        WHERE rc.CONSTRAINT_SCHEMA = ?
          AND kcu.REFERENCED_TABLE_SCHEMA IS NOT NULL
        ORDER BY rc.TABLE_NAME, rc.CONSTRAINT_NAME, kcu.ORDINAL_POSITION
    "#;
    let rows: Vec<Row> = conn
        .exec(query, (schema,))
        .await
        .map_err(DatabaseError::connection)?;

    let mut by_table: BTreeMap<String, Vec<RawForeignKey>> = BTreeMap::new();
    for row in rows {
        let table_name: String = row
            .get("TABLE_NAME")
            .ok_or_else(|| DatabaseError::Catalog("TABLE_NAME missing".to_string()))?;
        let fk = RawForeignKey {
            name: row
                .get("CONSTRAINT_NAME")
                .ok_or_else(|| DatabaseError::Catalog("CONSTRAINT_NAME missing".to_string()))?,
            column: row
                .get("COLUMN_NAME")
                .ok_or_else(|| DatabaseError::Catalog("COLUMN_NAME missing".to_string()))?,
            sequence: row
                .get("ORDINAL_POSITION")
                .ok_or_else(|| DatabaseError::Catalog("ORDINAL_POSITION missing".to_string()))?,
            referenced_schema: row.get("REFERENCED_TABLE_SCHEMA").ok_or_else(|| {
                DatabaseError::Catalog("REFERENCED_TABLE_SCHEMA missing".to_string())
            })?,
            referenced_table: row.get("REFERENCED_TABLE_NAME").ok_or_else(|| {
                DatabaseError::Catalog("REFERENCED_TABLE_NAME missing".to_string())
            })?,
            referenced_column: row.get("REFERENCED_COLUMN_NAME").ok_or_else(|| {
                DatabaseError::Catalog("REFERENCED_COLUMN_NAME missing".to_string())
            })?,
            on_update: row
                .get("UPDATE_RULE")
                .unwrap_or_else(|| "NO ACTION".to_string()),
            on_delete: row
                .get("DELETE_RULE")
                .unwrap_or_else(|| "NO ACTION".to_string()),
        };
        by_table.entry(table_name).or_default().push(fk);
    }

    Ok(by_table)
}

fn foreign_keys_for_table(
    table: &str,
    foreign_keys: &BTreeMap<String, Vec<RawForeignKey>>,
) -> Vec<ForeignKey> {
    let empty = Vec::new();
    let raw = foreign_keys.get(table).unwrap_or(&empty);

    let mut by_name: HashMap<String, Vec<&RawForeignKey>> = HashMap::new();
    for fk in raw {
        by_name.entry(fk.name.clone()).or_default().push(fk);
    }

    let mut result: Vec<ForeignKey> = by_name
        .into_iter()
        .map(|(name, mut parts)| {
            parts.sort_by_key(|p| p.sequence);
            ForeignKey {
                name,
                columns: parts.iter().map(|p| p.column.clone()).collect(),
                referenced_schema: parts[0].referenced_schema.clone(),
                referenced_table: parts[0].referenced_table.clone(),
                referenced_columns: parts.iter().map(|p| p.referenced_column.clone()).collect(),
                on_update: parts[0].on_update.clone(),
                on_delete: parts[0].on_delete.clone(),
            }
        })
        .collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}
