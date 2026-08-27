//! Findr: a small, dependency-free log analysis library.
//!
//! The binary in `src/main.rs` is a thin shell over the modules here. Keeping
//! parsing, filtering and aggregation in a library means the logic can be
//! tested without spawning a process, and an integration test still exercises
//! the real CLI through `tests/cli.rs`.

pub mod aggregate;
pub mod filter;
pub mod json;
pub mod parser;
pub mod stats;
pub mod tail;
pub mod value;

pub use aggregate::{TopEntry, numeric_fields, top_n};
pub use filter::{Condition, Filter};
pub use parser::{Format, ParsedLine, parse_line};
pub use stats::{Summary, percentile, summarize};
pub use value::{Record, Value};

/// Version reported by `findr --version` and the tests.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
