# test

This directory exists because the showcase scaffold checks for a `test/`
directory. The real Rust test suites live where Cargo expects them:

- `src/**/*.rs` unit tests (`#[cfg(test)]`)
- `tests/cli.rs` integration tests that invoke the compiled binary

Run both with `bash scripts/test.sh` or `cargo test`.
