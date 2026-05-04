# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`wld` is a Rust CLI tool for controlling WLED smart lights. It supports device management, power/brightness control, and exposes an MCP (Model Context Protocol) server for AI agent integration.

## Build & Test Commands

```bash
cargo build                  # Build (use --release for release build)
cargo test                   # Run all tests (unit + integration)
cargo fmt --check            # Check formatting (cargo fmt to auto-fix)
cargo clippy --locked --workspace --all-features --all-targets -- -D warnings  # Lint (warnings are errors)
```

Pre-commit hooks enforce: fmt, cargo-check, clippy, codespell, trailing whitespace, and file format checks. Always use `--locked` flag for reproducible builds.

## Architecture

Three source files in `src/`:

- **`main.rs`** — CLI entry point using `clap` derive macros. Defines `Commands` enum (add, delete, ls, set-default, on, off, brightness, status, mcp). Contains `set_device_power()`, `set_device_brightness()`, and `get_device_status()` which use the `wled-json-api-library` crate for HTTP communication with WLED devices.

- **`config.rs`** — `Config` struct managing device name→IP mappings and default device. Persists to `~/.wld.toml` as TOML. First device added auto-becomes default; deleting default reassigns to next available. Contains unit tests.

- **`mcp.rs`** — MCP server implementation using `rmcp` crate (conditionally compiled behind `mcp` feature, enabled by default). Wraps the same `set_device_power`/`set_device_brightness`/`get_device_status` functions as async MCP tools via `tokio::task::spawn_blocking`. Serves over stdio transport.

## Testing

- **Unit tests** in `src/config.rs` — test Config struct methods directly
- **CLI integration tests** in `tests/cli_integration_tests.rs` — run the compiled binary with a temp `$HOME` directory for isolation (via `HOME` env var override)
- **MCP integration tests** in `tests/mcp_integration_tests.rs` — send JSON-RPC requests to the MCP server via bash scripts with timeouts; use `serde_json` for response validation

Integration tests that involve device control (on/off/brightness) will fail with network errors since no real WLED device exists — tests verify command parsing, not network connectivity.

## Device Resolution

When a command specifies `--device`/`-d`: first checks if it's a saved device name (returns its IP), otherwise treats the value as a direct IP address. If omitted, uses the default device.
