#!/usr/bin/env bash
set -e
echo "Waiting for services..."
docker compose up -d
docker compose ps
echo
echo "Verify manually or invoke dbctx against:"
echo " PostgreSQL : localhost:${POSTGRES_PORT:-5432}"
echo " MariaDB    : localhost:${MARIADB_PORT:-3306}"
echo " MySQL      : localhost:${MYSQL_PORT:-3307}"
echo " SQL Server : localhost:${MSSQL_PORT:-1433}"
echo " SQLite     : sqlite/app.db"
