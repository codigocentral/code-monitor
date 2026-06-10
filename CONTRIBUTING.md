# Contributing to Code Monitor

Thanks for your interest in contributing! This document explains how to set up
your environment, the conventions we follow, and how to get your changes merged.

## Development setup

Prerequisites:

- **Rust** (stable toolchain) — install via [rustup](https://rustup.rs/)
- **protoc** (Protocol Buffers compiler) — required to build the gRPC layer
  - Debian/Ubuntu: `sudo apt install protobuf-compiler`
  - Windows: `choco install protoc`
  - macOS: `brew install protobuf`

Build everything:

```bash
cargo build --workspace
```

See [BUILD.md](BUILD.md) for platform-specific details (including Windows).

## Project layout

| Crate     | Path      | Description                                  |
|-----------|-----------|----------------------------------------------|
| `shared`  | `shared/` | Protobuf definitions, shared types, alerts, notifications |
| `monitor-server` | `server/` | gRPC server, system collectors (Docker, Postgres, MariaDB, systemd) |
| `monitor-client` | `client/` | TUI dashboard, CLI, SQLite storage |

## Running tests

```bash
cargo test --workspace
```

The CI runs tests on Linux and Windows. Please make sure your change passes on
your platform before opening a PR; cross-platform code should be guarded with
`#[cfg(...)]` where needed.

## Code style

These are enforced by CI:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features
```

Guidelines:

- No `.unwrap()` / `.expect()` in production code paths — propagate errors with
  `anyhow`/`thiserror`. They are fine inside `#[cfg(test)]`.
- Use `saturating_sub` (or checked arithmetic) when subtracting metrics counters.
- New features should come with unit tests in the same module (`mod tests`).

## Commit messages

Use short, imperative subject lines (e.g. `Add rate limiting to gRPC streams`).
Reference the tracker issue when applicable (e.g. `CMON-17`).

## Pull request checklist

- [ ] `cargo test --workspace` passes
- [ ] `cargo fmt --all` produces no diff
- [ ] `cargo clippy --all-targets --all-features` is clean
- [ ] New behavior is covered by tests
- [ ] `CHANGELOG.md` updated under `[Unreleased]`
- [ ] Docs updated (README, `docsx/`) if user-facing behavior changed

## Reporting issues

Open an issue describing:

1. What you expected to happen
2. What actually happened (logs help — run with `--log-level debug`)
3. OS/platform, and how you installed Code Monitor

Security issues: please do **not** open a public issue; contact the maintainer
directly instead.
