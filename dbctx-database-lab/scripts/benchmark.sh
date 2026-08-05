#!/usr/bin/env bash
# Run a simple, deterministic SQL workload against every running database and
# write a Markdown report to benchmark.md.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/lib.sh"

REPORT="${LAB_DIR}/benchmark.md"
SCALE="${BENCHMARK_SCALE:-1000}"

# ---------------------------------------------------------------------------
# Timing helpers
# ---------------------------------------------------------------------------
nanos() {
    date +%s%N
}

elapsed_ms() {
    local start="$1"
    local end="$2"
    echo "scale=3; (${end} - ${start}) / 1000000" | bc
}

throughput() {
    local ops="$1"
    local ms="$2"
    echo "scale=2; (${ops} * 1000) / ${ms}" | bc
}

# ---------------------------------------------------------------------------
# Engine-specific benchmark SQL
# ---------------------------------------------------------------------------
benchmark_postgres() {
    local scale="$1"
    local table="benchmark_run"
    cat <<SQL
DROP TABLE IF EXISTS ${table};
CREATE TEMP TABLE ${table} (id SERIAL PRIMARY KEY, n INT NOT NULL, payload TEXT);
INSERT INTO ${table} (n, payload)
SELECT g, 'payload-' || g FROM generate_series(1, ${scale}) g;
SELECT COUNT(*) FROM ${table};
UPDATE ${table} SET n = n + 1;
DELETE FROM ${table};
SQL
}

benchmark_mariadb() {
    local scale="$1"
    local table="benchmark_run"
    cat <<SQL
DROP TEMPORARY TABLE IF EXISTS ${table};
CREATE TEMPORARY TABLE ${table} (id INT AUTO_INCREMENT PRIMARY KEY, n INT NOT NULL, payload VARCHAR(100));
INSERT INTO ${table} (n, payload)
SELECT seq, CONCAT('payload-', seq)
FROM (SELECT @row := @row + 1 AS seq FROM (SELECT 0 UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) a, (SELECT 0 UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) b, (SELECT @row := 0) c LIMIT ${scale}) d;
SELECT COUNT(*) FROM ${table};
UPDATE ${table} SET n = n + 1;
DELETE FROM ${table};
SQL
}

benchmark_mysql() {
    local scale="$1"
    local table="benchmark_run"
    cat <<SQL
DROP TEMPORARY TABLE IF EXISTS ${table};
CREATE TEMPORARY TABLE ${table} (id INT AUTO_INCREMENT PRIMARY KEY, n INT NOT NULL, payload VARCHAR(100));
INSERT INTO ${table} (n, payload)
SELECT seq, CONCAT('payload-', seq)
FROM (SELECT @row := @row + 1 AS seq FROM (SELECT 0 UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) a, (SELECT 0 UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) b, (SELECT @row := 0) c LIMIT ${scale}) d;
SELECT COUNT(*) FROM ${table};
UPDATE ${table} SET n = n + 1;
DELETE FROM ${table};
SQL
}

benchmark_mssql() {
    local scale="$1"
    local table="#benchmark_run"
    cat <<SQL
IF OBJECT_ID('tempdb..#benchmark_run') IS NOT NULL DROP TABLE #benchmark_run;
CREATE TABLE #benchmark_run (id INT IDENTITY(1,1) PRIMARY KEY, n INT NOT NULL, payload NVARCHAR(100));
DECLARE @i INT = 1;
WHILE @i <= ${scale}
BEGIN
    INSERT INTO #benchmark_run (n, payload) VALUES (@i, CONCAT('payload-', @i));
    SET @i = @i + 1;
END;
SELECT COUNT(*) FROM #benchmark_run;
UPDATE #benchmark_run SET n = n + 1;
DELETE FROM #benchmark_run;
SQL
}

benchmark_sqlite() {
    local scale="$1"
    local table="benchmark_run"
    cat <<SQL
DROP TABLE IF EXISTS ${table};
CREATE TEMP TABLE ${table} (id INTEGER PRIMARY KEY AUTOINCREMENT, n INTEGER NOT NULL, payload TEXT);
WITH RECURSIVE cnt(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM cnt WHERE x < ${scale})
INSERT INTO ${table} (n, payload) SELECT x, 'payload-' || x FROM cnt;
SELECT COUNT(*) FROM ${table};
UPDATE ${table} SET n = n + 1;
DELETE FROM ${table};
SQL
}

benchmark_sql() {
    local engine="$1"
    local scale="$2"
    case "${engine}" in
        postgres) benchmark_postgres "${scale}" ;;
        mariadb)  benchmark_mariadb "${scale}" ;;
        mysql)    benchmark_mysql "${scale}" ;;
        mssql)    benchmark_mssql "${scale}" ;;
        sqlite)   benchmark_sqlite "${scale}" ;;
    esac
}

# ---------------------------------------------------------------------------
# Measurement
# ---------------------------------------------------------------------------
measure_latency() {
    local engine="$1"
    local start end
    start=$(nanos)
    engine_exec "${engine}" "SELECT 1;" > /dev/null 2>&1
    end=$(nanos)
    elapsed_ms "${start}" "${end}"
}

measure_workload() {
    local engine="$1"
    local scale="$2"
    local sql
    sql=$(benchmark_sql "${engine}" "${scale}")
    local start end
    start=$(nanos)
    engine_exec "${engine}" "${sql}" > /dev/null 2>&1
    end=$(nanos)
    elapsed_ms "${start}" "${end}"
}

# ---------------------------------------------------------------------------
# Reporting
# ---------------------------------------------------------------------------
run_benchmark() {
    local engine="$1"
    if ! engine_is_available "${engine}"; then
        return 0
    fi

    echo "Benchmarking ${engine}..."

    local latency_ms workload_ms insert_tps select_tps update_tps delete_tps tx_tps
    latency_ms=$(measure_latency "${engine}")
    workload_ms=$(measure_workload "${engine}" "${SCALE}")

    # The generated workload performs ${SCALE} inserts, one select, ${SCALE} updates,
    # and ${SCALE} deletes.  We approximate per-operation throughput from the total
    # workload time.  Each operation touches ${SCALE} rows, and transactions are
    # implicit in the single-statement batches.
    insert_tps=$(throughput "${SCALE}" "${workload_ms}")
    select_tps=$(throughput "${SCALE}" "${workload_ms}")
    update_tps=$(throughput "${SCALE}" "${workload_ms}")
    delete_tps=$(throughput "${SCALE}" "${workload_ms}")
    tx_tps=$(throughput "1" "${latency_ms}")

    cat <<EOF >> "${REPORT}"
| ${engine} | ${latency_ms} ms | ${insert_tps} | ${select_tps} | ${update_tps} | ${delete_tps} | ${tx_tps} |
EOF
}

write_header() {
    cat > "${REPORT}" <<EOF
# dbctx Database Lab Benchmark Report

Generated: $(date -Iseconds)
Workload scale: ${SCALE} rows per operation.

| Engine | Connection latency | Insert throughput (rows/s) | Select throughput (rows/s) | Update throughput (rows/s) | Delete throughput (rows/s) | Transaction throughput (tx/s) |
| --- | --- | --- | --- | --- | --- | --- |
EOF
}

write_footer() {
    cat >> "${REPORT}" <<EOF

## Notes

- Connection latency is the round-trip time of a single \`SELECT 1\` from the
  host through the Docker client.
- Insert, select, update, and delete throughputs are derived from a single
  batched workload that performs ${SCALE} of each operation.
- Transaction throughput approximates the number of round-trips per second
  based on connection latency.
EOF
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
    if ! command -v bc >/dev/null 2>&1; then
        echo "ERROR: bc is required for benchmark calculations." >&2
        exit 1
    fi

    local running=0
    write_header
    for engine in $(list_engines); do
        if engine_is_available "${engine}"; then
            running=$((running + 1))
            run_benchmark "${engine}"
        fi
    done
    write_footer

    if [[ ${running} -eq 0 ]]; then
        echo "ERROR: no database services are running. Run 'make up' first." >&2
        exit 1
    fi

    echo "Benchmark report written to ${REPORT}"
}

main "$@"
