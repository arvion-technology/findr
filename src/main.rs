//! Findr command line interface.
//!
//! Parsing, filtering and aggregation live in the library; this file is the
//! argument parser, the readers and the output formatting. Exit codes are
//! stable: 2 for usage errors, 1 for I/O or parse failures, 0 on success.

use std::env;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::process;
use std::time::Instant;

use findr::aggregate::{field_present, numeric_values};
use findr::filter::Filter;
use findr::parser::{ParsedLine, parse_line};
use findr::stats::{Summary, summarize};
use findr::tail::{read_appended, read_last_lines};
use findr::value::{Record, format_number, quote, record_to_json};

const HELP: &str = "\
findr {version}
Fast log analysis with no dependencies.

USAGE
  findr <COMMAND> [OPTIONS] [FILE...]

COMMANDS
  stats     Count records and summarize numeric fields
  top       Count the most common values of a field
  filter    Print records matching one or more expressions
  tail      Print the last lines of a file, optionally following it
  bench     Measure parse and aggregation throughput on generated data

OPTIONS
  --field <FIELDS>    Comma separated numeric fields for stats (repeatable)
  --expr <EXPR>       Filter expression (repeatable). Supported forms:
                        status=500        status!=500
                        msg contains text latency>200
                        latency>=200      latency<50      latency<=50
  --by <FIELD>        Field to count for top
  -n, --n <N>         Rows for top (default 10)
  --limit <N>         Maximum records printed by filter
  --format <FORMAT>   table, json or raw (default table; filter prints raw)
  --file <PATH>       File for tail (otherwise the first FILE or stdin)
  --lines <N>         Tail line count (default 10); record count for bench
  --follow            Keep following the tail file for appended lines
  --strict            Stop on the first malformed line instead of skipping
  --quiet             Suppress warnings and summary lines
  -h, --help          Show this help
  -V, --version       Print the version

EXAMPLES
  findr stats examples/access.log --field latency
  findr top --by country -n 5 examples/access.log
  findr filter --expr 'status=500' --expr 'latency>200' examples/access.log
  findr tail --file examples/service.log --lines 5
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Table,
    Json,
    Raw,
}

#[derive(Debug)]
struct Options {
    command: String,
    files: Vec<String>,
    fields: Vec<String>,
    expressions: Vec<String>,
    by: Option<String>,
    n: usize,
    limit: Option<usize>,
    format: Format,
    strict: bool,
    quiet: bool,
    file: Option<String>,
    lines: usize,
    lines_set: bool,
    follow: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            command: String::new(),
            files: Vec::new(),
            fields: Vec::new(),
            expressions: Vec::new(),
            by: None,
            n: 10,
            limit: None,
            format: Format::Table,
            strict: false,
            quiet: false,
            file: None,
            lines: 10,
            lines_set: false,
            follow: false,
        }
    }
}

#[derive(Debug)]
enum Failure {
    Usage(String),
    Runtime(String),
}

fn usage(message: impl Into<String>) -> Failure {
    Failure::Usage(message.into())
}

fn runtime(message: impl Into<String>) -> Failure {
    Failure::Runtime(message.into())
}

fn main() {
    match run(env::args().skip(1).collect()) {
        Ok(()) => {}
        Err(Failure::Usage(message)) => {
            eprintln!("findr: {message}");
            eprintln!("try 'findr --help'");
            process::exit(2);
        }
        Err(Failure::Runtime(message)) => {
            eprintln!("findr: {message}");
            process::exit(1);
        }
    }
}

fn run(args: Vec<String>) -> Result<(), Failure> {
    let options = parse_args(args)?;
    match options.command.as_str() {
        "help" | "" => {
            print!("{}", HELP.replace("{version}", findr::VERSION));
            if options.command.is_empty() {
                Err(usage("missing command"))
            } else {
                Ok(())
            }
        }
        "version" => {
            println!("findr {}", findr::VERSION);
            Ok(())
        }
        "stats" => command_stats(&options),
        "top" => command_top(&options),
        "filter" => command_filter(&options),
        "tail" => command_tail(&options),
        "bench" => command_bench(&options),
        other => Err(usage(format!("unknown command '{other}'"))),
    }
}

fn parse_args(args: Vec<String>) -> Result<Options, Failure> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].clone();
        let mut value = |name: &str| -> Result<String, Failure> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| usage(format!("{name} needs a value")))
        };
        match argument.as_str() {
            "-h" | "--help" => options.command = "help".to_string(),
            "-V" | "--version" => options.command = "version".to_string(),
            "--strict" => options.strict = true,
            "--quiet" => options.quiet = true,
            "--follow" => options.follow = true,
            "--field" => options.fields.push(value("--field")?),
            "--expr" => options.expressions.push(value("--expr")?),
            "--by" => options.by = Some(value("--by")?),
            "-n" | "--n" => {
                let raw = value("--n")?;
                options.n = raw
                    .parse()
                    .map_err(|_| usage(format!("--n expects a number, got '{raw}'")))?;
            }
            "--limit" => {
                let raw = value("--limit")?;
                options.limit = Some(
                    raw.parse()
                        .map_err(|_| usage(format!("--limit expects a number, got '{raw}'")))?,
                );
            }
            "--format" => {
                let raw = value("--format")?;
                options.format = match raw.as_str() {
                    "table" => Format::Table,
                    "json" => Format::Json,
                    other => {
                        return Err(usage(format!(
                            "unknown format '{other}', use table or json"
                        )));
                    }
                };
            }
            "--file" => options.file = Some(value("--file")?),
            "--lines" => {
                let raw = value("--lines")?;
                options.lines = raw
                    .parse()
                    .map_err(|_| usage(format!("--lines expects a number, got '{raw}'")))?;
                options.lines_set = true;
            }
            other if other.starts_with("--") => {
                if let Some((name, raw)) = other.split_once('=') {
                    match name {
                        "--field" => options.fields.push(raw.to_string()),
                        "--expr" => options.expressions.push(raw.to_string()),
                        "--by" => options.by = Some(raw.to_string()),
                        "--n" => {
                            options.n = raw
                                .parse()
                                .map_err(|_| usage(format!("--n expects a number, got '{raw}'")))?
                        }
                        "--limit" => {
                            options.limit = Some(raw.parse().map_err(|_| {
                                usage(format!("--limit expects a number, got '{raw}'"))
                            })?)
                        }
                        "--format" => {
                            options.format = match raw {
                                "table" => Format::Table,
                                "json" => Format::Json,
                                "raw" => Format::Raw,
                                other => return Err(usage(format!("unknown format '{other}'"))),
                            }
                        }
                        "--file" => options.file = Some(raw.to_string()),
                        "--lines" => {
                            options.lines = raw.parse().map_err(|_| {
                                usage(format!("--lines expects a number, got '{raw}'"))
                            })?
                        }
                        _ => return Err(usage(format!("unknown option '{name}'"))),
                    }
                } else {
                    return Err(usage(format!("unknown option '{other}'")));
                }
            }
            other if options.command.is_empty() => options.command = other.to_string(),
            other => options.files.push(other.to_string()),
        }
        index += 1;
    }
    Ok(options)
}

struct Loaded {
    lines: Vec<ParsedLine>,
    malformed: usize,
}

fn load(options: &Options) -> Result<Loaded, Failure> {
    let mut loaded = Loaded {
        lines: Vec::new(),
        malformed: 0,
    };
    if options.files.is_empty() {
        let mut content = String::new();
        io::stdin()
            .read_to_string(&mut content)
            .map_err(|error| runtime(format!("reading stdin: {error}")))?;
        parse_into(&mut loaded, &content, "<stdin>", options)?;
        return Ok(loaded);
    }
    for path in &options.files {
        let content =
            fs::read_to_string(path).map_err(|error| runtime(format!("{path}: {error}")))?;
        parse_into(&mut loaded, &content, path, options)?;
    }
    Ok(loaded)
}

fn parse_into(
    loaded: &mut Loaded,
    content: &str,
    source: &str,
    options: &Options,
) -> Result<(), Failure> {
    for (index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(line, index + 1) {
            Ok(parsed) => loaded.lines.push(parsed),
            Err(error) => {
                loaded.malformed += 1;
                if options.strict {
                    return Err(runtime(format!("{source}:{}: {error}", index + 1)));
                }
                if !options.quiet {
                    eprintln!("warning: {source}:{}: {error}", index + 1);
                }
            }
        }
    }
    Ok(())
}

fn build_filter(options: &Options) -> Result<Filter, Failure> {
    Filter::parse_all(&options.expressions).map_err(usage)
}

fn matched<'a>(lines: &'a [ParsedLine], filter: &Filter) -> Vec<&'a ParsedLine> {
    lines
        .iter()
        .filter(|line| filter.matches(&line.record))
        .collect()
}

fn command_stats(options: &Options) -> Result<(), Failure> {
    let loaded = load(options)?;
    let filter = build_filter(options)?;
    let records: Vec<Record> = matched(&loaded.lines, &filter)
        .iter()
        .map(|line| line.record.clone())
        .collect();

    let mut fields: Vec<String> = Vec::new();
    for field in &options.fields {
        for part in field.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                fields.push(part.to_string());
            }
        }
    }
    if fields.is_empty() {
        fields = findr::numeric_fields(&records);
    }

    let mut summaries: Vec<(String, Summary)> = Vec::new();
    for field in &fields {
        if let Some(summary) = summarize(numeric_values(&records, field)) {
            summaries.push((field.clone(), summary));
        }
    }

    if options.format == Format::Json {
        let fields_json: Vec<String> = summaries
            .iter()
            .map(|(field, summary)| {
                format!(
                    "{}:{{\"count\":{},\"min\":{},\"max\":{},\"sum\":{},\"mean\":{},\"p50\":{},\"p95\":{},\"p99\":{}}}",
                    quote(field),
                    summary.count,
                    format_number(summary.min),
                    format_number(summary.max),
                    format_number(summary.sum),
                    format_number(summary.mean),
                    format_number(summary.p50),
                    format_number(summary.p95),
                    format_number(summary.p99),
                )
            })
            .collect();
        println!(
            "{{\"records\":{},\"matched\":{},\"malformed\":{},\"fields\":{{{}}}}}",
            loaded.lines.len() + loaded.malformed,
            records.len(),
            loaded.malformed,
            fields_json.join(",")
        );
        return Ok(());
    }

    println!(
        "records: {}   matched: {}   malformed: {}",
        loaded.lines.len() + loaded.malformed,
        records.len(),
        loaded.malformed
    );
    if summaries.is_empty() {
        println!("no numeric fields found; use --field <name> to choose one");
        return Ok(());
    }

    let headers = ["field", "count", "min", "mean", "p50", "p95", "p99", "max"];
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (field, summary) in &summaries {
        rows.push(vec![
            field.clone(),
            summary.count.to_string(),
            format_number(summary.min),
            format_number(summary.mean),
            format_number(summary.p50),
            format_number(summary.p95),
            format_number(summary.p99),
            format_number(summary.max),
        ]);
    }
    print_table(&headers, &rows);
    Ok(())
}

fn command_top(options: &Options) -> Result<(), Failure> {
    let by = options
        .by
        .clone()
        .ok_or_else(|| usage("top needs --by <field>"))?;
    let loaded = load(options)?;
    let filter = build_filter(options)?;
    let records: Vec<Record> = matched(&loaded.lines, &filter)
        .iter()
        .map(|line| line.record.clone())
        .collect();
    let entries = findr::top_n(&records, &by, options.n);
    let present = field_present(&records, &by);

    if options.format == Format::Json {
        let items: Vec<String> = entries
            .iter()
            .map(|entry| {
                format!(
                    "{{\"key\":{},\"count\":{}}}",
                    quote(&entry.key),
                    entry.count
                )
            })
            .collect();
        println!(
            "{{\"field\":{},\"matched\":{},\"present\":{},\"values\":[{}]}}",
            quote(&by),
            records.len(),
            present,
            items.join(",")
        );
        return Ok(());
    }

    println!(
        "field: {by}   matched: {}   present: {}   top: {}",
        records.len(),
        present,
        entries.len()
    );
    if entries.is_empty() {
        println!("no records with field '{by}'");
        return Ok(());
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let share = if present == 0 {
            0.0
        } else {
            entry.count as f64 / present as f64 * 100.0
        };
        rows.push(vec![
            (index + 1).to_string(),
            entry.key.clone(),
            entry.count.to_string(),
            format!("{share:.1}%"),
        ]);
    }
    print_table(&["rank", "key", "count", "share"], &rows);
    Ok(())
}

fn command_filter(options: &Options) -> Result<(), Failure> {
    let loaded = load(options)?;
    let filter = build_filter(options)?;
    let selected = matched(&loaded.lines, &filter);
    let limit = options.limit.unwrap_or(usize::MAX);
    let shown = selected.iter().take(limit).collect::<Vec<_>>();

    if options.format == Format::Json {
        for line in &shown {
            println!("{}", record_to_json(&line.record));
        }
    } else {
        // Table and raw both print the original line; filtering never rewrites a log.
        for line in &shown {
            println!("{}", line.raw);
        }
    }
    if !options.quiet {
        eprintln!(
            "matched {} of {} records ({} malformed)",
            selected.len(),
            loaded.lines.len(),
            loaded.malformed
        );
    }
    Ok(())
}

fn command_tail(options: &Options) -> Result<(), Failure> {
    let file = options
        .file
        .clone()
        .or_else(|| options.files.first().cloned());
    let filter = build_filter(options)?;

    let Some(file) = file else {
        if options.follow {
            return Err(usage("--follow needs --file"));
        }
        // Stream stdin as it arrives.
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.map_err(|error| runtime(format!("reading stdin: {error}")))?;
            if line.trim().is_empty() {
                continue;
            }
            match parse_line(&line, 0) {
                Ok(parsed) if filter.matches(&parsed.record) => {
                    println!("{}", parsed.raw);
                }
                Ok(_) => {}
                Err(error) => {
                    if options.strict {
                        return Err(runtime(error.to_string()));
                    }
                    if !options.quiet {
                        eprintln!("warning: {error}");
                    }
                }
            }
        }
        return Ok(());
    };

    let path = Path::new(&file);
    for line in
        read_last_lines(path, options.lines).map_err(|error| runtime(format!("{file}: {error}")))?
    {
        print_tail_line(&line, &filter, options)?;
    }
    if !options.follow {
        return Ok(());
    }

    let mut offset = fs::metadata(path)
        .map_err(|error| runtime(format!("{file}: {error}")))?
        .len();
    loop {
        match read_appended(path, offset) {
            Ok((lines, next)) => {
                offset = next;
                for line in lines {
                    print_tail_line(&line, &filter, options)?;
                }
            }
            Err(error) => {
                if !options.quiet {
                    eprintln!("warning: {file}: {error}");
                }
            }
        }
        io::stdout().flush().ok();
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn print_tail_line(line: &str, filter: &Filter, options: &Options) -> Result<(), Failure> {
    if line.trim().is_empty() {
        return Ok(());
    }
    match parse_line(line, 0) {
        Ok(parsed) if filter.matches(&parsed.record) => {
            println!("{}", parsed.raw);
            io::stdout().flush().ok();
        }
        Ok(_) => {}
        Err(error) => {
            if options.strict {
                return Err(runtime(error));
            }
            if !options.quiet {
                eprintln!("warning: {error}");
            }
        }
    }
    Ok(())
}

fn command_bench(options: &Options) -> Result<(), Failure> {
    let count = if options.lines_set {
        options.lines
    } else {
        100_000
    }
    .max(1);
    let started = Instant::now();
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut lines = Vec::with_capacity(count);
    for index in 0..count {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let latency = 5 + (state >> 33) % 900;
        let status = match (state >> 20) % 10 {
            0..=5 => 200,
            6..=7 => 404,
            8 => 500,
            _ => 201,
        };
        let country = ["NP", "US", "IN", "DE"][((state >> 8) % 4) as usize];
        lines.push(format!(
            "{{\"ts\":\"2026-01-01T10:{:02}:{:02}Z\",\"status\":{},\"latency\":{},\"country\":\"{}\",\"path\":\"/api/{}\"}}",
            index % 60,
            index % 60,
            status,
            latency,
            country,
            index % 7
        ));
    }
    let generation = started.elapsed();

    let parse_started = Instant::now();
    let mut records = Vec::with_capacity(count);
    for (index, line) in lines.iter().enumerate() {
        let parsed = parse_line(line, index + 1)
            .map_err(|error| runtime(format!("generated line failed to parse: {error}")))?;
        records.push(parsed.record);
    }
    let parse_time = parse_started.elapsed();

    let query_started = Instant::now();
    let filter = Filter::parse("latency>500").map_err(usage)?;
    let matching: Vec<Record> = records
        .iter()
        .filter(|record| filter.matches(record))
        .cloned()
        .collect();
    let summary = summarize(numeric_values(&matching, "latency"));
    let query_time = query_started.elapsed();

    let per_second = |elapsed: std::time::Duration| {
        if elapsed.as_secs_f64() > 0.0 {
            count as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        }
    };
    println!(
        "generated {count} lines in {:.1} ms",
        generation.as_secs_f64() * 1000.0
    );
    println!(
        "parsed in {:.1} ms ({:.0} lines/sec)",
        parse_time.as_secs_f64() * 1000.0,
        per_second(parse_time)
    );
    println!(
        "filter + summarize in {:.1} ms ({} matches, p95 {})",
        query_time.as_secs_f64() * 1000.0,
        matching.len(),
        summary
            .map(|value| format_number(value.p95))
            .unwrap_or_else(|| "-".to_string())
    );
    Ok(())
}

fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if index < widths.len() {
                widths[index] = widths[index].max(cell.chars().count());
            }
        }
    }
    let header_line: Vec<String> = headers
        .iter()
        .enumerate()
        .map(|(index, header)| format!("{:<width$}", header, width = widths[index]))
        .collect();
    println!("{}", header_line.join("  "));
    let rule: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
    println!("{}", rule.join("  "));
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(index, cell)| format!("{:<width$}", cell, width = widths[index]))
            .collect();
        println!("{}", cells.join("  "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> Options {
        parse_args(args.iter().map(|arg| arg.to_string()).collect()).unwrap()
    }

    #[test]
    fn parses_command_files_and_flags() {
        let parsed = options(&[
            "stats",
            "--field",
            "latency,bytes",
            "--expr",
            "status=500",
            "a.log",
            "b.log",
        ]);
        assert_eq!(parsed.command, "stats");
        assert_eq!(parsed.fields, vec!["latency,bytes"]);
        assert_eq!(parsed.expressions, vec!["status=500"]);
        assert_eq!(parsed.files, vec!["a.log", "b.log"]);
    }

    #[test]
    fn parses_equals_form_and_numbers() {
        let parsed = options(&["top", "--by=country", "--n=3", "--format", "json"]);
        assert_eq!(parsed.by.as_deref(), Some("country"));
        assert_eq!(parsed.n, 3);
        assert_eq!(parsed.format, Format::Json);
    }

    #[test]
    fn rejects_unknown_options_and_bad_numbers() {
        assert!(parse_args(vec!["stats".into(), "--wat".into()]).is_err());
        assert!(parse_args(vec!["top".into(), "--n".into(), "abc".into()]).is_err());
        assert!(parse_args(vec!["stats".into(), "--format".into(), "xml".into()]).is_err());
    }

    #[test]
    fn tail_options_are_parsed() {
        let parsed = options(&["tail", "--file", "a.log", "--lines", "5", "--follow"]);
        assert_eq!(parsed.file.as_deref(), Some("a.log"));
        assert_eq!(parsed.lines, 5);
        assert!(parsed.follow);
    }
}
