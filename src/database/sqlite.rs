//! SQLite catalog introspection, including attached databases.
//!
//! `SPEC.md` §7 wants `INFORMATION_SCHEMA` first and native catalog views only
//! for facts it does not expose; SQLite has neither `INFORMATION_SCHEMA` nor a
//! non-textual catalog, so every fact here comes from `sqlite_master` and the
//! `PRAGMA` family. `WITHOUT ROWID` and `STRICT` exist only as keywords in a
//! table's declared SQL text, so they are the one place this module reads
//! that text rather than a structured catalog value; every other fact comes
//! from a `PRAGMA`.
//!
//! [`ConnectionConfig::databases`] lists one or more files: the first is
//! attached as `main` implicitly by connecting to it, and the rest are
//! attached in order as `attach1`, `attach2`, and so on, matching the
//! `--database` order `SPEC.md` §4.2 defines.

use std::collections::BTreeMap;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Row, Sqlite};

use crate::Result;
use crate::config::ConnectionConfig;
use crate::database::DatabaseError;
use crate::model::{Column, Database, DatabaseMetadata, Engine, ForeignKey, Index, Table, View};

/// Read the schema of every database named in `config`, main first and then
/// each attached database in order.
pub async fn inspect(config: &ConnectionConfig) -> Result<Database> {
    let pool = connect(config).await?;
    let version = sqlite_version(&pool).await?;

    let mut tables = Vec::new();
    let mut views = Vec::new();

    for (index, _) in config.databases().iter().enumerate() {
        let schema = schema_name(index);

        for (name, sql) in sqlite_master_entries(&pool, &schema, "table").await? {
            let raw_columns = table_columns(&pool, &schema, &name).await?;
            let raw_indexes = table_indexes(&pool, &schema, &name).await?;
            let raw_foreign_keys = table_foreign_keys(&pool, &schema, &name).await?;

            let mut attributes = BTreeMap::new();
            let upper = sql.to_ascii_uppercase();
            if upper.contains("WITHOUT ROWID") {
                attributes.insert("without_rowid".to_string(), serde_json::json!(true));
            }
            if upper.contains("STRICT") {
                attributes.insert("strict".to_string(), serde_json::json!(true));
            }

            tables.push(Table {
                schema: schema.clone(),
                name: name.clone(),
                engine: None,
                charset: None,
                collation: None,
                comment: None,
                columns: raw_columns
                    .into_iter()
                    .map(|raw| to_column(raw, &raw_indexes))
                    .collect(),
                indexes: indexes_from_raw(&raw_indexes),
                foreign_keys: foreign_keys_from_raw(&schema, &raw_foreign_keys),
                analysis: None,
                ai: None,
                attributes,
            });
        }

        for (name, _sql) in sqlite_master_entries(&pool, &schema, "view").await? {
            let raw_columns = table_columns(&pool, &schema, &name).await?;
            views.push(View {
                schema: schema.clone(),
                name,
                columns: raw_columns
                    .into_iter()
                    .map(|raw| to_column(raw, &[]))
                    .collect(),
                attributes: BTreeMap::new(),
            });
        }
    }

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let mut database = Database {
        header: crate::model::DocumentHeader::new(Database::FORMAT, generated_at),
        metadata: DatabaseMetadata {
            database: config.database().to_string(),
            engine: Engine::Sqlite,
            engine_version: version,
            default_charset: None,
            default_collation: None,
            attributes: BTreeMap::new(),
        },
        tables,
        views,
        attributes: BTreeMap::new(),
    };
    database.sort();

    Ok(database)
}

/// The schema name [`inspect`] assigns to the database at `index` in
/// [`ConnectionConfig::databases`]: `main` for the first, `attach1`,
/// `attach2`, ... for the rest, in the order they were configured.
fn schema_name(index: usize) -> String {
    if index == 0 {
        "main".to_string()
    } else {
        format!("attach{index}")
    }
}

pub(crate) async fn connect(config: &ConnectionConfig) -> Result<Pool<Sqlite>, DatabaseError> {
    let databases = config.databases();
    let options = SqliteConnectOptions::new()
        .filename(&databases[0])
        .create_if_missing(false);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(DatabaseError::connection)?;

    for (index, path) in databases.iter().enumerate().skip(1) {
        let schema = schema_name(index);
        sqlx::query(&format!("ATTACH DATABASE ? AS {schema}"))
            .bind(path)
            .execute(&pool)
            .await
            .map_err(DatabaseError::connection)?;
    }

    Ok(pool)
}

async fn sqlite_version(pool: &Pool<Sqlite>) -> Result<String, DatabaseError> {
    let row = sqlx::query("SELECT sqlite_version() AS version")
        .fetch_one(pool)
        .await
        .map_err(DatabaseError::connection)?;
    row.try_get("version").map_err(catalog_error)
}

/// `name` and `sql` for every `sqlite_master` entry of `kind` (`table` or
/// `view`) in `schema`, ordered by name.
async fn sqlite_master_entries(
    pool: &Pool<Sqlite>,
    schema: &str,
    kind: &str,
) -> Result<Vec<(String, String)>, DatabaseError> {
    let query = format!(
        "SELECT name, sql FROM {schema}.sqlite_master \
         WHERE type = ? AND name NOT LIKE 'sqlite_%' AND sql IS NOT NULL \
         ORDER BY name"
    );
    let rows = sqlx::query(&query)
        .bind(kind)
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::connection)?;

    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get("name").map_err(catalog_error)?,
                row.try_get("sql").map_err(catalog_error)?,
            ))
        })
        .collect()
}

#[derive(Debug, Clone)]
struct RawColumn {
    name: String,
    ordinal_position: u32,
    decl_type: String,
    notnull: bool,
    dflt_value: Option<String>,
    pk: i64,
    hidden: i64,
}

/// The columns of `schema.table`, via `PRAGMA table_xinfo`: a superset of
/// `table_info` that also reports `hidden`, which is how a generated column
/// (`hidden` 2 or 3) is told apart from a stored one.
async fn table_columns(
    pool: &Pool<Sqlite>,
    schema: &str,
    table: &str,
) -> Result<Vec<RawColumn>, DatabaseError> {
    let query = format!("PRAGMA {schema}.table_xinfo({})", quote_ident(table));
    let rows = sqlx::query(&query)
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::connection)?;

    rows.into_iter()
        .map(|row| {
            let cid: i64 = row.try_get("cid").map_err(catalog_error)?;
            Ok(RawColumn {
                name: row.try_get("name").map_err(catalog_error)?,
                ordinal_position: cid as u32 + 1,
                decl_type: row.try_get("type").map_err(catalog_error)?,
                notnull: row.try_get::<i64, _>("notnull").map_err(catalog_error)? != 0,
                dflt_value: row.try_get("dflt_value").map_err(catalog_error)?,
                pk: row.try_get("pk").map_err(catalog_error)?,
                hidden: row.try_get("hidden").map_err(catalog_error)?,
            })
        })
        .collect()
}

fn to_column(raw: RawColumn, indexes: &[RawIndex]) -> Column {
    let bare_type = raw
        .decl_type
        .split('(')
        .next()
        .unwrap_or(&raw.decl_type)
        .trim()
        .to_string();
    let generated = raw.hidden == 2 || raw.hidden == 3;
    let primary = raw.pk > 0;
    let unique = primary
        || indexes
            .iter()
            .any(|index| index.unique && index.columns == [raw.name.clone()]);
    let auto_increment =
        primary && bare_type.eq_ignore_ascii_case("integer") && raw.dflt_value.is_none();

    let mut attributes = BTreeMap::new();
    if raw.hidden != 0 {
        attributes.insert("hidden".to_string(), serde_json::json!(raw.hidden));
    }

    Column {
        name: raw.name,
        ordinal_position: raw.ordinal_position,
        data_type: bare_type,
        full_type: raw.decl_type,
        nullable: !raw.notnull,
        default: raw.dflt_value,
        auto_increment,
        primary_key: primary,
        unique,
        comment: None,
        generated,
        expression: None,
        attributes,
    }
}

#[derive(Debug, Clone)]
struct RawIndex {
    name: String,
    unique: bool,
    origin: String,
    columns: Vec<String>,
}

async fn table_indexes(
    pool: &Pool<Sqlite>,
    schema: &str,
    table: &str,
) -> Result<Vec<RawIndex>, DatabaseError> {
    let list_query = format!("PRAGMA {schema}.index_list({})", quote_ident(table));
    let list_rows = sqlx::query(&list_query)
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::connection)?;

    let mut indexes = Vec::new();
    for row in list_rows {
        let name: String = row.try_get("name").map_err(catalog_error)?;
        let unique = row.try_get::<i64, _>("unique").map_err(catalog_error)? != 0;
        let origin: String = row.try_get("origin").map_err(catalog_error)?;

        let info_query = format!("PRAGMA {schema}.index_info({})", quote_ident(&name));
        let info_rows = sqlx::query(&info_query)
            .fetch_all(pool)
            .await
            .map_err(DatabaseError::connection)?;

        let mut columns: Vec<(i64, Option<String>)> = info_rows
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<i64, _>("seqno").map_err(catalog_error)?,
                    row.try_get::<Option<String>, _>("name")
                        .map_err(catalog_error)?,
                ))
            })
            .collect::<Result<_, DatabaseError>>()?;
        columns.sort_by_key(|(seqno, _)| *seqno);
        let columns: Vec<String> = columns.into_iter().filter_map(|(_, name)| name).collect();
        if columns.is_empty() {
            // Every column is an expression rather than a named column;
            // nothing here matches what the canonical model can express.
            continue;
        }

        indexes.push(RawIndex {
            name,
            unique,
            origin,
            columns,
        });
    }
    Ok(indexes)
}

fn indexes_from_raw(raw: &[RawIndex]) -> Vec<Index> {
    raw.iter()
        .map(|index| {
            let mut attributes = BTreeMap::new();
            attributes.insert("origin".to_string(), serde_json::json!(index.origin));
            Index {
                name: index.name.clone(),
                unique: index.unique,
                columns: index.columns.clone(),
                index_type: "BTREE".to_string(),
                attributes,
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
struct RawForeignKey {
    id: i64,
    seq: i64,
    referenced_table: String,
    column: String,
    referenced_column: Option<String>,
    on_update: String,
    on_delete: String,
}

/// The foreign keys of `schema.table`, via `PRAGMA foreign_key_list`.
///
/// SQLite never reports a constraint name, even when the DDL gave one, so
/// [`foreign_keys_from_raw`] synthesizes one from the table and the pragma's
/// own `id` grouping column.
async fn table_foreign_keys(
    pool: &Pool<Sqlite>,
    schema: &str,
    table: &str,
) -> Result<Vec<RawForeignKey>, DatabaseError> {
    let query = format!("PRAGMA {schema}.foreign_key_list({})", quote_ident(table));
    let rows = sqlx::query(&query)
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::connection)?;

    rows.into_iter()
        .map(|row| {
            Ok(RawForeignKey {
                id: row.try_get("id").map_err(catalog_error)?,
                seq: row.try_get("seq").map_err(catalog_error)?,
                referenced_table: row.try_get("table").map_err(catalog_error)?,
                column: row.try_get("from").map_err(catalog_error)?,
                referenced_column: row.try_get("to").map_err(catalog_error)?,
                on_update: row.try_get("on_update").map_err(catalog_error)?,
                on_delete: row.try_get("on_delete").map_err(catalog_error)?,
            })
        })
        .collect()
}

fn foreign_keys_from_raw(schema: &str, raw: &[RawForeignKey]) -> Vec<ForeignKey> {
    let mut by_id: BTreeMap<i64, Vec<&RawForeignKey>> = BTreeMap::new();
    for fk in raw {
        by_id.entry(fk.id).or_default().push(fk);
    }

    by_id
        .into_iter()
        .map(|(id, mut parts)| {
            parts.sort_by_key(|p| p.seq);
            let table = parts[0].referenced_table.clone();
            ForeignKey {
                name: format!("{table}_fk{id}"),
                columns: parts.iter().map(|p| p.column.clone()).collect(),
                referenced_schema: schema.to_string(),
                referenced_table: table,
                referenced_columns: parts
                    .iter()
                    .map(|p| p.referenced_column.clone().unwrap_or_default())
                    .collect(),
                on_update: parts[0].on_update.clone(),
                on_delete: parts[0].on_delete.clone(),
                attributes: BTreeMap::new(),
            }
        })
        .collect()
}

/// Quotes `name` as a SQLite identifier, so a table name can be interpolated
/// into a `PRAGMA` statement: `PRAGMA` arguments cannot be bound parameters,
/// only literal identifiers or strings.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn catalog_error(source: sqlx::Error) -> DatabaseError {
    DatabaseError::Catalog(source.to_string())
}
