//! PostgreSQL catalog introspection.
//!
//! Table and view enumeration come from `information_schema`, per `SPEC.md`
//! §7. Columns, indexes and foreign keys are read from `pg_catalog` instead:
//! `format_type` gives an accurate bare/full type split that
//! `information_schema.columns` cannot, and `pg_index`/`pg_constraint` give
//! ordered, multi-column index and foreign key definitions in one query each,
//! which `information_schema` only offers by joining several views together
//! for the same result.

use std::collections::BTreeMap;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Pool, Postgres, Row};

use crate::Result;
use crate::config::ConnectionConfig;
use crate::database::DatabaseError;
use crate::model::{Column, Database, DatabaseMetadata, Engine, ForeignKey, Index, Table, View};

/// Read the schema of the database named in `config`.
pub async fn inspect(config: &ConnectionConfig) -> Result<Database> {
    let pool = connect(config).await?;

    let (database_name, version) = database_identity(&pool).await?;
    let (default_charset, default_collation) = database_collation(&pool, &database_name).await?;

    let table_names = tables(&pool).await?;
    let view_names = views(&pool).await?;
    let table_info = table_info(&pool).await?;
    let all_columns = columns(&pool).await?;
    let indexes = indexes(&pool).await?;
    let foreign_keys = foreign_keys(&pool).await?;

    let tables: Vec<Table> = table_names
        .into_iter()
        .map(|(schema, name)| {
            let key = (schema.clone(), name.clone());
            let info = table_info.get(&key);
            let mut attributes = BTreeMap::new();
            if let Some(info) = info {
                if let Some(access_method) = &info.access_method {
                    attributes.insert(
                        "access_method".to_string(),
                        serde_json::json!(access_method),
                    );
                }
                if let Some(tablespace) = &info.tablespace {
                    attributes.insert("tablespace".to_string(), serde_json::json!(tablespace));
                }
                attributes.insert(
                    "row_security".to_string(),
                    serde_json::json!(info.row_security),
                );
            }

            Table {
                schema: schema.clone(),
                name: name.clone(),
                engine: None,
                charset: None,
                collation: None,
                comment: info.and_then(|info| info.comment.clone()),
                columns: columns_for(&all_columns, &key, &indexes),
                indexes: indexes_for_table(&indexes, &key),
                foreign_keys: foreign_keys_for_table(&foreign_keys, &key),
                analysis: None,
                ai: None,
                attributes,
            }
        })
        .collect();

    let views: Vec<View> = view_names
        .into_iter()
        .map(|(schema, name)| {
            let key = (schema.clone(), name.clone());
            View {
                schema,
                name,
                columns: columns_for(&all_columns, &key, &indexes),
                attributes: BTreeMap::new(),
            }
        })
        .collect();

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let mut database = Database {
        header: crate::model::DocumentHeader::new(Database::FORMAT, generated_at),
        metadata: DatabaseMetadata {
            database: database_name,
            engine: Engine::Postgres,
            engine_version: version,
            default_charset,
            default_collation,
            attributes: BTreeMap::new(),
        },
        tables,
        views,
        attributes: BTreeMap::new(),
    };
    database.sort();

    Ok(database)
}

pub(crate) async fn connect(config: &ConnectionConfig) -> Result<Pool<Postgres>, DatabaseError> {
    let mut options = PgConnectOptions::new()
        .host(config.host())
        .port(config.port())
        .database(config.database())
        .username(config.user().unwrap_or("postgres"));
    if let Some(password) = config.password() {
        options = options.password(password);
    }

    PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(DatabaseError::connection)
}

async fn database_identity(pool: &Pool<Postgres>) -> Result<(String, String), DatabaseError> {
    let row = sqlx::query("SELECT current_database() AS database, version() AS version")
        .fetch_one(pool)
        .await
        .map_err(DatabaseError::connection)?;

    Ok((
        row.try_get("database").map_err(catalog_error)?,
        row.try_get("version").map_err(catalog_error)?,
    ))
}

async fn database_collation(
    pool: &Pool<Postgres>,
    database: &str,
) -> Result<(Option<String>, Option<String>), DatabaseError> {
    let row = sqlx::query(
        "SELECT pg_encoding_to_char(encoding) AS encoding, datcollate \
         FROM pg_database WHERE datname = $1",
    )
    .bind(database)
    .fetch_optional(pool)
    .await
    .map_err(DatabaseError::connection)?;

    Ok(match row {
        Some(row) => (
            row.try_get("encoding").map_err(catalog_error)?,
            row.try_get("datcollate").map_err(catalog_error)?,
        ),
        None => (None, None),
    })
}

async fn tables(pool: &Pool<Postgres>) -> Result<Vec<(String, String)>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT table_schema, table_name \
         FROM information_schema.tables \
         WHERE table_type = 'BASE TABLE' \
           AND table_schema NOT IN ('pg_catalog', 'information_schema') \
           AND table_schema NOT LIKE 'pg\\_toast%' \
         ORDER BY table_schema, table_name",
    )
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::connection)?;

    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get("table_schema").map_err(catalog_error)?,
                row.try_get("table_name").map_err(catalog_error)?,
            ))
        })
        .collect()
}

async fn views(pool: &Pool<Postgres>) -> Result<Vec<(String, String)>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT table_schema, table_name \
         FROM information_schema.views \
         WHERE table_schema NOT IN ('pg_catalog', 'information_schema') \
         ORDER BY table_schema, table_name",
    )
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::connection)?;

    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get("table_schema").map_err(catalog_error)?,
                row.try_get("table_name").map_err(catalog_error)?,
            ))
        })
        .collect()
}

struct RawTableInfo {
    comment: Option<String>,
    access_method: Option<String>,
    tablespace: Option<String>,
    row_security: bool,
}

async fn table_info(
    pool: &Pool<Postgres>,
) -> Result<BTreeMap<(String, String), RawTableInfo>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT \
             n.nspname AS table_schema, \
             c.relname AS table_name, \
             obj_description(c.oid, 'pg_class') AS comment, \
             am.amname AS access_method, \
             ts.spcname AS tablespace, \
             c.relrowsecurity AS row_security \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_am am ON am.oid = c.relam \
         LEFT JOIN pg_tablespace ts ON ts.oid = c.reltablespace \
         WHERE c.relkind IN ('r', 'p') \
           AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
           AND n.nspname NOT LIKE 'pg\\_toast%'",
    )
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::connection)?;

    let mut info = BTreeMap::new();
    for row in rows {
        let schema: String = row.try_get("table_schema").map_err(catalog_error)?;
        let name: String = row.try_get("table_name").map_err(catalog_error)?;
        info.insert(
            (schema, name),
            RawTableInfo {
                comment: row.try_get("comment").map_err(catalog_error)?,
                access_method: row.try_get("access_method").map_err(catalog_error)?,
                tablespace: row.try_get("tablespace").map_err(catalog_error)?,
                row_security: row.try_get("row_security").map_err(catalog_error)?,
            },
        );
    }
    Ok(info)
}

#[derive(Debug, Clone)]
struct RawColumn {
    name: String,
    ordinal_position: u32,
    data_type: String,
    full_type: String,
    nullable: bool,
    default: Option<String>,
    is_identity: bool,
    identity_generation: Option<String>,
    is_generated: bool,
    generation_expression: Option<String>,
    collation: Option<String>,
    comment: Option<String>,
}

async fn columns(
    pool: &Pool<Postgres>,
) -> Result<BTreeMap<(String, String), Vec<RawColumn>>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT \
             n.nspname AS table_schema, \
             cl.relname AS table_name, \
             a.attname AS column_name, \
             a.attnum AS ordinal_position, \
             format_type(a.atttypid, NULL) AS data_type, \
             format_type(a.atttypid, a.atttypmod) AS full_type, \
             NOT a.attnotnull AS nullable, \
             CASE WHEN a.attgenerated = '' THEN pg_get_expr(d.adbin, d.adrelid) END AS column_default, \
             a.attidentity <> '' AS is_identity, \
             CASE a.attidentity WHEN 'a' THEN 'ALWAYS' WHEN 'd' THEN 'BY DEFAULT' END AS identity_generation, \
             a.attgenerated <> '' AS is_generated, \
             CASE WHEN a.attgenerated <> '' THEN pg_get_expr(d.adbin, d.adrelid) END AS generation_expression, \
             co.collname AS collation, \
             col_description(cl.oid, a.attnum) AS comment \
         FROM pg_attribute a \
         JOIN pg_class cl ON cl.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = cl.relnamespace \
         LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         LEFT JOIN pg_collation co ON co.oid = a.attcollation \
         WHERE a.attnum > 0 AND NOT a.attisdropped \
           AND cl.relkind IN ('r', 'p', 'v', 'm') \
           AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
           AND n.nspname NOT LIKE 'pg\\_toast%' \
         ORDER BY n.nspname, cl.relname, a.attnum",
    )
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::connection)?;

    let mut by_table: BTreeMap<(String, String), Vec<RawColumn>> = BTreeMap::new();
    for row in rows {
        let schema: String = row.try_get("table_schema").map_err(catalog_error)?;
        let table: String = row.try_get("table_name").map_err(catalog_error)?;
        let ordinal: i16 = row.try_get("ordinal_position").map_err(catalog_error)?;
        let column = RawColumn {
            name: row.try_get("column_name").map_err(catalog_error)?,
            ordinal_position: ordinal as u32,
            data_type: row.try_get("data_type").map_err(catalog_error)?,
            full_type: row.try_get("full_type").map_err(catalog_error)?,
            nullable: row.try_get("nullable").map_err(catalog_error)?,
            default: row.try_get("column_default").map_err(catalog_error)?,
            is_identity: row.try_get("is_identity").map_err(catalog_error)?,
            identity_generation: row.try_get("identity_generation").map_err(catalog_error)?,
            is_generated: row.try_get("is_generated").map_err(catalog_error)?,
            generation_expression: row
                .try_get("generation_expression")
                .map_err(catalog_error)?,
            collation: row.try_get("collation").map_err(catalog_error)?,
            comment: row.try_get("comment").map_err(catalog_error)?,
        };
        by_table.entry((schema, table)).or_default().push(column);
    }
    Ok(by_table)
}

fn columns_for(
    all_columns: &BTreeMap<(String, String), Vec<RawColumn>>,
    key: &(String, String),
    indexes: &BTreeMap<(String, String), Vec<RawIndex>>,
) -> Vec<Column> {
    all_columns
        .get(key)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|raw| to_column(raw, key, indexes))
        .collect()
}

fn to_column(
    raw: RawColumn,
    key: &(String, String),
    indexes: &BTreeMap<(String, String), Vec<RawIndex>>,
) -> Column {
    let auto_increment = raw.is_identity
        || raw
            .default
            .as_deref()
            .is_some_and(|expr| expr.starts_with("nextval("));
    let primary = is_primary_key(key, &raw.name, indexes);
    let unique = is_unique_column(key, &raw.name, indexes);

    let mut attributes = BTreeMap::new();
    if let Some(identity_generation) = &raw.identity_generation {
        attributes.insert(
            "identity_generation".to_string(),
            serde_json::json!(identity_generation),
        );
    }
    if let Some(collation) = &raw.collation {
        attributes.insert("collation".to_string(), serde_json::json!(collation));
    }

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
        generated: raw.is_generated,
        expression: raw.generation_expression,
        attributes,
    }
}

#[derive(Debug, Clone)]
struct RawIndex {
    name: String,
    unique: bool,
    primary: bool,
    index_type: String,
    column: String,
    sequence: u32,
}

async fn indexes(
    pool: &Pool<Postgres>,
) -> Result<BTreeMap<(String, String), Vec<RawIndex>>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT \
             n.nspname AS table_schema, \
             t.relname AS table_name, \
             ic.relname AS index_name, \
             i.indisunique AS is_unique, \
             i.indisprimary AS is_primary, \
             am.amname AS index_type, \
             a.attname AS column_name, \
             k.ord AS ordinal \
         FROM pg_index i \
         JOIN pg_class t ON t.oid = i.indrelid \
         JOIN pg_class ic ON ic.oid = i.indexrelid \
         JOIN pg_namespace n ON n.oid = t.relnamespace \
         JOIN pg_am am ON am.oid = ic.relam \
         JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum \
         WHERE n.nspname NOT IN ('pg_catalog', 'information_schema') \
           AND n.nspname NOT LIKE 'pg\\_toast%' \
         ORDER BY n.nspname, t.relname, ic.relname, k.ord",
    )
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::connection)?;

    let mut by_table: BTreeMap<(String, String), Vec<RawIndex>> = BTreeMap::new();
    for row in rows {
        let schema: String = row.try_get("table_schema").map_err(catalog_error)?;
        let table: String = row.try_get("table_name").map_err(catalog_error)?;
        let ordinal: i64 = row.try_get("ordinal").map_err(catalog_error)?;
        let index = RawIndex {
            name: row.try_get("index_name").map_err(catalog_error)?,
            unique: row.try_get("is_unique").map_err(catalog_error)?,
            primary: row.try_get("is_primary").map_err(catalog_error)?,
            index_type: row.try_get("index_type").map_err(catalog_error)?,
            column: row.try_get("column_name").map_err(catalog_error)?,
            sequence: ordinal as u32,
        };
        by_table.entry((schema, table)).or_default().push(index);
    }
    Ok(by_table)
}

fn indexes_for_table(
    indexes: &BTreeMap<(String, String), Vec<RawIndex>>,
    key: &(String, String),
) -> Vec<Index> {
    let empty = Vec::new();
    let raw = indexes.get(key).unwrap_or(&empty);

    let mut by_name: BTreeMap<String, Vec<&RawIndex>> = BTreeMap::new();
    for index in raw {
        by_name.entry(index.name.clone()).or_default().push(index);
    }

    by_name
        .into_iter()
        .map(|(name, mut parts)| {
            parts.sort_by_key(|i| i.sequence);
            Index {
                unique: parts[0].unique,
                columns: parts.iter().map(|i| i.column.clone()).collect(),
                index_type: parts[0].index_type.clone(),
                name,
                attributes: BTreeMap::new(),
            }
        })
        .collect()
}

fn is_primary_key(
    key: &(String, String),
    column: &str,
    indexes: &BTreeMap<(String, String), Vec<RawIndex>>,
) -> bool {
    indexes
        .get(key)
        .into_iter()
        .flatten()
        .any(|i| i.primary && i.column == column)
}

fn is_unique_column(
    key: &(String, String),
    column: &str,
    indexes: &BTreeMap<(String, String), Vec<RawIndex>>,
) -> bool {
    let empty = Vec::new();
    let raw = indexes.get(key).unwrap_or(&empty);

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
    pool: &Pool<Postgres>,
) -> Result<BTreeMap<(String, String), Vec<RawForeignKey>>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT \
             n.nspname AS table_schema, \
             t.relname AS table_name, \
             c.conname AS name, \
             a.attname AS column_name, \
             k.ord AS ordinal, \
             fn.nspname AS referenced_schema, \
             ft.relname AS referenced_table, \
             fa.attname AS referenced_column, \
             CASE c.confupdtype \
                 WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE' \
                 WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT' END AS on_update, \
             CASE c.confdeltype \
                 WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE' \
                 WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT' END AS on_delete \
         FROM pg_constraint c \
         JOIN pg_class t ON t.oid = c.conrelid \
         JOIN pg_namespace n ON n.oid = t.relnamespace \
         JOIN pg_class ft ON ft.oid = c.confrelid \
         JOIN pg_namespace fn ON fn.oid = ft.relnamespace \
         JOIN LATERAL unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum \
         JOIN LATERAL unnest(c.confkey) WITH ORDINALITY AS fk(attnum, ord) ON fk.ord = k.ord \
         JOIN pg_attribute fa ON fa.attrelid = ft.oid AND fa.attnum = fk.attnum \
         WHERE c.contype = 'f' \
         ORDER BY n.nspname, t.relname, c.conname, k.ord",
    )
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::connection)?;

    let mut by_table: BTreeMap<(String, String), Vec<RawForeignKey>> = BTreeMap::new();
    for row in rows {
        let schema: String = row.try_get("table_schema").map_err(catalog_error)?;
        let table: String = row.try_get("table_name").map_err(catalog_error)?;
        let ordinal: i64 = row.try_get("ordinal").map_err(catalog_error)?;
        let fk = RawForeignKey {
            name: row.try_get("name").map_err(catalog_error)?,
            column: row.try_get("column_name").map_err(catalog_error)?,
            sequence: ordinal as u32,
            referenced_schema: row.try_get("referenced_schema").map_err(catalog_error)?,
            referenced_table: row.try_get("referenced_table").map_err(catalog_error)?,
            referenced_column: row.try_get("referenced_column").map_err(catalog_error)?,
            on_update: row.try_get("on_update").map_err(catalog_error)?,
            on_delete: row.try_get("on_delete").map_err(catalog_error)?,
        };
        by_table.entry((schema, table)).or_default().push(fk);
    }
    Ok(by_table)
}

fn foreign_keys_for_table(
    foreign_keys: &BTreeMap<(String, String), Vec<RawForeignKey>>,
    key: &(String, String),
) -> Vec<ForeignKey> {
    let empty = Vec::new();
    let raw = foreign_keys.get(key).unwrap_or(&empty);

    let mut by_name: BTreeMap<String, Vec<&RawForeignKey>> = BTreeMap::new();
    for fk in raw {
        by_name.entry(fk.name.clone()).or_default().push(fk);
    }

    by_name
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
                attributes: BTreeMap::new(),
            }
        })
        .collect()
}

fn catalog_error(source: sqlx::Error) -> DatabaseError {
    DatabaseError::Catalog(source.to_string())
}
