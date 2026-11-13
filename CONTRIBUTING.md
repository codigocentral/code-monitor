# Contributing to Code Monitor

Thanks for your interest in contributing!

## Setup

You need the stable Rust toolchain ([rustup](https://rustup.rs/)) and `protoc`
(Protocol Buffers compiler):

- Debian/Ubuntu: `sudo apt install protobuf-compiler`
- macOS: `brew install protobuf`
- Windows: `choco install protoc`

See [BUILD.md](BUILD.md) for platform details and [AGENTS.md](AGENTS.md) for
the architecture, crate layout and conventions.

## Before opening a pull request

These are the checks CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

- No `.unwrap()` / `.expect()` in production paths; collectors must never
  panic. Both are fine inside `#[cfg(test)]`.
- New behavior comes with unit tests in the same module.
- Update `CHANGELOG.md` under `[Unreleased]` and the docs in `docs/` when
  user-facing behavior changes.
- Use short, imperative commit subjects (Conventional Commits welcome, e.g.
  `fix(tls): ...`).
- Never commit real hostnames, IP addresses, tokens or credentials. Use the
  documentation ranges (`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`)
  and `example.com` in examples and tests.

## Reporting issues

Describe what you expected, what happened (run with `--log-level debug`), and
your OS and install method. For security issues, see [SECURITY.md](SECURITY.md).
