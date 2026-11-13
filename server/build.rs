//! Embed the git revision so `--version` and `/metrics` say exactly which
//! build is running.
//!
//! Resolution order: `CODE_MONITOR_GIT_SHA` (set by CI or a packager), then
//! `git rev-parse` in the source tree, then `unknown` for a source tarball.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=CODE_MONITOR_GIT_SHA");
    println!("cargo:rerun-if-env-changed=CI_COMMIT_SHORT_SHA");

    let git = |args: &[&str]| -> Option<String> {
        let output = Command::new("git").args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
        (!text.is_empty()).then_some(text)
    };

    // Rebuild when HEAD moves, so a local build never reports a stale sha
    if let Some(head) = git(&["rev-parse", "--path-format=absolute", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &reference,
        ]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    let sha = std::env::var("CODE_MONITOR_GIT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("CI_COMMIT_SHORT_SHA")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| git(&["rev-parse", "--short=8", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=CODE_MONITOR_GIT_SHA={sha}");
}
