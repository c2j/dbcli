# AGENTS.md — polar-mysql (dbcli)

Rust workspace. Single binary crate `polar-mysql`: a CLI + MCP server for MySQL/PolarDB-X introspection.

## Build & Dev Commands

```bash
# Build (debug)
cargo build

# Build release
cargo build --release -p polar-mysql

# Format check
cargo fmt --all -- --check

# Clippy (requires libdbus-1-dev on Ubuntu)
sudo apt-get install -y libdbus-1-dev pkg-config
cargo clippy --all --all-targets

# Unit tests
cargo test --all

# Integration tests (require running MySQL)
POLARDB_MYSQL_TEST_URL=mysql://mcp:testpass@127.0.0.1:3306/testdb cargo test --all --features integration
```

**CI order matters**: `cargo fmt --check` → `cargo clippy` → `cargo test` (do NOT skip clippy).

## Architecture

```
dbcli/                          # Cargo workspace root
├── Cargo.toml                  # workspace: members = ["polar-mysql"]
├── polar-mysql/
│   ├── Cargo.toml              # bin crate, name = "polar-mysql"
│   └── src/
│       ├── main.rs             # CLI arg parsing (clap), entrypoint + MCP server bootstrap
│       ├── cli.rs              # SQL execution, output rendering, read-only enforcement
│       ├── config.rs           # TOML config parsing, URL building, keyring, password migration
│       ├── connection.rs       # MySQL connection pool, timeout config, timeout_action
│       ├── server.rs           # MCP server via rmcp: tools (query, metadata, explain, etc.)
│       ├── interactive.rs      # REPL mode: rustyline + SQL tokenizer (MySQL syntax aware)
│       ├── output.rs           # Table formatting, column type → JSON value conversion
│       ├── queries.rs          # Static SQL strings for introspection queries
│       └── logger.rs           # Tracing to ~/.local/share/polar-mysql/polar-mysql.log (daily)
└── .github/workflows/
    ├── ci.yml                  # PR/push: fmt, clippy, test (MySQL 8 service container)
    └── release-build.yml       # Tag push: linux-x86_64, linux-arm64, windows-x86_64
```

**Dual mode**: The binary defaults to MCP server (`polar-mysql` with no subcommand). Use `polar-mysql cli` for one-shot SQL or `polar-mysql cli --interactive` for REPL.

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `mysql_async` 0.37 | Async MySQL driver (rustls-tls, ring crypto) |
| `rmcp` 1.5 | MCP server framework (stdio transport) |
| `clap` 4 | CLI argument parsing |
| `keyring` 3 | OS keychain for password storage |
| `rustyline` 18 | Interactive REPL |
| `tracing` | Structured logging |

## Quirks & Gotchas

### Connection Config
- Config file: `~/.polardb-mysql.toml` (TOML format)
- Env var override: `POLARDB_MYSQL_URL=mysql://user:pass@host:port/db`
- Multi-connection support via `[connections.NAME]` sections in TOML
- Password stored in OS keychain (macOS Keychain, Linux Secret Service). Plaintext passwords in config are **auto-migrated** to keychain on first successful connection.

### Password Flow
When a connection has `password = "keyring"` (sentinel value), the system reads from OS keychain. The migration rewrites the config file replacing the password with `"keyring"`. This is important: do NOT commit config files with plaintext passwords to git.

### Testing
- Unit tests are **inline** (`#[cfg(test)] mod tests { ... }`) in each source file — there is no `/tests/` directory.
- Integration tests live behind the `integration` feature flag in the same `#[cfg(test)]` blocks.
- Integration tests require `POLARDB_MYSQL_TEST_URL` env var and a running MySQL instance.

### MCP Server
- Runs on **stdio** (not HTTP/WebSocket). Intended to be spawned by MCP clients (e.g., Claude, Cursor).
- All tool calls from MCP enforce **read-only**: only SELECT, EXPLAIN, SHOW, DESCRIBE, DESC are allowed.
- `execute_query` tool appends `LIMIT 1000` by default (configurable with `max_rows`, capped at 10000).
- Connection pooling: connections are reused and recycled based on `connection_max_lifetime`.

### CI
- `libdbus-1-dev` and `pkg-config` are system dependencies for `clippy` and `test`. Without them, `cargo clippy` will fail on the `keyring` crate.
- Release builds on Windows link statically (`-C target-feature=+crt-static`).
- Release tags: `polar-mysql-v*` or `v*`.

### Style Conventions
- Section headers use `// ─── ... ───` style.
- `pub(crate)` visibility throughout, not `pub`.
- `use` statements organized: std → external crates → crate modules.
- Rust edition 2021, workspace resolver v2.
