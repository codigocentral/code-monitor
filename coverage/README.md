# Unit coverage campaign — session 13

Run from the isolated worktree after `source coverage/environment.sh`.
Artifacts belong to `resultados/codigocentral-code-monitor`, outside Git.

## Scope

Three Rust crates (`shared`, `monitor-server`, `monitor-client`), the Rust build
script, ten Bash scripts, and four PowerShell scripts remain in scope. Initial
inventory and source hashes are recorded in `scope-manifest.json`. The original
`main` checkout was clean and remains untouched. Generated protobuf output,
third-party dependencies, and declarative/documentation files are not assigned
runtime coverage percentages. Platform-gated Linux Rust code must be accounted
for separately before whole-repository completion.

## Bash

```sh
bash coverage/run-shell.sh /absolute/evidence/bash
ruby coverage/check-bash-collector.rb
```

Uses existing isolated Bash 5.3, Ruby 2.6.10, Bashcov 1.8.2 and SimpleCov 0.15.1.
All ten tracked scripts are included even when unexecuted. Side-effecting
commands are function doubles: sudo, service managers, accounts, Docker,
Postgres, HTTP, Git (including simulated push), archive/crypto/install/file
commands. Real writes are limited to owned scratch fixtures on LaCie. GNU sed
syntax unavailable on macOS has an explicit collaborator double. No service,
account, database grant, Git publication, certificate key deployment or network
provisioning occurs. SQL payloads, service configuration and certificate SAN
configuration are asserted as captured outputs. The initial scripts contain
some error paths inside command substitution that still return success from
`install.sh`; tests preserve and document their actual existing exit behavior.

Four scripts accept `CODE_MONITOR_PREFIX` for filesystem staging. Empty retains
all original host paths. Pipeline continuation joins change formatting only.

The old collector's greedy heredoc regexp incorrectly marked code between
multiple documents as executed. `bashcov-lexer.rb` bounds each heredoc at its
first complete delimiter (including legal spacing), and recognizes non-executable
empty case arms and redirected closing braces. A regression test preserves zero
for an unexecuted statement between two documents. It also validates the fixture
syntax with Bash. No executable statement is excluded and no hit count is
manufactured. Initial raw legacy baseline is preserved; final reports use the
corrected lexer. Bashcov supports line coverage only; branches/functions have
no supported counters and receive no percentage. Raw native LCOV, line arrays,
TAP, JUnit and command metadata are retained.

## PowerShell

```sh
DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1 python3 coverage/record-command.py \
  /absolute/evidence/powershell-command \
  "$campaign/cache/session-13/powershell/runtime/pwsh" -NoLogo -NoProfile \
  -File coverage/run-powershell.ps1 -OutputDirectory /absolute/evidence/powershell
```

Uses portable PowerShell 7.6.7 and Pester 5.7.1 entirely on LaCie. Pester collects
all four production scripts, with target 100 and exact executed/analyzed command
checks. Reports are native Cobertura, native JUnit and full Pester summary.
Pester measures commands and lines; this report exposes no branch/function
counters. Zero branch denominators are not a branch-coverage claim.

`install.ps1` has a sourceable `Install-CodeMonitor` entry and small platform
provider adapters. Direct script execution retains the same parameters and
default behavior. Tests supply fake identity/principal/environment types and
mock HTTP, filesystem, Windows task/service commands and NSSM. No machine PATH,
service, scheduled task, user or real provider is changed. The public entrypoint
is tested with an early filesystem failure double as well.

The baseline no-op Pester case verifies instrumentation while executing zero
production commands. Its one collector probe is not an existing production test.

## Rust

All dependency caches, generated files, target outputs and temporary files use
session-13 folders on LaCie. Jobs=1 and offline/locked builds reuse the preserved
cache. LLVM 22 tools match Rust 1.95.0; the toolchain itself is reused. The host
Homebrew protoc is broken (missing abseil dylib); an official standalone protoc
33.2 is selected for this session only. Download metadata/checksums are preserved
in the tools cache. No global installation was changed.

The original unit suite failed to compile because the referenced public PEM
fixture was absent. `server/tests/fixtures/sample-cert.pem` is a generated,
self-signed test certificate; its private key is outside the repository. The
ignore exception admits this public fixture only.

Build instrumentation environment is recorded in `setup/llvm-env.sh`. After a
successful instrumented `cargo test --workspace --no-run --offline --locked
--message-format=json`, run:

```sh
python3 coverage/run-rust-units.py /absolute/evidence/rust /absolute/compile/run.log
```

The runner inventories every harness test and separates known real-system,
loopback HTTP/gRPC, and SQLite integration cases. They remain present and are
not used to increase unit coverage. Their production files remain in the LLVM
objects. Raw harness output, exact selection/reasons, JUnit, fresh unit-only
profraw/profdata, LCOV and native LLVM JSON are retained. Build-execution
profiles and test-listing profiles are not merged into unit evidence. The build
script and Linux-only code require separate unit evidence before completion.
Rust coverage is not complete until all supported counters and scope checks pass.
