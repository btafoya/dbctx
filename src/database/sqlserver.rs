//! SQL Server catalog introspection, using `INFORMATION_SCHEMA` first and
//! `sys.*` views only for facts it does not expose, per `SPEC.md` §7.

use std::collections::{BTreeMap, HashMap};

use tiberius::{AuthMethod, Client, Config, Row};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use crate::Result;
use crate::config::ConnectionConfig;
use crate::database::DatabaseError;
use crate::model::{Column, Database, DatabaseMetadata, Engine, ForeignKey, Index, Table, View};

type SqlClient = Client<tokio_util::compat::Compat<TcpStream>>;

/// Read every user schema of the target database.
pub async fn inspect(config: &ConnectionConfig) -> Result<Database> {
    let mut client = connect(config).await?;

    let version = version(&mut client).await?;
    let database_name = database_name(&mut client).await?;
    let default_collation = database_collation(&mut client).await?;

    let table_metadata = tables(&mut client).await?;
    let all_columns = columns(&mut client).await?;
    let indexes = indexes(&mut client).await?;
    let foreign_keys = foreign_keys(&mut client).await?;
    let identities = identity_columns(&mut client).await?;
    let table_comments = table_comments(&mut client).await?;
    let column_comments = column_comments(&mut client).await?;
    let computed = computed_columns(&mut client).await?;
    let view_names = views(&mut client).await?;

    let tables: Vec<Table> = table_metadata
        .into_iter()
        .map(|(schema, name)| {
            let key = (schema.clone(), name.clone());
            let cols = all_columns
                .get(&key)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|c| {
                    to_column(
                        c,
                        &indexes,
                        &identities,
                        &column_comments,
                        &computed,
                        &schema,
                        &name,
                    )
                })
                .collect();
            Table {
                schema,
                name: name.clone(),
                engine: None,
                charset: None,
                collation: None,
                comment: table_comments.get(&key).cloned(),
                columns: cols,
                indexes: indexes_for_table(&key, &indexes),
                foreign_keys: foreign_keys_for_table(&key, &foreign_keys),
                analysis: None,
                ai: None,
                attributes: std::collections::BTreeMap::new(),
            }
        })
        .collect();

    let views: Vec<View> = view_names
        .into_iter()
        .map(|(schema, name)| {
            let key = (schema.clone(), name.clone());
            View {
                schema,
                name: name.clone(),
                columns: all_columns
                    .get(&key)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| {
                        to_column(
                            c,
                            &indexes,
                            &identities,
                            &column_comments,
                            &computed,
                            &key.0,
                            &key.1,
                        )
                    })
                    .collect(),
                attributes: std::collections::BTreeMap::new(),
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
            engine: Engine::Sqlserver,
            engine_version: version,
            default_charset: None,
            default_collation,
            attributes: std::collections::BTreeMap::new(),
        },
        tables,
        views,
        attributes: std::collections::BTreeMap::new(),
    };
    database.sort();

    Ok(database)
}

pub(crate) async fn connect(
    config: &ConnectionConfig,
) -> Result<Client<tokio_util::compat::Compat<TcpStream>>, DatabaseError> {
    let mut tiberius_config = Config::new();
    tiberius_config.host(config.host());
    tiberius_config.port(config.port());
    tiberius_config.authentication(AuthMethod::sql_server(
        config.user().unwrap_or("sa"),
        config.password().unwrap_or(""),
    ));
    // Connect to master first; the target database is selected after login so
    // the SA login, whose default database is master, can reach it.
    tiberius_config.database("master");
    tiberius_config.trust_cert();

    let tcp = TcpStream::connect(tiberius_config.get_addr())
        .await
        .map_err(DatabaseError::connection)?;
    let _ = tcp.set_nodelay(true);

    let mut client = Client::connect(tiberius_config, tcp.compat_write())
        .await
        .map_err(DatabaseError::connection)?;
    let _ = client
        .execute(&format!("USE [{}]", config.database()), &[])
        .await
        .map_err(DatabaseError::connection)?;

    Ok(client)
}

async fn version(client: &mut SqlClient) -> Result<String, DatabaseError> {
    let row = client
        .query("SELECT @@VERSION AS version", &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_row()
        .await
        .map_err(DatabaseError::connection)?;
    row.and_then(|row| row.get::<&str, _>("version").map(|s| s.to_string()))
        .ok_or_else(|| DatabaseError::Catalog("@@VERSION returned no row".to_string()))
}

async fn database_name(client: &mut SqlClient) -> Result<String, DatabaseError> {
    let row = client
        .query("SELECT DB_NAME() AS database_name", &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_row()
        .await
        .map_err(DatabaseError::connection)?;
    row.and_then(|row| row.get::<&str, _>("database_name").map(|s| s.to_string()))
        .ok_or_else(|| DatabaseError::Catalog("DB_NAME() returned no row".to_string()))
}

async fn database_collation(client: &mut SqlClient) -> Result<Option<String>, DatabaseError> {
    let row = client
        .query(
            "SELECT CAST(DATABASEPROPERTYEX(DB_NAME(), 'Collation') AS NVARCHAR(255)) AS collation",
            &[],
        )
        .await
        .map_err(DatabaseError::connection)?
        .into_row()
        .await
        .map_err(DatabaseError::connection)?;
    Ok(row.and_then(|row| row.get::<&str, _>("collation").map(|s| s.to_string())))
}

async fn tables(client: &mut SqlClient) -> Result<Vec<(String, String)>, DatabaseError> {
    let query = r#"
        SELECT TABLE_SCHEMA, TABLE_NAME
        FROM information_schema.TABLES
        WHERE TABLE_TYPE = 'BASE TABLE'
        ORDER BY TABLE_SCHEMA, TABLE_NAME
    "#;
    let rows = client
        .query(query, &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_results()
        .await
        .map_err(DatabaseError::connection)?
        .into_iter()
        .next()
        .unwrap_or_default();

    rows.into_iter()
        .map(|row| {
            let schema = row
                .get::<&str, _>("TABLE_SCHEMA")
                .ok_or_else(|| DatabaseError::Catalog("TABLE_SCHEMA missing".to_string()))?
                .to_string();
            let name = row
                .get::<&str, _>("TABLE_NAME")
                .ok_or_else(|| DatabaseError::Catalog("TABLE_NAME missing".to_string()))?
                .to_string();
            Ok((schema, name))
        })
        .collect()
}

async fn views(client: &mut SqlClient) -> Result<Vec<(String, String)>, DatabaseError> {
    let query = r#"
        SELECT TABLE_SCHEMA, TABLE_NAME
        FROM information_schema.VIEWS
        ORDER BY TABLE_SCHEMA, TABLE_NAME
    "#;
    let rows = client
        .query(query, &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_results()
        .await
        .map_err(DatabaseError::connection)?
        .into_iter()
        .next()
        .unwrap_or_default();

    rows.into_iter()
        .map(|row| {
            let schema = row
                .get::<&str, _>("TABLE_SCHEMA")
                .ok_or_else(|| DatabaseError::Catalog("TABLE_SCHEMA missing".to_string()))?
                .to_string();
            let name = row
                .get::<&str, _>("TABLE_NAME")
                .ok_or_else(|| DatabaseError::Catalog("TABLE_NAME missing".to_string()))?
                .to_string();
            Ok((schema, name))
        })
        .collect()
}

#[derive(Debug, Clone)]
struct RawColumn {
    name: String,
    ordinal_position: u32,
    data_type: String,
    full_type: String,
    nullable: bool,
    default: Option<String>,
}

async fn columns(
    client: &mut SqlClient,
) -> Result<BTreeMap<(String, String), Vec<RawColumn>>, DatabaseError> {
    let query = r#"
        SELECT
            TABLE_SCHEMA,
            TABLE_NAME,
            COLUMN_NAME,
            ORDINAL_POSITION,
            DATA_TYPE,
            CHARACTER_MAXIMUM_LENGTH,
            NUMERIC_PRECISION,
            NUMERIC_SCALE,
            DATETIME_PRECISION,
            IS_NULLABLE,
            COLUMN_DEFAULT
        FROM information_schema.COLUMNS
        ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION
    "#;
    let rows = client
        .query(query, &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_results()
        .await
        .map_err(DatabaseError::connection)?
        .into_iter()
        .next()
        .unwrap_or_default();

    let mut by_table: BTreeMap<(String, String), Vec<RawColumn>> = BTreeMap::new();
    for row in rows {
        let schema = row
            .get::<&str, _>("TABLE_SCHEMA")
            .ok_or_else(|| DatabaseError::Catalog("TABLE_SCHEMA missing".to_string()))?
            .to_string();
        let name = row
            .get::<&str, _>("TABLE_NAME")
            .ok_or_else(|| DatabaseError::Catalog("TABLE_NAME missing".to_string()))?
            .to_string();
        let data_type = row
            .get::<&str, _>("DATA_TYPE")
            .ok_or_else(|| DatabaseError::Catalog("DATA_TYPE missing".to_string()))?
            .to_string();
        let full_type = full_sql_type(&row, &data_type);
        let column = RawColumn {
            name: row
                .get::<&str, _>("COLUMN_NAME")
                .ok_or_else(|| DatabaseError::Catalog("COLUMN_NAME missing".to_string()))?
                .to_string(),
            ordinal_position: row
                .get::<i32, _>("ORDINAL_POSITION")
                .ok_or_else(|| DatabaseError::Catalog("ORDINAL_POSITION missing".to_string()))?
                as u32,
            data_type,
            full_type,
            nullable: row
                .get::<&str, _>("IS_NULLABLE")
                .map(|v| v == "YES")
                .unwrap_or(false),
            default: row.get::<&str, _>("COLUMN_DEFAULT").map(|s| s.to_string()),
        };
        by_table.entry((schema, name)).or_default().push(column);
    }

    Ok(by_table)
}

fn full_sql_type(row: &Row, data_type: &str) -> String {
    if let Some(len) = row.get::<i32, _>("CHARACTER_MAXIMUM_LENGTH")
        && len >= 0
    {
        return format!("{}({})", data_type, len);
    }
    if let Some(precision) = row.get::<u8, _>("NUMERIC_PRECISION") {
        let scale = row.get::<i32, _>("NUMERIC_SCALE").unwrap_or(0);
        return format!("{}({},{})", data_type, precision, scale);
    }
    if let Some(precision) = row.get::<u8, _>("DATETIME_PRECISION") {
        return format!("{}({})", data_type, precision);
    }
    data_type.to_string()
}

#[derive(Debug, Clone)]
struct RawIndex {
    name: String,
    unique: bool,
    primary: bool,
    column: String,
    sequence: u32,
    index_type: String,
}

async fn indexes(
    client: &mut SqlClient,
) -> Result<BTreeMap<(String, String), Vec<RawIndex>>, DatabaseError> {
    let query = r#"
        SELECT
            s.name AS table_schema,
            t.name AS table_name,
            i.name AS index_name,
            i.type_desc AS index_type,
            i.is_unique AS is_unique,
            i.is_primary_key AS is_primary_key,
            c.name AS column_name,
            ic.key_ordinal AS key_ordinal,
            ic.is_descending_key AS is_descending
        FROM sys.indexes i
        JOIN sys.tables t ON i.object_id = t.object_id
        JOIN sys.schemas s ON t.schema_id = s.schema_id
        JOIN sys.index_columns ic ON i.object_id = ic.object_id AND i.index_id = ic.index_id
        JOIN sys.columns c ON ic.object_id = c.object_id AND ic.column_id = c.column_id
        WHERE i.type IN (1, 2)
          AND t.is_ms_shipped = 0
        ORDER BY s.name, t.name, i.name, ic.key_ordinal
    "#;
    let rows = client
        .query(query, &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_results()
        .await
        .map_err(DatabaseError::connection)?
        .into_iter()
        .next()
        .unwrap_or_default();

    let mut by_table: BTreeMap<(String, String), Vec<RawIndex>> = BTreeMap::new();
    for row in rows {
        let schema = row
            .get::<&str, _>("table_schema")
            .ok_or_else(|| DatabaseError::Catalog("table_schema missing".to_string()))?
            .to_string();
        let table = row
            .get::<&str, _>("table_name")
            .ok_or_else(|| DatabaseError::Catalog("table_name missing".to_string()))?
            .to_string();
        let index = RawIndex {
            name: row
                .get::<&str, _>("index_name")
                .ok_or_else(|| DatabaseError::Catalog("index_name missing".to_string()))?
                .to_string(),
            unique: row
                .get::<bool, _>("is_unique")
                .ok_or_else(|| DatabaseError::Catalog("is_unique missing".to_string()))?,
            primary: row
                .get::<bool, _>("is_primary_key")
                .ok_or_else(|| DatabaseError::Catalog("is_primary_key missing".to_string()))?,
            column: row
                .get::<&str, _>("column_name")
                .ok_or_else(|| DatabaseError::Catalog("column_name missing".to_string()))?
                .to_string(),
            sequence: row
                .get::<u8, _>("key_ordinal")
                .ok_or_else(|| DatabaseError::Catalog("key_ordinal missing".to_string()))?
                as u32,
            index_type: row
                .get::<&str, _>("index_type")
                .ok_or_else(|| DatabaseError::Catalog("index_type missing".to_string()))?
                .to_string(),
        };
        by_table.entry((schema, table)).or_default().push(index);
    }

    Ok(by_table)
}

fn indexes_for_table(
    table_key: &(String, String),
    indexes: &BTreeMap<(String, String), Vec<RawIndex>>,
) -> Vec<Index> {
    let empty = Vec::new();
    let raw = indexes.get(table_key).unwrap_or(&empty);

    let mut by_name: BTreeMap<String, Vec<&RawIndex>> = BTreeMap::new();
    for index in raw {
        by_name.entry(index.name.clone()).or_default().push(index);
    }

    by_name
        .into_iter()
        .map(|(name, mut parts)| {
            parts.sort_by_key(|i| i.sequence);
            let unique = parts[0].unique || parts[0].primary;
            let index_type = parts[0].index_type.clone();
            Index {
                name,
                unique,
                columns: parts.into_iter().map(|i| i.column.clone()).collect(),
                index_type,
                attributes: std::collections::BTreeMap::new(),
            }
        })
        .collect()
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
    client: &mut SqlClient,
) -> Result<BTreeMap<(String, String), Vec<RawForeignKey>>, DatabaseError> {
    let query = r#"
        SELECT
            s.name AS table_schema,
            t.name AS table_name,
            fk.name AS constraint_name,
            c.name AS column_name,
            fkc.constraint_column_id AS ordinal,
            rs.name AS referenced_schema_name,
            rt.name AS referenced_table_name,
            rc.name AS referenced_column_name,
            fk.update_referential_action_desc AS on_update,
            fk.delete_referential_action_desc AS on_delete
        FROM sys.foreign_keys fk
        JOIN sys.tables t ON fk.parent_object_id = t.object_id
        JOIN sys.schemas s ON t.schema_id = s.schema_id
        JOIN sys.foreign_key_columns fkc ON fk.object_id = fkc.constraint_object_id
        JOIN sys.columns c ON fkc.parent_object_id = c.object_id AND fkc.parent_column_id = c.column_id
        JOIN sys.tables rt ON fk.referenced_object_id = rt.object_id
        JOIN sys.schemas rs ON rt.schema_id = rs.schema_id
        JOIN sys.columns rc ON fkc.referenced_object_id = rc.object_id AND fkc.referenced_column_id = rc.column_id
        WHERE t.is_ms_shipped = 0
        ORDER BY s.name, t.name, fk.name, fkc.constraint_column_id
    "#;
    let rows = client
        .query(query, &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_results()
        .await
        .map_err(DatabaseError::connection)?
        .into_iter()
        .next()
        .unwrap_or_default();

    let mut by_table: BTreeMap<(String, String), Vec<RawForeignKey>> = BTreeMap::new();
    for row in rows {
        let schema = row
            .get::<&str, _>("table_schema")
            .ok_or_else(|| DatabaseError::Catalog("table_schema missing".to_string()))?
            .to_string();
        let table = row
            .get::<&str, _>("table_name")
            .ok_or_else(|| DatabaseError::Catalog("table_name missing".to_string()))?
            .to_string();
        let fk = RawForeignKey {
            name: row
                .get::<&str, _>("constraint_name")
                .ok_or_else(|| DatabaseError::Catalog("constraint_name missing".to_string()))?
                .to_string(),
            column: row
                .get::<&str, _>("column_name")
                .ok_or_else(|| DatabaseError::Catalog("column_name missing".to_string()))?
                .to_string(),
            sequence: row
                .get::<i32, _>("ordinal")
                .ok_or_else(|| DatabaseError::Catalog("ordinal missing".to_string()))?
                as u32,
            referenced_schema: row
                .get::<&str, _>("referenced_schema_name")
                .ok_or_else(|| {
                    DatabaseError::Catalog("referenced_schema_name missing".to_string())
                })?
                .to_string(),
            referenced_table: row
                .get::<&str, _>("referenced_table_name")
                .ok_or_else(|| DatabaseError::Catalog("referenced_table_name missing".to_string()))?
                .to_string(),
            referenced_column: row
                .get::<&str, _>("referenced_column_name")
                .ok_or_else(|| {
                    DatabaseError::Catalog("referenced_column_name missing".to_string())
                })?
                .to_string(),
            on_update: row
                .get::<&str, _>("on_update")
                .unwrap_or("NO ACTION")
                .to_string(),
            on_delete: row
                .get::<&str, _>("on_delete")
                .unwrap_or("NO ACTION")
                .to_string(),
        };
        by_table.entry((schema, table)).or_default().push(fk);
    }

    Ok(by_table)
}

fn foreign_keys_for_table(
    table_key: &(String, String),
    foreign_keys: &BTreeMap<(String, String), Vec<RawForeignKey>>,
) -> Vec<ForeignKey> {
    let empty = Vec::new();
    let raw = foreign_keys.get(table_key).unwrap_or(&empty);

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
                attributes: std::collections::BTreeMap::new(),
            }
        })
        .collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

async fn identity_columns(
    client: &mut SqlClient,
) -> Result<HashMap<(String, String, String), ()>, DatabaseError> {
    let query = r#"
        SELECT
            s.name AS table_schema,
            t.name AS table_name,
            c.name AS column_name
        FROM sys.identity_columns ic
        JOIN sys.tables t ON ic.object_id = t.object_id
        JOIN sys.schemas s ON t.schema_id = s.schema_id
        JOIN sys.columns c ON ic.object_id = c.object_id AND ic.column_id = c.column_id
        WHERE t.is_ms_shipped = 0
    "#;
    let rows = client
        .query(query, &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_results()
        .await
        .map_err(DatabaseError::connection)?
        .into_iter()
        .next()
        .unwrap_or_default();

    let mut identities = HashMap::new();
    for row in rows {
        let schema = row
            .get::<&str, _>("table_schema")
            .ok_or_else(|| DatabaseError::Catalog("table_schema missing".to_string()))?
            .to_string();
        let table = row
            .get::<&str, _>("table_name")
            .ok_or_else(|| DatabaseError::Catalog("table_name missing".to_string()))?
            .to_string();
        let column = row
            .get::<&str, _>("column_name")
            .ok_or_else(|| DatabaseError::Catalog("column_name missing".to_string()))?
            .to_string();
        identities.insert((schema, table, column), ());
    }

    Ok(identities)
}

async fn table_comments(
    client: &mut SqlClient,
) -> Result<HashMap<(String, String), String>, DatabaseError> {
    let query = r#"
        SELECT
            s.name AS table_schema,
            t.name AS table_name,
            CAST(p.value AS NVARCHAR(MAX)) AS comment
        FROM sys.tables t
        JOIN sys.schemas s ON t.schema_id = s.schema_id
        JOIN sys.extended_properties p ON p.major_id = t.object_id AND p.minor_id = 0 AND p.name = 'MS_Description'
        WHERE t.is_ms_shipped = 0
    "#;
    let rows = client
        .query(query, &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_results()
        .await
        .map_err(DatabaseError::connection)?
        .into_iter()
        .next()
        .unwrap_or_default();

    let mut comments = HashMap::new();
    for row in rows {
        let schema = row
            .get::<&str, _>("table_schema")
            .ok_or_else(|| DatabaseError::Catalog("table_schema missing".to_string()))?
            .to_string();
        let table = row
            .get::<&str, _>("table_name")
            .ok_or_else(|| DatabaseError::Catalog("table_name missing".to_string()))?
            .to_string();
        let comment = row
            .get::<&str, _>("comment")
            .ok_or_else(|| DatabaseError::Catalog("comment missing".to_string()))?
            .to_string();
        comments.insert((schema, table), comment);
    }

    Ok(comments)
}

async fn column_comments(
    client: &mut SqlClient,
) -> Result<HashMap<(String, String, String), String>, DatabaseError> {
    let query = r#"
        SELECT
            s.name AS table_schema,
            t.name AS table_name,
            c.name AS column_name,
            CAST(p.value AS NVARCHAR(MAX)) AS comment
        FROM sys.columns c
        JOIN sys.tables t ON c.object_id = t.object_id
        JOIN sys.schemas s ON t.schema_id = s.schema_id
        JOIN sys.extended_properties p ON p.major_id = c.object_id AND p.minor_id = c.column_id AND p.name = 'MS_Description'
        WHERE t.is_ms_shipped = 0
    "#;
    let rows = client
        .query(query, &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_results()
        .await
        .map_err(DatabaseError::connection)?
        .into_iter()
        .next()
        .unwrap_or_default();

    let mut comments = HashMap::new();
    for row in rows {
        let schema = row
            .get::<&str, _>("table_schema")
            .ok_or_else(|| DatabaseError::Catalog("table_schema missing".to_string()))?
            .to_string();
        let table = row
            .get::<&str, _>("table_name")
            .ok_or_else(|| DatabaseError::Catalog("table_name missing".to_string()))?
            .to_string();
        let column = row
            .get::<&str, _>("column_name")
            .ok_or_else(|| DatabaseError::Catalog("column_name missing".to_string()))?
            .to_string();
        let comment = row
            .get::<&str, _>("comment")
            .ok_or_else(|| DatabaseError::Catalog("comment missing".to_string()))?
            .to_string();
        comments.insert((schema, table, column), comment);
    }

    Ok(comments)
}

async fn computed_columns(
    client: &mut SqlClient,
) -> Result<HashMap<(String, String, String), String>, DatabaseError> {
    let query = r#"
        SELECT
            s.name AS table_schema,
            t.name AS table_name,
            c.name AS column_name,
            cc.definition AS expression
        FROM sys.computed_columns cc
        JOIN sys.columns c ON cc.object_id = c.object_id AND cc.column_id = c.column_id
        JOIN sys.tables t ON cc.object_id = t.object_id
        JOIN sys.schemas s ON t.schema_id = s.schema_id
        WHERE t.is_ms_shipped = 0
    "#;
    let rows = client
        .query(query, &[])
        .await
        .map_err(DatabaseError::connection)?
        .into_results()
        .await
        .map_err(DatabaseError::connection)?
        .into_iter()
        .next()
        .unwrap_or_default();

    let mut computed = HashMap::new();
    for row in rows {
        let schema = row
            .get::<&str, _>("table_schema")
            .ok_or_else(|| DatabaseError::Catalog("table_schema missing".to_string()))?
            .to_string();
        let table = row
            .get::<&str, _>("table_name")
            .ok_or_else(|| DatabaseError::Catalog("table_name missing".to_string()))?
            .to_string();
        let column = row
            .get::<&str, _>("column_name")
            .ok_or_else(|| DatabaseError::Catalog("column_name missing".to_string()))?
            .to_string();
        let expression = row
            .get::<&str, _>("expression")
            .ok_or_else(|| DatabaseError::Catalog("expression missing".to_string()))?
            .to_string();
        computed.insert((schema, table, column), expression);
    }

    Ok(computed)
}

fn to_column(
    raw: RawColumn,
    indexes: &BTreeMap<(String, String), Vec<RawIndex>>,
    identities: &HashMap<(String, String, String), ()>,
    comments: &HashMap<(String, String, String), String>,
    computed: &HashMap<(String, String, String), String>,
    schema: &str,
    table: &str,
) -> Column {
    let key = (schema.to_string(), table.to_string());
    let primary = is_primary_key(&key, &raw.name, indexes);
    let unique = is_unique_column(&key, &raw.name, indexes);
    let auto_increment =
        identities.contains_key(&(schema.to_string(), table.to_string(), raw.name.clone()));
    let comment = comments
        .get(&(schema.to_string(), table.to_string(), raw.name.clone()))
        .cloned();
    let expression = computed
        .get(&(schema.to_string(), table.to_string(), raw.name.clone()))
        .cloned();
    let generated = expression.is_some();

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
        comment,
        generated,
        expression,
        attributes: std::collections::BTreeMap::new(),
    }
}

fn is_primary_key(
    table_key: &(String, String),
    column: &str,
    indexes: &BTreeMap<(String, String), Vec<RawIndex>>,
) -> bool {
    indexes
        .get(table_key)
        .unwrap_or(&Vec::new())
        .iter()
        .any(|i| i.primary && i.column == column)
}

fn is_unique_column(
    table_key: &(String, String),
    column: &str,
    indexes: &BTreeMap<(String, String), Vec<RawIndex>>,
) -> bool {
    let empty = Vec::new();
    let raw = indexes.get(table_key).unwrap_or(&empty);

    let mut by_name: BTreeMap<String, Vec<&RawIndex>> = BTreeMap::new();
    for index in raw {
        by_name.entry(index.name.clone()).or_default().push(index);
    }

    by_name
        .values()
        .any(|parts| parts.len() == 1 && parts[0].column == column && parts[0].unique)
}
