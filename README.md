# polar-mysql

CLI and MCP server for MySQL/PolarDB-X database introspection.

## Features

- **MCP server** — spawn as a Model Context Protocol server for AI tools (Claude, Cursor, etc.) to query MySQL/PolarDB-X databases with read-only safety enforcement
- **One-shot CLI** — execute SQL from command line, file, or stdin with multiple output formats
- **Interactive REPL** — MySQL-aware SQL prompt with multi-line editing, history, and dot commands
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
# binary at: target/release/polar-mysql
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
```

### Environment variable

```bash
export POLARDB_MYSQL_URL="mysql://user:password@host:port/database"
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
polar-mysql
polar-mysql --config /path/to/config.toml
```

Runs on stdio. Intended to be spawned by MCP clients. All queries are read-only (SELECT, EXPLAIN, SHOW, DESCRIBE/DESC only).

### One-shot SQL

```bash
# From command line
polar-mysql cli --sql "SELECT version()"

# From file
polar-mysql cli --file query.sql

# From stdin
echo "SHOW TABLES" | polar-mysql cli

# Custom output format
polar-mysql cli --sql "SELECT * FROM users" --format json
polar-mysql cli --sql "SELECT * FROM users" --format csv
polar-mysql cli --sql "SELECT * FROM users" --format vertical

# Target a specific connection
polar-mysql cli --name prod --sql "SELECT count(*) FROM orders"
```

### Interactive REPL

```bash
polar-mysql cli --interactive
polar-mysql cli -i --name dev
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
polar-mysql check

# Verbose: shows server version, user, charset, TLS modes
polar-mysql check --verbose

# Check specific connection
polar-mysql check --name prod
```

### Store password

```bash
polar-mysql store-password
polar-mysql store-password --name prod
```

Prompts for password and stores it in the OS keychain.

## MCP Tools

When running as MCP server, the following tools are available:

| Tool | Description |
|------|-------------|
| `get_database_info` | Server version, current user, charset, OS |
| `list_tables` | All user tables/views with engine, row count, size |
| `get_table_metadata` | Column types, nullability, defaults, indexes |
| `execute_query` | Read-only SELECT/EXPLAIN/SHOW/DESCRIBE (appends `LIMIT 1000` by default) |
| `get_execution_plan` | EXPLAIN or EXPLAIN ANALYZE with TEXT/JSON format |
| `list_connections` | List all configured connections and their status |

## Development

```bash
# Build
cargo build

# Release
cargo build --release -p polar-mysql

# Format check
cargo fmt --all -- --check

# Lint (Ubuntu: apt-get install libdbus-1-dev pkg-config first)
cargo clippy --all --all-targets

# Unit tests
cargo test --all

# Integration tests (requires running MySQL instance)
POLARDB_MYSQL_TEST_URL=mysql://mcp:testpass@127.0.0.1:3306/testdb cargo test --all --features integration
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

## License

MIT OR Apache-2.0
