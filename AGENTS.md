# AGENTS.md — hepta_dbcli (dbcli)

Rust workspace. Single binary crate `hepta_dbcli` (package `polar-mysql`): a CLI + MCP server for MySQL/PolarDB-X/Oracle/GaussDB database introspection.

## Build & Dev Commands

```bash
# Build (debug, MySQL only)
cargo build

# Build with Oracle support
cargo build --features oracle

# Build release
cargo build --release -p polar-mysql --features oracle,gaussdb

# Format check
cargo fmt --all -- --check

# Clippy (requires libdbus-1-dev on Ubuntu)
sudo apt-get install -y libdbus-1-dev pkg-config
cargo clippy --all --all-targets

# Unit tests
cargo test --all

# Integration tests (require running MySQL)
HEPTA_DBCLI_TEST_URL=mysql://mcp:testpass@127.0.0.1:3306/testdb cargo test --all --features integration

# Oracle tests (unit)
cargo test --features oracle

# Oracle integration tests (require running Oracle)
POLARDB_ORACLE_TEST_URL=oracle://system:testpass@127.0.0.1:1521/FREEPDB1 cargo test --features "oracle,integration" -- oracle
```

**CI order matters**: `cargo fmt --check` → `cargo clippy` → `cargo test` (do NOT skip clippy).

## Architecture

```
dbcli/                          # Cargo workspace root
├── Cargo.toml                  # workspace: members = ["dbcli"]
├── dbcli/
│   ├── Cargo.toml              # bin crate, name = "hepta_dbcli" (package = "polar-mysql")
│   └── src/
│       ├── main.rs             # CLI arg parsing (clap), entrypoint + MCP server bootstrap
│       ├── cli.rs              # SQL execution, output rendering, read-only enforcement
│       ├── config.rs           # TOML config parsing, URL building (mysql:// + oracle://), keyring
│       ├── server.rs           # MCP server via rmcp: DbMcp with 6 tools (multi-backend)
│       ├── interactive.rs      # REPL mode: rustyline + SQL tokenizer (MySQL/Oracle aware)
│       ├── output.rs           # Table formatting (type mapping moved to backend/)
│       ├── queries.rs          # Legacy: MySQL SQL strings (still used by check command)
│       ├── connection.rs       # Legacy: MySQL connection helpers (still used by check command)
│       ├── logger.rs           # Tracing to ~/.local/share/hepta-dbcli/hepta-dbcli.log (daily)
│       └── backend/            # Multi-database abstraction layer
│           ├── mod.rs          # DbPool, DbConn, Dialect, BackendFactory traits + QueryResult
│           ├── error.rs        # DbError, DbErrorKind
│           ├── factory.rs      # BackendRegistry (scheme → factory routing)
│           ├── mysql/          # MySQL backend
│           │   ├── mod.rs      # MySqlFactory
│           │   ├── pool.rs     # MySqlPool (wraps mysql_async::Pool)
│           │   ├── conn.rs     # MySqlConn (wraps mysql_async::Conn)
│           │   ├── dialect.rs  # MySqlDialect (information_schema queries)
│           │   └── types.rs    # mysql_async::ColumnType → serde_json::Value
│           └── oracle/         # Oracle backend (feature-gated: --features oracle)
│               ├── mod.rs      # OracleFactory
│               ├── pool.rs     # OraclePool + oracle:// URL parser
│               ├── conn.rs     # OracleConn (wraps oracle_rs::Connection)
│               ├── dialect.rs  # OracleDialect (ALL_TABLES, SYS_CONTEXT, LISTAGG)
│               └── types.rs    # oracle_rs::Row → serde_json::Value
└── .github/workflows/
    ├── ci.yml                  # PR/push: fmt, clippy, test (MySQL 8 service container)
    └── release-build.yml       # Tag push: linux-x86_64, linux-arm64, windows-x86_64 (--features oracle)
```

**Dual mode**: The binary defaults to MCP server (`hepta_dbcli` with no subcommand). Use `hepta_dbcli cli` for one-shot SQL or `hepta_dbcli cli --interactive` for REPL.

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `mysql_async` 0.37 | Async MySQL driver (rustls-tls, ring crypto) |
| `oracle-rs` 0.1 | Pure Rust Oracle driver (optional, `--features oracle`) |
| `rmcp` 1.5 | MCP server framework (stdio transport) |
| `clap` 4 | CLI argument parsing |
| `keyring` 3 | OS keychain for password storage |
| `rustyline` 18 | Interactive REPL |
| `tracing` | Structured logging |
| `async-trait` 0.1 | Async trait support |

## Multi-Database Abstraction

The `backend/` module defines three traits that every database backend implements:

```rust
trait DbPool: Send + Sync { async fn acquire(&self) -> Result<Box<dyn DbConn + Send>>; }
trait DbConn: Send { async fn query(&mut self, sql: &str) -> Result<QueryResult>; fn dialect(&self) -> &dyn Dialect; }
trait Dialect: Send + Sync { fn database_info(&self) -> &str; fn add_limit(&self, sql: &str, n: usize) -> String; ... }
```

Adding a new database requires implementing these three traits + a `BackendFactory` — all consumers (server, cli, interactive) work without changes.

## Quirks & Gotchas

### Connection Config
- Config file: `~/.hepta-dbcli.toml` (TOML format, also reads legacy `~/.polardb-mysql.toml`)
- Env var override: `HEPTA_DBCLI_URL=mysql://user:pass@host:port/db`
- Multi-connection support via `[connections.NAME]` sections in TOML
- **`driver` field**: set `driver = "oracle"` for Oracle connections (defaults to `"mysql"`)
- Password stored in OS keychain (macOS Keychain, Linux Secret Service). Plaintext passwords in config are **auto-migrated** to keychain on first successful connection.

### Oracle Connection Example

```toml
default_connection = "dev"

[connections.dev]
host = "127.0.0.1"
user = "root"
password = "keyring"

[connections.ora]
driver = "oracle"
host = "oracle.internal"
port = 1521
user = "scott"
password = "keyring"
database = "FREEPDB1"
```

Or use URL directly:
```toml
[connections.ora]
url = "oracle://scott:tiger@oracle.internal:1521/FREEPDB1"
```

### URL Scheme Detection
The `BackendRegistry` routes connections by URL scheme:
- `mysql://...` → `MySqlFactory`
- `oracle://...` → `OracleFactory`
- Config field `driver = "oracle"` also routes to `OracleFactory`

### Password Flow
When a connection has `password = "keyring"` (sentinel value), the system reads from OS keychain. The migration rewrites the config file replacing the password with `"keyring"`. This is important: do NOT commit config files with plaintext passwords to git.

**Keyring key structure**: The OS keychain stores passwords under two identifiers:
- **Service**: `hepta-dbcli` (was `polar-mysql` in versions ≤ 0.2.7)
- **Account**: `{connection_name}#{8_hex_chars}` — the hex suffix is a djb2 hash of the canonical config file path, which disambiguates identically-named connections across different config files

On macOS, this appears in Keychain Access as `hepta-dbcli (dev#a3f9b2c1)`. On Linux, it's stored in the Secret Service.

**Migration from old keyring**: When `read_keyring_password` fails to find an entry under the new service name (`hepta-dbcli`), it falls back to the old service name (`polar-mysql`) with the old account format (`{user}/{name}`). On success, it auto-migrates the password to the new key and logs a warning if migration fails. This ensures smooth upgrades from ≤ 0.2.7.

### Naming Conventions

The project was renamed from `polar-mysql` to `hepta_dbcli`. All new code must use the current names:

| Category | Current | Legacy (≤ 0.2.7) |
|----------|---------|-------------------|
| Binary | `hepta_dbcli` | `polar-mysql` |
| Config file | `~/.hepta-dbcli.toml` | `~/.polardb-mysql.toml` |
| Env var (URL) | `HEPTA_DBCLI_URL` | `POLARDB_MYSQL_URL` |
| Env var (password) | `HEPTA_DBCLI_PASSWORD` | `POLARDB_MYSQL_PASSWORD` |
| Test env var | `HEPTA_DBCLI_TEST_URL` | `POLARDB_MYSQL_TEST_URL` |
| Keyring service | `hepta-dbcli` | `polar-mysql` |
| Keyring account | `{name}#{path_hash}` | `{user}/{name}` |
| Logger dir | `~/.local/share/hepta-dbcli/` | — |
| History dir | `~/.local/share/polar-mysql/history/` | — |

**Backward compatibility rules**:
- Config file: `~/.hepta-dbcli.toml` is tried first; `~/.polardb-mysql.toml` is tried as fallback
- Keyring: new service+account tried first; old service+account as fallback with auto-migration
- Oracle test env var `POLARDB_ORACLE_TEST_URL` has NOT been renamed (Oracle-specific, less widespread)
- Package name in Cargo.toml remains `polar-mysql` (separate from binary name)
- Crate name remains `polar_mysql` (Rust naming convention)

### Testing
- Unit tests are **inline** (`#[cfg(test)] mod tests { ... }`) in each source file — there is no `/tests/` directory.
- Integration tests live behind the `integration` feature flag in the same `#[cfg(test)]` blocks.
- MySQL integration tests require `HEPTA_DBCLI_TEST_URL` env var and a running MySQL instance.
- Oracle integration tests require `POLARDB_ORACLE_TEST_URL` env var and a running Oracle instance (Docker: `gvenzl/oracle-free:23-slim`).

### MCP Server
- Runs on **stdio** (not HTTP/WebSocket). Intended to be spawned by MCP clients (e.g., Claude, Cursor).
- All tool calls from MCP enforce **read-only**: only SELECT, EXPLAIN, SHOW, DESCRIBE, DESC are allowed (MySQL). Oracle only allows SELECT, EXPLAIN, WITH.
- `execute_query` tool appends `LIMIT N` (MySQL) or `FETCH FIRST N ROWS ONLY` (Oracle 12c+) — dialect-specific.
- `get_execution_plan` uses `EXPLAIN FORMAT=JSON` (MySQL) or `EXPLAIN PLAN ... DBMS_XPLAN` (Oracle).
- Connection pooling: connections are reused and recycled based on `connection_max_lifetime`.

### CI
- `libdbus-1-dev` and `pkg-config` are system dependencies for `clippy` and `test`. Without them, `cargo clippy` will fail on the `keyring` crate.
- Release builds on Windows link statically (`-C target-feature=+crt-static`).
- Release tags: `v*` (e.g. `v0.2.1`).
- Release binaries are built with `--features oracle` to include both MySQL and Oracle backends.

### Style Conventions
- Section headers use `// ─── ... ───` style.
- `pub(crate)` visibility throughout, not `pub`.
- `use` statements organized: std → external crates → crate modules.
- Rust edition 2021, workspace resolver v2.
