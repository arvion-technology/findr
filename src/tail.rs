//! Tail helpers.
//!
//! The command is intentionally simple: read a file, take the final N lines,
//! then follow appended bytes. It reads the file rather than seeking backwards
//! from the end, which is fine for the log sizes a CLI like this is used on and
//! keeps the logic easy to test. The README names that trade-off.

use std::fs;
use std::io;
use std::path::Path;

/// Last `n` lines from a buffer, without their trailing newline.
pub fn last_lines(content: &str, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let lines: Vec<String> = content
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].to_vec()
}

/// Read the last `n` lines of a file.
pub fn read_last_lines(path: &Path, n: usize) -> io::Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    Ok(last_lines(&content, n))
}

/// Read complete lines appended after `offset`.
///
/// Returns the lines and the new offset. A trailing partial line is not
/// returned, so a writer that flushes half a line does not produce half a log
/// record; the next poll picks it up.
pub fn read_appended(path: &Path, mut offset: u64) -> io::Result<(Vec<String>, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length < offset {
        // The file was truncated or rotated; start again from the beginning.
        file.seek(SeekFrom::Start(0))?;
        offset = 0;
    } else {
        file.seek(SeekFrom::Start(offset))?;
    }
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)?;
    let complete_len = buffer.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let complete = &buffer[..complete_len];
    let lines = complete
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect();
    let new_offset = if length < offset {
        complete_len as u64
    } else {
        offset + complete_len as u64
    };
    Ok((lines, new_offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("findr-{name}-{stamp}.log"));
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn takes_the_last_lines() {
        let content = "one\ntwo\nthree\nfour\n";
        assert_eq!(last_lines(content, 2), vec!["three", "four"]);
        assert_eq!(last_lines(content, 10).len(), 4);
        assert!(last_lines(content, 0).is_empty());
    }

    #[test]
    fn handles_a_file_without_a_trailing_newline() {
        let content = "one\ntwo";
        assert_eq!(last_lines(content, 1), vec!["two"]);
    }

    #[test]
    fn reads_last_lines_from_disk() {
        let path = temp_file("tail", "a\nb\nc\n");
        let lines = read_last_lines(&path, 2).unwrap();
        assert_eq!(lines, vec!["b", "c"]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn appends_and_reports_the_new_offset() {
        let path = temp_file("append", "first\n");
        let offset = fs::metadata(&path).unwrap().len();
        let (lines, new_offset) = read_appended(&path, offset).unwrap();
        assert!(lines.is_empty());

        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write;
        writeln!(file, "second").unwrap();
        writeln!(file, "third").unwrap();
        let (lines, new_offset) = read_appended(&path, new_offset).unwrap();
        assert_eq!(lines, vec!["second", "third"]);
        assert!(new_offset > offset);
        let _ = fs::remove_file(path);
    }
}
