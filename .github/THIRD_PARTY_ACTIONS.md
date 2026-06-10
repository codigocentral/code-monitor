# Third-Party GitHub Actions Inventory

All actions used in our workflows are pinned to full commit SHAs (with the
human-readable tag kept as a trailing comment) to prevent supply-chain attacks
via moving tags. This file documents each action, why we use it, and how to
keep it up to date.

| Action | Upstream | Pinned ref | Purpose |
|--------|----------|------------|---------|
| `actions/checkout` | https://github.com/actions/checkout | `34e1148...` (v4) | Check out the repository (official GitHub action) |
| `actions/upload-artifact` | https://github.com/actions/upload-artifact | `ea165f8...` (v4) | Upload release build artifacts (official) |
| `actions/download-artifact` | https://github.com/actions/download-artifact | `d3f86a1...` (v4) | Download artifacts in the release job (official) |
| `dtolnay/rust-toolchain` | https://github.com/dtolnay/rust-toolchain | `29eef33...` (stable) | Install the Rust toolchain; maintained by a libs-team Rust maintainer |
| `Swatinem/rust-cache` | https://github.com/Swatinem/rust-cache | `e18b497...` (v2) | Cache cargo registry/target between CI runs |
| `codecov/codecov-action` | https://github.com/codecov/codecov-action | `ab904c4...` (v3) | Upload tarpaulin coverage reports to Codecov |
| `rustsec/audit-check` | https://github.com/rustsec/audit-check | `e9159ac...` (v1) | Run `cargo audit` against the RustSec advisory DB |
| `taiki-e/install-action` | https://github.com/taiki-e/install-action | `903f26e...` (cross) | Install `cross` for aarch64/musl cross-compilation in releases |
| `softprops/action-gh-release` | https://github.com/softprops/action-gh-release | `de2c0eb...` (v1) | Create the GitHub Release and attach binaries |

Full SHAs are in `.github/workflows/ci.yml` and `.github/workflows/release.yml`.

## Update policy

- **Never** switch back to floating tags (`@v4`, `@stable`, `@cross`).
- To update an action, resolve the new tag to a commit SHA
  (`curl -s https://api.github.com/repos/OWNER/REPO/commits/TAG`), update the
  SHA and the comment together, and review the upstream changelog/diff before
  merging.
- Recommended: enable [Dependabot for GitHub Actions](https://docs.github.com/en/code-security/dependabot/working-with-dependabot/keeping-your-actions-up-to-date-with-dependabot)
  (`package-ecosystem: "github-actions"` in `.github/dependabot.yml`) — it
  understands SHA pinning and keeps the trailing version comment in sync.
- Review cadence: quarterly, or immediately when an advisory affects one of the
  actions above.

## Trust assessment

- `actions/*` are first-party GitHub actions.
- `dtolnay/rust-toolchain`, `Swatinem/rust-cache` and `taiki-e/install-action`
  are de-facto standards in the Rust ecosystem with broad usage.
- `codecov/codecov-action` had a supply-chain incident in 2021 (bash uploader);
  the action is pinned and only receives a coverage XML, no secrets beyond the
  Codecov token.
- `rustsec/audit-check` and `softprops/action-gh-release` receive the
  `GITHUB_TOKEN` — keep them pinned and review diffs on updates.
