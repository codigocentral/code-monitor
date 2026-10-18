#!/bin/bash
campaign=/Volumes/LaCie/GitLab/coverage-unitarios-2026-10-17
export TMPDIR="$campaign/cache/session-13/tmp"
export npm_config_cache="$campaign/cache/session-13/npm"
export NUGET_PACKAGES="$campaign/cache/session-13/nuget"
export CARGO_TARGET_DIR="$campaign/cache/session-13/cargo-code-monitor"
export PYTHONPATH="$campaign/cache/session-13/python${PYTHONPATH:+:$PYTHONPATH}"
export PIP_CACHE_DIR="$campaign/cache/session-13/pip"
export GEM_HOME="$campaign/cache/session-13/ruby"
export GEM_PATH="$GEM_HOME:/Library/Ruby/Gems/2.6.0:/System/Library/Frameworks/Ruby.framework/Versions/2.6/usr/lib/ruby/gems/2.6.0"

export CARGO_HOME="$campaign/cache/session-13/cargo-home-code-monitor"
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export PATH="$campaign/cache/session-13/rust-tools/cargo-llvm-cov:$PATH"
export LLVM_COV="$campaign/cache/session-13/rust-tools/llvm-tools-1.95.0-aarch64-apple-darwin/llvm-tools-preview/lib/rustlib/aarch64-apple-darwin/bin/llvm-cov"
export LLVM_PROFDATA="${LLVM_COV%/llvm-cov}/llvm-profdata"
export CARGO_LLVM_COV_SETUP=no
export PROTOC="$campaign/cache/session-13/rust-tools/protoc-33.2/bin/protoc"
export PROTOC_INCLUDE="$campaign/cache/session-13/rust-tools/protoc-33.2/include"
export TOKIO_WORKER_THREADS=1
