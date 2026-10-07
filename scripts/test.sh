#!/usr/bin/env bash
# One test entry point for the polyglot projects. The gate calls this when a
# project has no package.json.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --quiet
