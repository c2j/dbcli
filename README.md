# hepta_dbcli

CLI and MCP server for MySQL/PolarDB-X/Oracle database introspection.

## Features

- **MCP server** — spawn as a Model Context Protocol server for AI tools (Claude, Cursor, etc.) to query databases with read-only safety enforcement
- **Multi-database** — supports MySQL, PolarDB-X, and Oracle (single binary with `--features oracle`)
- **One-shot CLI** — execute SQL from command line, file, or stdin with multiple output formats
- **Interactive REPL** — database-aware SQL prompt with multi-line editing, history, and dot commands
- **Multi-connection** — manage multiple database connections with per-connection timeout config
- **OS keychain** — passwords stored in macOS Keychain or Linux Secret Service, with automatic migration from plaintext config files

## Installation

### Download binary

Prebuilt binaries for Linux (x86_64, arm64) and Windows (x86_64) from [GitHub Releases](https://github.com/YOUR_ORG/dbcli/releases).

### Build from source

```bash
git clone https://github.com/YOUR_ORG/dbcli.git
cd dbcli
cargo build --release -p polar-mysql
# binary at: target/release/hepta_dbcli
```

## Configuration

### Config file

Create `~/.polardb-mysql.toml`:

```toml
# Single connection (sections below are the defaults)
host = "127.0.0.1"
port = 3306
user = "root"
password = "your-password"
database = "mysql"
```

On first successful connection, the password is automatically migrated to your OS keychain and the file is rewritten with `password = "keyring"`.

### Multi-connection

```toml
default_connection = "dev"

[connections.prod]
host = "prod-db.example.com"
user = "readonly"
password = "keyring"

[connections.dev]
host = "127.0.0.1"
user = "root"
password = "keyring"

# Oracle connection
[connections.ora]
driver = "oracle"
host = "oracle.internal"
port = 1521
user = "scott"
password = "keyring"
database = "FREEPDB1"
```

### Environment variable

```bash
export POLARDB_MYSQL_URL="mysql://user:password@host:port/database"

# Oracle via URL
export POLARDB_MYSQL_URL="oracle://scott:tiger@host:1521/FREEPDB1"
```

### Timeout settings

```toml
# Global defaults (per-connection overrides in [connections.NAME])
statement_timeout = "30s"       # Per-query max execution time
connection_max_lifetime = "1h"  # Recycle connection after this duration
```

Supported units: `ms`, `s`, `min`, `h`, or plain seconds.

## Usage

### MCP server (default)

```bash
hepta_dbcli
hepta_dbcli --config /path/to/config.toml
```

Runs on stdio. Intended to be spawned by MCP clients. All queries are read-only (SELECT, EXPLAIN, SHOW, DESCRIBE/DESC only).

### One-shot SQL

```bash
# From command line
hepta_dbcli cli --sql "SELECT version()"

# From file
hepta_dbcli cli --file query.sql

# From stdin
echo "SHOW TABLES" | hepta_dbcli cli

# Custom output format
hepta_dbcli cli --sql "SELECT * FROM users" --format json
hepta_dbcli cli --sql "SELECT * FROM users" --format csv
hepta_dbcli cli --sql "SELECT * FROM users" --format vertical

# Target a specific connection
hepta_dbcli cli --name prod --sql "SELECT count(*) FROM orders"
```

### Interactive REPL

```bash
hepta_dbcli cli --interactive
hepta_dbcli cli -i --name dev
```

REPL commands:
- `.help` / `?` — show help
- `.connect [name]` — switch connection
- `.history` — show SQL execution history
- `.output [file]` — redirect SQL output to file
- `.save <file> [format]` — save last result
- `.clear` / `.cls` — clear screen
- `.exit` / `.quit` — exit

Output formats: `table` (default), `json`, `vertical`, `csv`.

End SQL statements with `;` + Enter to execute. Multi-line with incomplete statements is supported.

### Test connection

```bash
# Basic connectivity check
hepta_dbcli check

# Verbose: shows server version, user, charset, TLS modes
hepta_dbcli check --verbose

# Check specific connection
hepta_dbcli check --name prod
```

### Store password

```bash
hepta_dbcli store-password
hepta_dbcli store-password --name prod
```

Prompts for password and stores it in the OS keychain.

## MCP Tools

When running as MCP server, the following tools are available:

| Tool | Description |
|------|-------------|
| `get_database_info` | Server version, current user, charset, OS |
| `list_tables` | All user tables/views with engine, row count, size |
| `get_table_metadata` | Column types, nullability, defaults, indexes |
| `execute_query` | Read-only SELECT/EXPLAIN/SHOW/DESCRIBE (appends `LIMIT N` or `FETCH FIRST N ROWS ONLY`; Oracle allows SELECT/EXPLAIN/WITH) |
| `get_execution_plan` | EXPLAIN or EXPLAIN ANALYZE with TEXT/JSON format (Oracle: EXPLAIN PLAN + DBMS_XPLAN) |
| `list_connections` | List all configured connections and their status |

## Development

```bash
# Build (MySQL only)
cargo build

# Build with Oracle support
cargo build --features oracle

# Release (includes Oracle)
cargo build --release -p polar-mysql --features oracle

# Format check
cargo fmt --all -- --check

# Lint (Ubuntu: apt-get install libdbus-1-dev pkg-config first)
cargo clippy --all --all-targets

# Unit tests
cargo test --all

# Integration tests (requires running MySQL instance)
POLARDB_MYSQL_TEST_URL=mysql://mcp:testpass@127.0.0.1:3306/testdb cargo test --all --features integration

# Oracle unit tests
cargo test --features oracle

# Oracle integration tests (requires running Oracle)
POLARDB_ORACLE_TEST_URL=oracle://system:testpass@127.0.0.1:1521/FREEPDB1 cargo test --features "oracle,integration" -- oracle
```

CI enforces: `cargo fmt --check` → `cargo clippy` → `cargo test` (in that order).

### Running MySQL for integration tests

```bash
docker run -d --name mysql-test \
  -e MYSQL_ROOT_PASSWORD=testpass \
  -e MYSQL_DATABASE=testdb \
  -p 3306:3306 \
  mysql:8

# Create test user
docker exec mysql-test mysql -u root -ptestpass -e "
  CREATE USER 'mcp'@'%' IDENTIFIED WITH caching_sha2_password BY 'testpass';
  GRANT ALL PRIVILEGES ON *.* TO 'mcp'@'%';
  FLUSH PRIVILEGES;
"

# Run tests
POLARDB_MYSQL_TEST_URL=mysql://mcp:testpass@127.0.0.1:3306/testdb cargo test --all --features integration
```

### Running Oracle for integration tests

```bash
docker run -d --name oracle-test -p 1521:1521 \
  -e ORACLE_PASSWORD=testpass \
  gvenzl/oracle-free:23-slim

# Wait for Oracle to be ready (can take ~30s)
docker logs -f oracle-test

# Run tests
POLARDB_ORACLE_TEST_URL=oracle://system:testpass@127.0.0.1:1521/FREEPDB1 \
  cargo test --features "oracle,integration" -- oracle
```

## License

MIT OR Apache-2.0
