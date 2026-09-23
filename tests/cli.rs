//! Integration tests for the `findr` binary.
//!
//! These spawn the compiled CLI with `CARGO_BIN_EXE_findr` and read its stdout
//! and stderr, so argument parsing, file reading, output formatting and exit
//! codes are all exercised through the same path a user takes.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn fixture(name: &str) -> String {
    format!("{}/examples/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_findr"))
        .args(args)
        .output()
        .expect("failed to run findr")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn stats_reports_counts_and_percentiles() {
    let output = run(&["stats", &fixture("access.log")]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("records: 24"), "{text}");
    assert!(text.contains("matched: 24"), "{text}");
    assert!(text.contains("malformed: 0"), "{text}");
    assert!(text.contains("latency"), "{text}");
    assert!(text.contains("p95"), "{text}");
    assert!(text.contains("p99"), "{text}");
}

#[test]
fn stats_can_emit_json() {
    let output = run(&["stats", "--format", "json", &fixture("access.log")]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.starts_with("{\"records\":24"), "{text}");
    assert!(text.contains("\"fields\":{\"latency\""), "{text}");
    assert!(text.contains("\"p95\":"), "{text}");
    assert!(text.trim_end().ends_with('}'));
}

#[test]
fn stats_field_limits_output_to_the_requested_field() {
    let output = run(&["stats", "--field", "latency", &fixture("access.log")]);
    let text = stdout(&output);
    assert!(text.contains("latency"));
    assert!(!text.contains("country"), "{text}");
    assert!(text.contains("p50"));
}

#[test]
fn filter_by_equality_prints_matching_raw_lines() {
    let output = run(&["filter", "--expr", "status=500", &fixture("access.log")]);
    assert!(output.status.success());
    let lines: Vec<String> = stdout(&output).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 5, "{lines:?}");
    for line in &lines {
        assert!(line.contains("\"status\":500"), "{line}");
    }
    assert!(stderr(&output).contains("matched 5 of 24"));
}

#[test]
fn filter_combines_numeric_and_contains_expressions() {
    let output = run(&[
        "filter",
        "--expr",
        "latency>300",
        "--expr",
        "msg contains exhausted",
        &fixture("service.log"),
    ]);
    let lines: Vec<String> = stdout(&output).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines.iter().all(|line| line.contains("db pool exhausted")));
}

#[test]
fn filter_json_outputs_structured_records() {
    let output = run(&[
        "filter",
        "--expr",
        "status=500",
        "--format",
        "json",
        &fixture("access.log"),
    ]);
    let lines: Vec<String> = stdout(&output).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 5);
    for line in &lines {
        assert!(line.starts_with('{') && line.ends_with('}'), "{line}");
        assert!(line.contains("\"latency\":"));
        assert!(line.contains("\"status\":500"));
    }
}

#[test]
fn key_value_logs_are_filtered_too() {
    let output = run(&["filter", "--expr", "route=/health", &fixture("service.log")]);
    let lines: Vec<String> = stdout(&output).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 4, "{lines:?}");
}

#[test]
fn top_counts_values_and_orders_by_count() {
    let output = run(&["top", "--by", "country", "-n", "2", &fixture("access.log")]);
    assert!(output.status.success());
    let text = stdout(&output);
    let rows: Vec<Vec<String>> = text
        .lines()
        .filter(|line| {
            !line.starts_with("field:") && !line.starts_with("rank") && !line.starts_with("--")
        })
        .map(|line| line.split_whitespace().map(str::to_string).collect())
        .collect();
    assert_eq!(rows.len(), 2, "{text}");
    assert_eq!(rows[0][1], "NP");
    assert_eq!(rows[0][2], "11");
    assert_eq!(rows[1][1], "US");
    assert_eq!(rows[1][2], "6");
}

#[test]
fn top_can_emit_json() {
    let output = run(&[
        "top",
        "--by",
        "country",
        "--format",
        "json",
        &fixture("access.log"),
    ]);
    let text = stdout(&output);
    assert!(text.contains("\"field\":\"country\""), "{text}");
    assert!(text.contains("\"key\":\"NP\""), "{text}");
}

#[test]
fn tail_prints_the_last_lines_of_a_file() {
    let output = run(&["tail", "--file", &fixture("service.log"), "--lines", "3"]);
    assert!(output.status.success());
    let lines: Vec<String> = stdout(&output).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[2].contains("latency=44"), "{}", lines[2]);
}

#[test]
fn tail_applies_the_filter() {
    let output = run(&[
        "tail",
        "--file",
        &fixture("service.log"),
        "--lines",
        "12",
        "--expr",
        "level=error",
    ]);
    let lines: Vec<String> = stdout(&output).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
}

#[test]
fn reads_from_stdin_when_no_file_is_given() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_findr"))
        .args(["stats", "--field", "latency"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn findr");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        writeln!(stdin, "{{\"status\":200,\"latency\":10}}").unwrap();
        writeln!(stdin, "{{\"status\":500,\"latency\":30}}").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("records: 2"), "{text}");
    assert!(text.contains("latency"));
}

#[test]
fn strict_mode_stops_on_malformed_input() {
    let output = run(&["stats", "--strict", &fixture("malformed.log")]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(stderr(&output).contains("findr:"), "{}", stderr(&output));
}

#[test]
fn malformed_lines_are_skipped_with_a_warning_by_default() {
    let output = run(&["stats", &fixture("malformed.log")]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("records: 3"), "{text}");
    assert!(text.contains("malformed: 1"), "{text}");
    assert!(stderr(&output).contains("warning:"), "{}", stderr(&output));
}

#[test]
fn missing_input_file_exits_one() {
    let output = run(&["stats", "does-not-exist.log"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("findr:"));
}

#[test]
fn unknown_command_and_unknown_flag_exit_two() {
    assert_eq!(run(&["nope"]).status.code(), Some(2));
    assert_eq!(run(&["stats", "--wat"]).status.code(), Some(2));
}

#[test]
fn help_and_version_are_available() {
    let help = run(&["--help"]);
    assert!(help.status.success());
    assert!(stdout(&help).contains("USAGE"));
    assert!(stdout(&help).contains("findr stats"));

    let version = run(&["--version"]);
    assert!(version.status.success());
    assert!(stdout(&version).contains("findr 1.0.0"));
}

#[test]
fn bench_runs_without_external_data() {
    let output = run(&["bench", "--lines", "2000"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("parsed in"), "{text}");
    assert!(text.contains("filter + summarize"), "{text}");
}

#[test]
fn filter_with_no_matches_prints_nothing_and_says_so() {
    let output = run(&["filter", "--expr", "status=999", &fixture("access.log")]);
    assert!(output.status.success());
    assert!(stdout(&output).trim().is_empty());
    assert!(stderr(&output).contains("matched 0 of 24"));
}
