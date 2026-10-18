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
successful instrumented `cargo test --workspace
--features monitor-client/unit-doubles,monitor-server/unit-doubles
--no-run --offline --locked --message-format=json`, run:

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

### Corrected initial selection

The first client selection indirectly opened SQLite through
`DashboardApp::new`, despite excluding the explicit `storage::tests` module.
Those historical client/aggregate numbers are mixed evidence and are not valid
unit-only coverage. `baseline/rust-strict` corrects the selection using the
unchanged original binaries, whose SHA-256, timestamps, original build command
and source revision are recorded in `binary-provenance.json`. It excludes all
original dashboard cases, the native SQLite/gRPC cases and Docker smoke probes.
The corrected Rust baseline is 451 passing tests, 2,211/8,847 production lines,
276/815 functions and 3,085/13,430 regions. The historical mixed reports and the
initial flaky alert-clock failure are retained rather than overwritten.

### Production scope projection

Native LLVM JSON and LCOV retain inline Rust tests and macro-attributed standard
library points. `rust-production-scope.py` additionally emits `production.info`
and a proof: source hashes, exact syn-parsed `#[cfg(test)]` ranges, explicit
external standard-library/test-tooling origins, every removed point and every
retained uncovered production line. Production hit counts are never changed.
The tool reconstructs LLVM's native generic function/region groups and requires
exact agreement for every original file before applying the production scope.
The include of the actual `shared/build.rs` is canonicalized back to production,
so its points cannot disappear because the caller lives under `coverage/`.
Stable Rust/LLVM does not expose branch or MC/DC counters for this build; zero
unsupported denominators are not a coverage percentage. Regions, functions and
lines remain exact measured counters.

`shared` currently has 199 passing units and 1,103/1,103 production lines,
126/126 functions and 1,477/1,477 regions. Its notification transport and clock
are replaced by typed doubles. JSON payloads, status handling and errors are
asserted without HTTP. The flaky zero-cooldown assertion now uses deterministic
instants and checks the original strict greater-than boundary.

The build script is executed from its actual tracked source with an explicit
protoc command double. The double captures arguments and returns a public
schema descriptor fixture or a controlled error; no compiler is contacted in
these units. Both client/server code-generation settings and error context are
asserted. Two units cover 8/8 production lines, 2/2 functions and 12/12 regions.

The repository remains incomplete until all client/server paths and platform
logic pass their unit scope checks, native bindings build, and the queue gate
accepts the final committed revision.

### Native versus unit provider bindings

The `unit-doubles` feature on client/server selects external provider doubles
only when compiling tests. Normal production builds always keep the original
native SDK imports. The client's original SQLite adapter tests remain intact
under the normal test configuration; in the explicit unit configuration they
are separated from the scripted adapter units. Their original names and scope
remain inventoried by the baseline, rather than counted as unit tests. This
keeps normal SDK behavior reviewable and prevents dashboard constructors from
opening a real database during unit measurement. Scripted SQLite responses
verify SQL, values, operation order, row conversion and errors; they do not
execute SQL or reimplement the storage methods. The gRPC double captures typed
requests and authentication metadata, checks gzip settings and returns typed
responses/errors without sockets. Unconfigured external connections fail closed.


### Historical committed checkpoint (`90d0b60`)

`iterations/workspace-01` retains 708 passing selected units (212 server,
297 client, 199 shared), original LLVM exports, JUnit, selection/catalog and the
production-scope proof. Production workspace coverage is still partial:
6,142/9,096 lines, 576/845 functions and 9,384/13,800 regions. The separate build
script and Bash/PowerShell reports remain preserved.

The client gRPC mapping, configuration, SQLite orchestration, TLS and legacy UI
are fully covered by supported counters at this checkpoint. The dashboard render
coordinator, database/process renderers and popups are complete; the tab renderer
still needs one region. Main/bootstrap, the dashboard event loop, most server
monitor/service/collector paths and a few error cases remain unfinished.

The `tracing/log` feature creates a separate fallback formatting path. Dedicated
unit cases run before subscriber installation and capture native log output in
memory; later subscriber cases exercise ordinary tracing. Both paths execute
real production code, without removing compiler-generated coverage points.

Original baseline binaries are copied with SHA-256 verification under
`baseline/preserved-original-binaries`, so later Cargo builds cannot overwrite
the baseline's executable evidence. Source and binary provenance is retained.


### Resumed work (not a final coverage claim)

`progress.json` outside Git is the current evidence index. `client-13` has 333 passing unit tests, 5,185/5,185 production lines,
351/351 functions and 8,050/8,050 native regions. `server-13` has 286 passing
units, 2,810/2,810 lines, 374/374 functions and 4,307/4,307 regions. These are
component iterations, not a final repository claim. Reports are never combined
by adding counts from different source revisions. Final coverage requires native
checks and a clean committed workspace run.

The resumed units execute real dashboard handlers and rendering against a
memory backend wrapping `tui::TestBackend`, scripted crossterm input and a virtual
monotonic clock. Files and configuration use owned scratch paths. HTTP listeners,
registry requests, Docker, PostgreSQL and MariaDB SDKs are replaced by typed
responses, error queues and captured requests; they never fall back to a real
service. Monitoring service units replace their monitor collaborator while the
actual monitor body remains compiled and is covered independently against its
sysinfo and collector SDK boundaries. Startup uses typed CLI input, a memory
logging builder, an in-memory task scheduler, gRPC transport and signal doubles.
No user/service provisioning, deployment, publication or host monitoring runs.

Minimal private seams keep the original defaults: startup connection policy is
factored out of the dashboard; expiry decoding accepts chrono's original decoder;
stream limits expose the same lock/error mapping; network discovery keeps lock
ordering and accepts a provider. A pure font policy takes the platform flag so
both Unix and Windows defaults are observable in units. Successful-only private
refresh/settings/connection-probe operations now return their values directly,
and the nonterminating monitoring loop returns `!`; their original behavior had
no reachable `Err`/success-return respectively. The selected-server id and tab
hotkey checks use their already-established invariants without deleting any
reachable behavior. Existing public defaults and normal native SDK imports are
preserved. Native production builds remain a separate required final check.

Additional public certificate fixtures cover legacy CN and IP-only SAN parsing.
`create-certificate-fixtures.py` runs only as offline fixture preparation, records
its invocation, and discards temporary private keys. Unit certificate reads are
restricted to owned temporary paths, with explicit inventory/read failure cases.

Server command dispatch now loads configuration inside each selected stateful
arm of one exhaustive match. This preserves Init's ability to work without
preloading the existing file, and preserves Import's existing validation of the
current file before replacement. It removes the duplicate, unreachable Init arm.
The EnableAuth handler uses Config::load's existing guarantee that missing tokens
are generated and persisted; the redundant second empty-token branch had no
reachable path. Empty-token behavior remains asserted by the original handler
case and dedicated loader units. No testable command or production file is
excluded to close coverage.
