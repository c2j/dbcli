# Integration Test Configurations

## Quick Start

```bash
# Start databases
docker compose -f tests/docker-compose.yml up -d

# Wait for Oracle (can take ~30s on first start)
docker logs -f hepta-oracle-test

# Create MySQL test user
docker exec hepta-mysql-test mysql -u root -ptestpass -e "
  CREATE USER 'mcp'@'%' IDENTIFIED WITH caching_sha2_password BY 'testpass';
  GRANT ALL PRIVILEGES ON *.* TO 'mcp'@'%';
  FLUSH PRIVILEGES;
"
```

## Config Files

| File | Backend | Connection Name |
|------|---------|-----------------|
| `docker-mysql.toml` | MySQL only | `local` |
| `docker-oracle.toml` | Oracle only | `oracle` |
| `docker-all.toml` | MySQL + Oracle | `mysql` (default), `oracle` |

## Usage Examples

### One-shot SQL

```bash
# MySQL
cargo run -- --config tests/docker-mysql.toml cli --sql "SELECT VERSION()"

# Oracle
cargo run -- --config tests/docker-oracle.toml cli --sql "SELECT * FROM dual"
cargo run --features oracle -- --config tests/docker-oracle.toml cli --sql "SELECT banner FROM v\$version"

# Multi-backend (list all connections)
cargo run --features oracle -- --config tests/docker-all.toml

# Target specific connection
cargo run --features oracle -- --config tests/docker-all.toml cli --name oracle --sql "SELECT table_name FROM all_tables WHERE ROWNUM <= 10"
```

### MCP Server

```bash
# Single-backend (MySQL)
cargo run --features oracle -- --config tests/docker-all.toml
```

### Integration Tests

```bash
# MySQL
POLARDB_MYSQL_TEST_URL=mysql://mcp:testpass@127.0.0.1:3306/testdb cargo test --features integration

# Oracle
POLARDB_ORACLE_TEST_URL=oracle://system:testpass@127.0.0.1:1521/FREEPDB1 cargo test --features "oracle,integration" -- oracle
```

## Cleanup

```bash
docker compose -f tests/docker-compose.yml down -v
```
