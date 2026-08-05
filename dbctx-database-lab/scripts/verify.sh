#!/usr/bin/env bash
# Verify every running database in the dbctx lab.
# Exits non-zero if any engine fails connectivity, authentication, schema,
# CRUD, transactions, prepared statements, Unicode, binary, timestamps,
# indexes, views, or stored procedures.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/lib.sh"

ERRORS=0

increment_error() {
    ERRORS=$((ERRORS + 1))
}

# ---------------------------------------------------------------------------
# Per-engine checks
# ---------------------------------------------------------------------------
verify_connectivity() {
    local engine="$1"
    section "${engine}: connectivity"
    if engine_exec "${engine}" "SELECT 1;" > /dev/null 2>&1; then
        pass "connectivity"
    else
        fail "connectivity"
        increment_error
    fi
}

verify_auth() {
    local engine="$1"
    section "${engine}: authentication"
    case "${engine}" in
        postgres)
            if docker exec "${POSTGRES_CONTAINER}" psql -U "${DB_USERNAME}" -d "${DB_DATABASE}" -c "SELECT 1;" > /dev/null 2>&1; then
                pass "authentication"
            else
                fail "authentication"
                increment_error
            fi
            ;;
        mariadb)
            if docker exec "${MARIADB_CONTAINER}" mariadb -u "${DB_USERNAME}" -p"${DB_PASSWORD}" --skip-ssl "${DB_DATABASE}" -e "SELECT 1;" > /dev/null 2>&1; then
                pass "authentication"
            else
                fail "authentication"
                increment_error
            fi
            ;;
        mysql)
            if docker exec "${MYSQL_CONTAINER}" mysql --ssl-mode=DISABLED -u "${DB_USERNAME}" -p"${DB_PASSWORD}" "${DB_DATABASE}" -e "SELECT 1;" > /dev/null 2>&1; then
                pass "authentication"
            else
                fail "authentication"
                increment_error
            fi
            ;;
        mssql)
            if docker exec "${MSSQL_CONTAINER}" /opt/mssql-tools18/bin/sqlcmd -S localhost -C -U "${DB_USERNAME}" -P "${DB_PASSWORD}" -d "${DB_DATABASE}" -Q "SELECT 1;" > /dev/null 2>&1 || \
               docker exec "${MSSQL_CONTAINER}" /opt/mssql-tools/bin/sqlcmd -S localhost -U "${DB_USERNAME}" -P "${DB_PASSWORD}" -d "${DB_DATABASE}" -Q "SELECT 1;" > /dev/null 2>&1; then
                pass "authentication"
            else
                fail "authentication"
                increment_error
            fi
            ;;
        sqlite)
            pass "authentication (file-based)"
            ;;
    esac
}

verify_schema() {
    local engine="$1"
    section "${engine}: schema"
    local expected_tables=(users companies products orders order_items)
    local missing=()
    for table in "${expected_tables[@]}"; do
        local sql
        case "${engine}" in
            postgres)
                sql="SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name = '${table}';"
                ;;
            mariadb|mysql)
                sql="SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = '${DB_DATABASE}' AND table_name = '${table}';"
                ;;
            mssql)
                sql="SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'dbo' AND table_name = '${table}';"
                ;;
            sqlite)
                sql="SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '${table}';"
                ;;
        esac
        local count
        count="$(engine_exec "${engine}" "${sql}" | tr -d '[:space:]' || true)"
        if [[ "${count}" != "1" ]]; then
            missing+=("${table}")
        fi
    done
    if [[ ${#missing[@]} -eq 0 ]]; then
        pass "schema tables present"
    else
        fail "missing tables: ${missing[*]}"
        increment_error
    fi
}

verify_crud() {
    local engine="$1"
    section "${engine}: CRUD"
    local table="verify_crud"
    local create_sql insert_sql select_sql update_sql delete_sql

    case "${engine}" in
        postgres)
            create_sql="CREATE TEMP TABLE ${table} (id SERIAL PRIMARY KEY, name TEXT);"
            insert_sql="INSERT INTO ${table} (name) VALUES ('test') RETURNING id;"
            select_sql="SELECT name FROM ${table} WHERE name = 'test';"
            update_sql="UPDATE ${table} SET name = 'updated' WHERE name = 'test';"
            delete_sql="DELETE FROM ${table} WHERE name = 'updated';"
            ;;
        mariadb|mysql)
            create_sql="CREATE TEMPORARY TABLE ${table} (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(100));"
            insert_sql="INSERT INTO ${table} (name) VALUES ('test'); SELECT LAST_INSERT_ID();"
            select_sql="SELECT name FROM ${table} WHERE name = 'test';"
            update_sql="UPDATE ${table} SET name = 'updated' WHERE name = 'test';"
            delete_sql="DELETE FROM ${table} WHERE name = 'updated';"
            ;;
        mssql)
            create_sql="CREATE TABLE #${table} (id INT IDENTITY(1,1) PRIMARY KEY, name NVARCHAR(100));"
            insert_sql="INSERT INTO #${table} (name) VALUES ('test'); SELECT SCOPE_IDENTITY();"
            select_sql="SELECT name FROM #${table} WHERE name = 'test';"
            update_sql="UPDATE #${table} SET name = 'updated' WHERE name = 'test';"
            delete_sql="DELETE FROM #${table} WHERE name = 'updated';"
            ;;
        sqlite)
            create_sql="CREATE TEMP TABLE ${table} (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT);"
            insert_sql="INSERT INTO ${table} (name) VALUES ('test');"
            select_sql="SELECT name FROM ${table} WHERE name = 'test';"
            update_sql="UPDATE ${table} SET name = 'updated' WHERE name = 'test';"
            delete_sql="DELETE FROM ${table} WHERE name = 'updated';"
            ;;
    esac

    engine_exec "${engine}" "${create_sql}${insert_sql}${select_sql}${update_sql}${delete_sql}" > /dev/null 2>&1
    pass "CRUD operations"
}

verify_transactions() {
    local engine="$1"
    section "${engine}: transactions"
    local sql

    case "${engine}" in
        postgres)
            sql="CREATE TEMP TABLE verify_tx (id SERIAL PRIMARY KEY, name TEXT); COMMIT; BEGIN; INSERT INTO verify_tx (name) VALUES ('rollback'); ROLLBACK; SELECT COUNT(*) FROM verify_tx WHERE name = 'rollback'; BEGIN; INSERT INTO verify_tx (name) VALUES ('commit'); COMMIT; SELECT COUNT(*) FROM verify_tx WHERE name = 'commit';"
            ;;
        mariadb|mysql)
            sql="CREATE TEMPORARY TABLE verify_tx (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(100)); START TRANSACTION; INSERT INTO verify_tx (name) VALUES ('rollback'); ROLLBACK; SELECT COUNT(*) FROM verify_tx WHERE name = 'rollback'; START TRANSACTION; INSERT INTO verify_tx (name) VALUES ('commit'); COMMIT; SELECT COUNT(*) FROM verify_tx WHERE name = 'commit';"
            ;;
        mssql)
            sql="CREATE TABLE #verify_tx (id INT IDENTITY(1,1) PRIMARY KEY, name NVARCHAR(100)); BEGIN TRANSACTION; INSERT INTO #verify_tx (name) VALUES ('rollback'); ROLLBACK; SELECT COUNT(*) FROM #verify_tx WHERE name = 'rollback'; BEGIN TRANSACTION; INSERT INTO #verify_tx (name) VALUES ('commit'); COMMIT; SELECT COUNT(*) FROM #verify_tx WHERE name = 'commit';"
            ;;
        sqlite)
            sql="CREATE TEMP TABLE verify_tx (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT); BEGIN TRANSACTION; INSERT INTO verify_tx (name) VALUES ('rollback'); ROLLBACK; SELECT COUNT(*) FROM verify_tx WHERE name = 'rollback'; BEGIN TRANSACTION; INSERT INTO verify_tx (name) VALUES ('commit'); COMMIT; SELECT COUNT(*) FROM verify_tx WHERE name = 'commit';"
            ;;
    esac

    local result
    result="$(engine_exec "${engine}" "${sql}" 2>/dev/null | tr -d '[:space:]' || true)"
    if [[ "${result}" == "01" ]]; then
        pass "transaction rollback/commit"
    else
        fail "transaction rollback/commit (got '${result}')"
        increment_error
    fi
}

verify_prepared() {
    local engine="$1"
    section "${engine}: prepared statements"

    if [[ "${engine}" == "sqlite" ]]; then
        pass "prepared statements (not supported by sqlite3 CLI)"
        return 0
    fi

    local sql
    case "${engine}" in
        postgres)
            sql="PREPARE verify_stmt(INT) AS SELECT username FROM users WHERE id = \$1; EXECUTE verify_stmt(1); DEALLOCATE verify_stmt;"
            ;;
        mariadb|mysql)
            sql="PREPARE verify_stmt FROM 'SELECT username FROM users WHERE id = ?'; SET @id = 1; EXECUTE verify_stmt USING @id; DEALLOCATE PREPARE verify_stmt;"
            ;;
        mssql)
            sql="EXEC sp_executesql N'SELECT username FROM users WHERE id = @id', N'@id int', @id = 1;"
            ;;
    esac

    if result="$(engine_exec "${engine}" "${sql}" 2>/dev/null | tr -d '[:space:]' || true)"; [[ "${result}" == *"alice"* ]]; then
        pass "prepared/parameterized query"
    else
        fail "prepared/parameterized query (got '${result}')"
        increment_error
    fi
}

verify_unicode() {
    local engine="$1"
    section "${engine}: Unicode"
    local sql
    case "${engine}" in
        postgres)
            sql="SELECT 'Héllo 世界' = 'Héllo 世界';"
            ;;
        mariadb|mysql)
            sql="SELECT 'Héllo 世界' = 'Héllo 世界';"
            ;;
        mssql)
            sql="SELECT CASE WHEN N'Héllo 世界' = N'Héllo 世界' THEN 1 ELSE 0 END;"
            ;;
        sqlite)
            sql="SELECT CASE WHEN 'Héllo 世界' = 'Héllo 世界' THEN 1 ELSE 0 END;"
            ;;
    esac
    local result
    result="$(engine_exec "${engine}" "${sql}" 2>/dev/null | tr -d '[:space:]' || true)"
    if [[ "${result}" == "1" || "${result}" == "t" || "${result}" == "true" ]]; then
        pass "Unicode comparison"
    else
        fail "Unicode comparison (got '${result}')"
        increment_error
    fi
}

verify_binary() {
    local engine="$1"
    section "${engine}: binary / blob"
    local sql
    case "${engine}" in
        postgres)
            sql="CREATE TEMP TABLE verify_bin (id SERIAL PRIMARY KEY, data BYTEA); INSERT INTO verify_bin (data) VALUES (decode('89504E47','hex')); SELECT encode(data,'hex') FROM verify_bin;"
            ;;
        mariadb|mysql)
            sql="CREATE TEMPORARY TABLE verify_bin (id INT AUTO_INCREMENT PRIMARY KEY, data BLOB); INSERT INTO verify_bin (data) VALUES (UNHEX('89504E47')); SELECT HEX(data) FROM verify_bin;"
            ;;
        mssql)
            sql="CREATE TABLE #verify_bin (id INT IDENTITY(1,1) PRIMARY KEY, data VARBINARY(MAX)); INSERT INTO #verify_bin (data) VALUES (0x89504E47); SELECT CONVERT(VARCHAR(MAX), data, 1) FROM #verify_bin;"
            ;;
        sqlite)
            sql="CREATE TEMP TABLE verify_bin (id INTEGER PRIMARY KEY AUTOINCREMENT, data BLOB); INSERT INTO verify_bin (data) VALUES (X'89504E47'); SELECT HEX(data) FROM verify_bin;"
            ;;
    esac
    local result
    result="$(engine_exec "${engine}" "${sql}" 2>/dev/null | tr '[:lower:]' '[:upper:]' | tr -d '[:space:]' || true)"
    if [[ "${result}" == *"89504E47"* ]]; then
        pass "binary / blob round-trip"
    else
        fail "binary / blob round-trip (got '${result}')"
        increment_error
    fi
}

verify_timestamps() {
    local engine="$1"
    section "${engine}: timestamps"
    local sql
    case "${engine}" in
        postgres)
            sql="CREATE TEMP TABLE verify_ts (id SERIAL PRIMARY KEY, ts TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP); INSERT INTO verify_ts DEFAULT VALUES; SELECT ts > '2000-01-01' FROM verify_ts;"
            ;;
        mariadb|mysql)
            sql="CREATE TEMPORARY TABLE verify_ts (id INT AUTO_INCREMENT PRIMARY KEY, ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP); INSERT INTO verify_ts () VALUES (); SELECT ts > '2000-01-01' FROM verify_ts;"
            ;;
        mssql)
            sql="CREATE TABLE #verify_ts (id INT IDENTITY(1,1) PRIMARY KEY, ts DATETIME2 DEFAULT GETDATE()); INSERT INTO #verify_ts DEFAULT VALUES; SELECT CASE WHEN ts > '2000-01-01' THEN 1 ELSE 0 END FROM #verify_ts;"
            ;;
        sqlite)
            sql="CREATE TEMP TABLE verify_ts (id INTEGER PRIMARY KEY AUTOINCREMENT, ts DATETIME DEFAULT CURRENT_TIMESTAMP); INSERT INTO verify_ts DEFAULT VALUES; SELECT ts > '2000-01-01' FROM verify_ts;"
            ;;
    esac
    local result
    result="$(engine_exec "${engine}" "${sql}" 2>/dev/null | tr -d '[:space:]' || true)"
    if [[ "${result}" == "1" || "${result}" == "t" || "${result}" == "true" ]]; then
        pass "timestamp default"
    else
        fail "timestamp default (got '${result}')"
        increment_error
    fi
}

verify_indexes() {
    local engine="$1"
    section "${engine}: indexes"
    local sql expected
    case "${engine}" in
        postgres)
            sql="SELECT COUNT(*) FROM pg_indexes WHERE schemaname = 'public' AND indexname LIKE 'idx_%';"
            expected="7"
            ;;
        mariadb|mysql)
            sql="SELECT COUNT(*) FROM information_schema.statistics WHERE table_schema = '${DB_DATABASE}' AND index_name LIKE 'idx_%';"
            expected="7"
            ;;
        mssql)
            sql="SELECT COUNT(*) FROM sys.indexes i JOIN sys.tables t ON t.object_id = i.object_id WHERE i.name LIKE 'idx_%';"
            expected="7"
            ;;
        sqlite)
            sql="SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%';"
            expected="7"
            ;;
    esac
    local result
    result="$(engine_exec "${engine}" "${sql}" 2>/dev/null | tr -d '[:space:]' || true)"
    if [[ "${result}" == "${expected}" ]]; then
        pass "indexes present"
    else
        fail "indexes present (expected ${expected}, got ${result})"
        increment_error
    fi
}

verify_views() {
    local engine="$1"
    section "${engine}: views"
    local sql
    case "${engine}" in
        postgres)
            sql="SELECT order_total FROM order_summary WHERE order_id = 1;"
            ;;
        mariadb|mysql)
            sql="SELECT order_total FROM order_summary WHERE order_id = 1;"
            ;;
        mssql)
            sql="SELECT order_total FROM order_summary WHERE order_id = 1;"
            ;;
        sqlite)
            sql="SELECT order_total FROM order_summary WHERE order_id = 1;"
            ;;
    esac
    local result
    result="$(engine_exec "${engine}" "${sql}" 2>/dev/null | tr -d '[:space:]' || true)"
    if [[ "${result}" == "39.97" || "${result}" == *"39.97"* ]]; then
        pass "order_summary view"
    else
        fail "order_summary view (expected 39.97, got ${result})"
        increment_error
    fi
}

verify_stored_procedures() {
    local engine="$1"
    section "${engine}: stored procedures / functions"

    if [[ "${engine}" == "sqlite" ]]; then
        pass "stored procedures (not supported)"
        return 0
    fi

    local sql expected
    case "${engine}" in
        postgres)
            sql="SELECT get_user_order_total(1);"
            expected="39.97"
            ;;
        mariadb|mysql)
            sql="CALL get_user_order_total(1);"
            expected="39.97"
            ;;
        mssql)
            sql="EXEC get_user_order_total @p_user_id = 1;"
            expected="39.97"
            ;;
    esac
    local result
    result="$(engine_exec "${engine}" "${sql}" 2>/dev/null | tr -d '[:space:]' || true)"
    if [[ "${result}" == *"${expected}"* ]]; then
        pass "stored procedure / function result"
    else
        fail "stored procedure / function result (expected ${expected}, got ${result})"
        increment_error
    fi
}

verify_engine() {
    local engine="$1"
    if ! engine_is_available "${engine}"; then
        section "${engine}: not running, skipping"
        return 0
    fi

    section "Verifying ${engine}"
    verify_connectivity "${engine}"
    verify_auth "${engine}"
    verify_schema "${engine}"
    verify_crud "${engine}"
    verify_transactions "${engine}"
    verify_prepared "${engine}"
    verify_unicode "${engine}"
    verify_binary "${engine}"
    verify_timestamps "${engine}"
    verify_indexes "${engine}"
    verify_views "${engine}"
    verify_stored_procedures "${engine}"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
    section "dbctx Database Lab Verification"

    local running=0
    for engine in $(list_engines); do
        if engine_is_available "${engine}"; then
            running=$((running + 1))
            verify_engine "${engine}"
        fi
    done

    if [[ ${running} -eq 0 ]]; then
        echo "ERROR: no database services are running. Run 'make up' first." >&2
        exit 1
    fi

    echo ""
    if [[ ${ERRORS} -eq 0 ]]; then
        echo "Verification complete: all checks passed."
        exit 0
    else
        echo "Verification failed: ${ERRORS} check(s) failed." >&2
        exit 1
    fi
}

main "$@"
