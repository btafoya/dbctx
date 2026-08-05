//! Read-only SQL execution used only by the `execute-statement` command.
//!
//! This path is deliberately isolated from introspection: it validates that a
//! statement is read-only before contacting the database, executes it through
//! the same connection layer, and returns the tabular result as JSON. It never
//! writes to the database and never feeds its output into the canonical schema
//! model.

use std::time::Duration;

use mysql_async::Row as MySqlRow;
use mysql_async::prelude::Queryable;
use serde::Serialize;
use thiserror::Error;
use tiberius::Row as SqlServerRow;

use crate::Result;
use crate::config::{ConnectionConfig, Driver};
use crate::database::DatabaseError;

/// Why a read-only statement could not be executed.
#[derive(Debug, Error)]
pub enum ExecutionError {
    /// The statement contains a mutating keyword.
    #[error("statement is not read-only: it contains `{keyword}`")]
    NotReadOnly { keyword: String },

    /// More than one statement was supplied.
    #[error("only a single statement is allowed")]
    MultipleStatements,

    /// The statement did not complete within the configured timeout.
    #[error("statement execution timed out after {0} seconds")]
    Timeout(u64),

    /// The database returned an error while executing the statement.
    #[error("could not execute statement: {0}")]
    Database(#[from] DatabaseError),

    /// The result could not be serialized to JSON.
    #[error("could not serialize results: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl ExecutionError {
    /// The exit code `CLI.md` gives this failure.
    pub fn exit_code(&self) -> u8 {
        match self {
            ExecutionError::NotReadOnly { .. } | ExecutionError::MultipleStatements => 8,
            ExecutionError::Database(DatabaseError::Connection(_)) => 2,
            ExecutionError::Database(DatabaseError::Catalog(_)) => 7,
            ExecutionError::Timeout(_) => 7,
            ExecutionError::Serialization(_) => 1,
        }
    }
}

/// The result of executing a single read-only statement.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionResult {
    /// Column names in the order returned by the database.
    pub columns: Vec<String>,
    /// Rows, each a list of cell values in column order.
    pub rows: Vec<Vec<serde_json::Value>>,
    /// Number of rows returned.
    pub row_count: usize,
    /// Time the database took to execute the statement, in milliseconds.
    pub execution_time_ms: u64,
}

impl ExecutionResult {
    fn new(
        columns: Vec<String>,
        rows: Vec<Vec<serde_json::Value>>,
        execution_time_ms: u64,
    ) -> Self {
        let row_count = rows.len();
        Self {
            columns,
            rows,
            row_count,
            execution_time_ms,
        }
    }
}

/// Execute `sql` against `config` and return the result as a JSON-ready value.
///
/// The statement is checked for read-only semantics before any database contact.
/// Execution is cancelled if it does not finish within `timeout`.
pub async fn execute(
    config: &ConnectionConfig,
    sql: &str,
    timeout: Duration,
) -> Result<ExecutionResult, ExecutionError> {
    validate_read_only(sql)?;

    let start = tokio::time::Instant::now();
    let result = tokio::time::timeout(timeout, execute_unchecked(config, sql)).await;
    let execution_time_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(res)) => Ok(ExecutionResult::new(
            res.columns,
            res.rows,
            execution_time_ms,
        )),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(ExecutionError::Timeout(timeout.as_secs())),
    }
}

async fn execute_unchecked(
    config: &ConnectionConfig,
    sql: &str,
) -> Result<ExecutionResult, ExecutionError> {
    match config.driver() {
        Driver::Mysql | Driver::Mariadb => execute_mysql(config, sql).await,
        Driver::Sqlsrv => execute_sqlserver(config, sql).await,
    }
}

async fn execute_mysql(
    config: &ConnectionConfig,
    sql: &str,
) -> Result<ExecutionResult, ExecutionError> {
    let mut conn = crate::database::mysql::connect(config).await?;

    let mut result = conn
        .query_iter(sql)
        .await
        .map_err(DatabaseError::connection)?;

    let columns: Vec<String> = result
        .columns_ref()
        .iter()
        .map(|col| col.name_str().to_string())
        .collect();

    let mut rows = Vec::new();
    while let Some(row) = result.next().await.map_err(DatabaseError::connection)? {
        rows.push(mysql_row_to_json(&row));
    }

    Ok(ExecutionResult::new(columns, rows, 0))
}

fn mysql_row_to_json(row: &MySqlRow) -> Vec<serde_json::Value> {
    (0..row.len())
        .map(|index| mysql_value_to_json(row.as_ref(index).unwrap_or(&mysql_async::Value::NULL)))
        .collect()
}

fn mysql_value_to_json(value: &mysql_async::Value) -> serde_json::Value {
    use serde_json::Value;

    match value {
        mysql_async::Value::NULL => Value::Null,
        mysql_async::Value::Bytes(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(text) => Value::String(text),
            Err(_) => Value::Array(
                bytes
                    .iter()
                    .map(|byte| Value::Number((*byte).into()))
                    .collect(),
            ),
        },
        mysql_async::Value::Int(number) => Value::Number((*number).into()),
        mysql_async::Value::UInt(number) => Value::Number((*number).into()),
        mysql_async::Value::Float(number) => serde_json::Number::from_f64(f64::from(*number))
            .map(Value::Number)
            .unwrap_or(Value::Null),
        mysql_async::Value::Double(number) => serde_json::Number::from_f64(*number)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        mysql_async::Value::Date(year, month, day, hour, minute, second, microsecond) => {
            Value::String(format!(
                "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{microsecond:06}"
            ))
        }
        mysql_async::Value::Time(negative, days, hours, minutes, seconds, microseconds) => {
            let sign = if *negative { '-' } else { '+' };
            Value::String(format!(
                "{sign}{days}:{hours:02}:{minutes:02}:{seconds:02}.{microseconds:06}"
            ))
        }
    }
}

async fn execute_sqlserver(
    config: &ConnectionConfig,
    sql: &str,
) -> Result<ExecutionResult, ExecutionError> {
    let mut client = crate::database::sqlserver::connect(config).await?;

    let mut stream = client
        .query(sql, &[])
        .await
        .map_err(DatabaseError::connection)?;

    let columns: Vec<String> = stream
        .columns()
        .await
        .map_err(DatabaseError::connection)?
        .map(|cols| cols.iter().map(|col| col.name().to_string()).collect())
        .unwrap_or_default();

    let rows = stream
        .into_first_result()
        .await
        .map_err(DatabaseError::connection)?
        .iter()
        .map(sqlserver_row_to_json)
        .collect();

    Ok(ExecutionResult::new(columns, rows, 0))
}

fn sqlserver_row_to_json(row: &SqlServerRow) -> Vec<serde_json::Value> {
    row.cells()
        .map(|(_, cell)| sqlserver_value_to_json(cell))
        .collect()
}

fn sqlserver_value_to_json(data: &tiberius::ColumnData<'_>) -> serde_json::Value {
    use serde_json::Value;
    use tiberius::ColumnData;

    match data {
        ColumnData::U8(Some(value)) => Value::Number(serde_json::Number::from(*value)),
        ColumnData::I16(Some(value)) => Value::Number(serde_json::Number::from(*value)),
        ColumnData::I32(Some(value)) => Value::Number(serde_json::Number::from(*value)),
        ColumnData::I64(Some(value)) => Value::Number(serde_json::Number::from(*value)),
        ColumnData::F32(Some(value)) => serde_json::Number::from_f64(f64::from(*value))
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ColumnData::F64(Some(value)) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ColumnData::Bit(Some(value)) => Value::Bool(*value),
        ColumnData::String(Some(value)) => Value::String(format!("{value}")),
        ColumnData::Guid(Some(value)) => Value::String(format!("{value}")),
        ColumnData::Binary(Some(value)) => Value::Array(
            value
                .iter()
                .map(|byte| Value::Number(serde_json::Number::from(*byte)))
                .collect(),
        ),
        ColumnData::Numeric(Some(value)) => Value::String(format!("{value}")),
        ColumnData::DateTime(Some(value)) => Value::String(format_sqlserver_datetime(value)),
        ColumnData::SmallDateTime(Some(value)) => {
            Value::String(format_sqlserver_smalldatetime(value))
        }
        ColumnData::Date(Some(value)) => Value::String(format_sqlserver_date(value)),
        ColumnData::Time(Some(value)) => Value::String(format_sqlserver_time(value)),
        ColumnData::DateTime2(Some(value)) => Value::String(format_sqlserver_datetime2(value)),
        ColumnData::DateTimeOffset(Some(value)) => {
            Value::String(format_sqlserver_datetimeoffset(value))
        }
        ColumnData::Xml(Some(value)) => Value::String(format!("{value}")),
        _ => Value::Null,
    }
}

fn format_sqlserver_date(date: &tiberius::time::Date) -> String {
    let base = time::Date::from_ordinal_date(1, 1).expect("valid date");
    let date = base
        .checked_add(time::Duration::days(date.days() as i64 - 1))
        .unwrap_or(base);
    date.to_string()
}

fn format_sqlserver_datetime(datetime: &tiberius::time::DateTime) -> String {
    let base = time::Date::from_ordinal_date(1900, 1).expect("valid date");
    let date = base
        .checked_add(time::Duration::days(datetime.days() as i64))
        .unwrap_or(base);

    let seconds = (datetime.seconds_fragments() as i64) / 300;
    let time =
        time::Time::from_hms(0, 0, 0).expect("valid time") + time::Duration::seconds(seconds);

    time::PrimitiveDateTime::new(date, time).to_string()
}

fn format_sqlserver_smalldatetime(datetime: &tiberius::time::SmallDateTime) -> String {
    let base = time::Date::from_ordinal_date(1900, 1).expect("valid date");
    let date = base
        .checked_add(time::Duration::days(datetime.days() as i64))
        .unwrap_or(base);

    let seconds = (datetime.seconds_fragments() as i64) * 60;
    let time =
        time::Time::from_hms(0, 0, 0).expect("valid time") + time::Duration::seconds(seconds);

    time::PrimitiveDateTime::new(date, time).to_string()
}

fn format_sqlserver_time(time_value: &tiberius::time::Time) -> String {
    let nanoseconds = if time_value.scale() >= 9 {
        time_value.increments() as i64
    } else {
        time_value.increments() as i64 * 10i64.pow(9 - u32::from(time_value.scale()))
    };

    let midnight = time::Time::from_hms(0, 0, 0).expect("valid time");
    (midnight + time::Duration::nanoseconds(nanoseconds)).to_string()
}

fn format_sqlserver_datetime2(datetime: &tiberius::time::DateTime2) -> String {
    format!(
        "{} {}",
        format_sqlserver_date(&datetime.date()),
        format_sqlserver_time(&datetime.time())
    )
}

fn format_sqlserver_datetimeoffset(offset: &tiberius::time::DateTimeOffset) -> String {
    let base = format_sqlserver_datetime2(&offset.datetime2());
    let minutes = offset.offset();
    let sign = if minutes >= 0 { '+' } else { '-' };
    let minutes = minutes.abs();
    format!("{base}{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Reject any statement that is not a single read-only query.
fn validate_read_only(sql: &str) -> Result<(), ExecutionError> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err(ExecutionError::NotReadOnly {
            keyword: "empty statement".to_string(),
        });
    }

    let mut scanner = Scanner::new(sql);
    scanner.skip_noise();
    let first = scanner.next_word().to_ascii_uppercase();
    if !matches!(first.as_str(), "SELECT" | "WITH") {
        return Err(ExecutionError::NotReadOnly { keyword: first });
    }

    let mut scanner = Scanner::new(sql);
    while let Some(token) = scanner.next_token() {
        match token {
            Token::Word(word) => {
                let upper = word.to_ascii_uppercase();
                if is_mutating_keyword(&upper) {
                    return Err(ExecutionError::NotReadOnly { keyword: upper });
                }
            }
            Token::Semicolon => {
                scanner.skip_noise();
                if !scanner.is_at_end() {
                    return Err(ExecutionError::MultipleStatements);
                }
            }
            Token::Other => {}
        }
    }

    Ok(())
}

fn is_mutating_keyword(word: &str) -> bool {
    const MUTATING: &[&str] = &[
        "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "TRUNCATE", "MERGE",
    ];
    MUTATING.contains(&word)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Semicolon,
    Other,
}

/// A tiny SQL scanner that ignores comments and quoted content so that keyword
/// and semicolon checks only see the literal statement structure.
struct Scanner<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> Scanner<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.input.len()
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.position += character.len_utf8();
        Some(character)
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.input[self.position..].starts_with(prefix)
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.peek() {
            if character.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_noise(&mut self) {
        loop {
            self.skip_whitespace();
            if self.skip_comment() {
                continue;
            }
            break;
        }
    }

    fn skip_comment(&mut self) -> bool {
        if self.starts_with("/*") {
            let after = &self.input[self.position + 2..];
            match after.find("*/") {
                Some(end) => self.position += 2 + end + 2,
                None => self.position = self.input.len(),
            }
            true
        } else if self.starts_with("--") {
            let after = &self.input[self.position + 2..];
            match after.find('\n') {
                Some(end) => self.position += 2 + end + 1,
                None => self.position = self.input.len(),
            }
            true
        } else {
            false
        }
    }

    fn next_word(&mut self) -> String {
        self.skip_noise();

        let mut word = String::new();
        if let Some(character) = self.peek() {
            if is_word_start(character) {
                word.push(character);
                self.advance();
                while let Some(character) = self.peek() {
                    if is_word_continue(character) {
                        word.push(character);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }
        word
    }

    fn next_token(&mut self) -> Option<Token> {
        self.skip_noise();
        if self.is_at_end() {
            return None;
        }

        let character = self.peek().expect("not at end");
        match character {
            ';' => {
                self.advance();
                Some(Token::Semicolon)
            }
            '\'' | '"' | '`' | '[' => {
                self.skip_quoted();
                Some(Token::Other)
            }
            _ if character.is_ascii_alphabetic() || character == '_' || character == '@' => {
                Some(Token::Word(self.next_word()))
            }
            _ => {
                self.advance();
                Some(Token::Other)
            }
        }
    }

    fn skip_quoted(&mut self) {
        let quote = self.peek().expect("not at end");
        match quote {
            '\'' => self.skip_single_quoted_string(),
            '"' => self.skip_delimited('"'),
            '`' => self.skip_delimited('`'),
            '[' => self.skip_delimited(']'),
            _ => {
                self.advance();
            }
        }
    }

    fn skip_single_quoted_string(&mut self) {
        // Handle N'...', X'...', and B'...' prefixes in a single pass.
        if let Some(prefix) = self.peek() {
            let is_string_prefix = "nNxXbB".contains(prefix);
            if is_string_prefix && self.starts_with(&format!("{prefix}'")) {
                self.advance(); // prefix
            }
        }

        if !self.starts_with("'") {
            return;
        }
        self.advance(); // opening quote

        while let Some(character) = self.peek() {
            if character == '\'' {
                self.advance();
                if self.starts_with("'") {
                    // Escaped quote ('') in a string literal.
                    self.advance();
                } else {
                    break;
                }
            } else {
                self.advance();
            }
        }
    }

    fn skip_delimited(&mut self, closer: char) {
        if self.is_at_end() {
            return;
        }
        self.advance(); // opening delimiter

        while let Some(character) = self.peek() {
            if character == closer {
                self.advance();
                break;
            }
            if character == '\\' {
                self.advance();
                self.advance();
            } else {
                self.advance();
            }
        }
    }
}

fn is_word_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_' || character == '@'
}

fn is_word_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '@' || character == '$'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_allowed(sql: &str) {
        validate_read_only(sql).expect("expected statement to be allowed");
    }

    fn assert_rejected(sql: &str, expected_keyword: &str) {
        let err = validate_read_only(sql).expect_err("expected statement to be rejected");
        let message = err.to_string();
        assert!(
            message.contains(expected_keyword),
            "{message:?} did not contain {expected_keyword:?}"
        );
    }

    #[test]
    fn select_is_allowed() {
        assert_allowed("SELECT 1");
    }

    #[test]
    fn with_cte_is_allowed() {
        assert_allowed("WITH cte AS (SELECT 1) SELECT * FROM cte");
    }

    #[test]
    fn leading_and_trailing_noise_is_ignored() {
        assert_allowed("/* comment */ -- another\nSELECT 1");
    }

    #[test]
    fn mutating_keywords_inside_string_literals_are_ignored() {
        assert_allowed("SELECT * FROM t WHERE name = 'delete'");
    }

    #[test]
    fn semicolons_inside_string_literals_are_ignored() {
        assert_allowed("SELECT * FROM t WHERE name = 'a;b'");
    }

    #[test]
    fn mutating_keywords_inside_quoted_identifiers_are_ignored() {
        assert_allowed("SELECT * FROM `update`");
        assert_allowed("SELECT * FROM [delete]");
        assert_allowed("SELECT * FROM \"drop\"");
    }

    #[test]
    fn insert_is_rejected() {
        assert_rejected("INSERT INTO t VALUES (1)", "INSERT");
    }

    #[test]
    fn update_is_rejected() {
        assert_rejected("UPDATE t SET x = 1", "UPDATE");
    }

    #[test]
    fn delete_is_rejected() {
        assert_rejected("DELETE FROM t", "DELETE");
    }

    #[test]
    fn drop_is_rejected() {
        assert_rejected("DROP TABLE t", "DROP");
    }

    #[test]
    fn alter_is_rejected() {
        assert_rejected("ALTER TABLE t ADD c INT", "ALTER");
    }

    #[test]
    fn create_is_rejected() {
        assert_rejected("CREATE TABLE t (id INT)", "CREATE");
    }

    #[test]
    fn truncate_is_rejected() {
        assert_rejected("TRUNCATE TABLE t", "TRUNCATE");
    }

    #[test]
    fn merge_is_rejected() {
        assert_rejected(
            "MERGE t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET x = 1",
            "MERGE",
        );
    }

    #[test]
    fn multiple_statements_are_rejected() {
        assert_rejected("SELECT 1; SELECT 2", "single");
        assert_rejected("SELECT 1; DROP TABLE t", "single");
    }

    #[test]
    fn mutating_words_hidden_in_comments_are_caught() {
        assert_rejected("SELECT 1\n/* comment */\nDROP TABLE t", "DROP");
    }

    #[test]
    fn non_select_statements_are_rejected() {
        assert_rejected("SHOW TABLES", "SHOW");
        assert_rejected("EXEC sp_help", "EXEC");
    }

    #[test]
    fn empty_statement_is_rejected() {
        assert_rejected("", "empty statement");
        assert_rejected("   ", "empty statement");
    }
}
