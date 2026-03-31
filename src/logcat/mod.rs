use anyhow::{Context, Result};
use serde::Serialize;

use crate::adb;
use crate::cli::LogArgs;

#[derive(Debug, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub pid: String,
    pub tid: String,
    pub level: String,
    pub tag: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct LogOutput {
    pub entries: Vec<LogEntry>,
    pub total: usize,
}

fn parse_level_filter(level: &str) -> &str {
    match level.to_lowercase().as_str() {
        "verbose" | "v" => "*:V",
        "debug" | "d" => "*:D",
        "info" | "i" => "*:I",
        "warn" | "w" => "*:W",
        "error" | "e" => "*:E",
        "fatal" | "f" => "*:F",
        _ => "*:V",
    }
}

fn parse_logcat_line(line: &str) -> Option<LogEntry> {
    // Threadtime format: "03-31 00:12:34.567  1234  5678 I Tag     : message"
    let line = line.trim();
    if line.is_empty() || line.starts_with('-') {
        return None;
    }

    // Parse threadtime format by finding the ": " separator, then splitting the prefix
    if let Some(colon_pos) = line.find(": ") {
        let prefix = &line[..colon_pos];
        let message = line[colon_pos + 2..].to_string();

        let prefix_parts: Vec<&str> = prefix.split_whitespace().collect();
        if prefix_parts.len() >= 5 {
            return Some(LogEntry {
                timestamp: format!("{} {}", prefix_parts[0], prefix_parts[1]),
                pid: prefix_parts[2].to_string(),
                tid: prefix_parts[3].to_string(),
                level: prefix_parts[4].to_string(),
                tag: prefix_parts.get(5).unwrap_or(&"").to_string(),
                message,
            });
        }
    }

    Some(LogEntry {
        timestamp: String::new(),
        pid: String::new(),
        tid: String::new(),
        level: String::new(),
        tag: String::new(),
        message: line.to_string(),
    })
}

/// Fetch recent logcat entries with filtering.
pub fn fetch(app: Option<&str>, tag: Option<&str>, level: &str, lines: u32) -> Result<LogOutput> {
    let level_filter = parse_level_filter(level);

    let cmd = if let Some(package) = app {
        // Get PID(s) for the package. pidof can return multiple space-separated PIDs
        let pid_output = adb::shell_str(&format!("pidof {package}"))?;
        let pid = pid_output.trim();
        if pid.is_empty() {
            anyhow::bail!("App {package} is not running");
        }
        // Use the first PID (main process). --pid only accepts one value.
        let main_pid = pid.split_whitespace().next().unwrap_or(pid);
        if !main_pid.bytes().all(|b| b.is_ascii_digit()) {
            anyhow::bail!("Unexpected PID format from device: {main_pid}");
        }
        format!("logcat -d -v threadtime --pid={main_pid} {level_filter} -t {lines}")
    } else {
        format!("logcat -d -v threadtime {level_filter} -t {lines}")
    };

    let output = adb::shell_str(&cmd).context("Failed to read logcat")?;

    let mut entries: Vec<LogEntry> = output.lines().filter_map(parse_logcat_line).collect();

    // Filter by tag if specified
    if let Some(tag_filter) = tag {
        entries.retain(|e| e.tag.contains(tag_filter));
    }

    let total = entries.len();
    Ok(LogOutput { entries, total })
}

/// CLI entry point.
pub async fn run(args: LogArgs) -> Result<()> {
    let result = fetch(
        args.app.as_deref(),
        args.tag.as_deref(),
        &args.level,
        args.lines,
    )?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        for entry in &result.entries {
            if entry.tag.is_empty() {
                println!("{}", entry.message);
            } else {
                println!(
                    "{} {} {}/{}: {}",
                    entry.timestamp, entry.pid, entry.level, entry.tag, entry.message
                );
            }
        }
        println!("--- {} entries ---", result.total);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_level_filter --

    #[test]
    fn level_filter_full_names() {
        assert_eq!(parse_level_filter("verbose"), "*:V");
        assert_eq!(parse_level_filter("debug"), "*:D");
        assert_eq!(parse_level_filter("info"), "*:I");
        assert_eq!(parse_level_filter("warn"), "*:W");
        assert_eq!(parse_level_filter("error"), "*:E");
        assert_eq!(parse_level_filter("fatal"), "*:F");
    }

    #[test]
    fn level_filter_short_names() {
        assert_eq!(parse_level_filter("v"), "*:V");
        assert_eq!(parse_level_filter("d"), "*:D");
        assert_eq!(parse_level_filter("i"), "*:I");
        assert_eq!(parse_level_filter("w"), "*:W");
        assert_eq!(parse_level_filter("e"), "*:E");
        assert_eq!(parse_level_filter("f"), "*:F");
    }

    #[test]
    fn level_filter_case_insensitive() {
        assert_eq!(parse_level_filter("INFO"), "*:I");
        assert_eq!(parse_level_filter("Error"), "*:E");
        assert_eq!(parse_level_filter("VERBOSE"), "*:V");
    }

    #[test]
    fn level_filter_unknown_defaults_to_verbose() {
        assert_eq!(parse_level_filter("unknown"), "*:V");
        assert_eq!(parse_level_filter(""), "*:V");
        assert_eq!(parse_level_filter("trace"), "*:V");
    }

    // -- parse_logcat_line --

    #[test]
    fn parse_standard_threadtime_line() {
        let line = "03-31 00:12:34.567  1234  5678 I MyTag   : Hello world";
        let entry = parse_logcat_line(line).unwrap();
        assert_eq!(entry.timestamp, "03-31 00:12:34.567");
        assert_eq!(entry.pid, "1234");
        assert_eq!(entry.tid, "5678");
        assert_eq!(entry.level, "I");
        assert_eq!(entry.tag, "MyTag");
        assert_eq!(entry.message, "Hello world");
    }

    #[test]
    fn parse_empty_line_returns_none() {
        assert!(parse_logcat_line("").is_none());
        assert!(parse_logcat_line("   ").is_none());
    }

    #[test]
    fn parse_separator_line_returns_none() {
        assert!(parse_logcat_line("--------- beginning of main").is_none());
        assert!(parse_logcat_line("--------- beginning of system").is_none());
    }

    #[test]
    fn parse_line_without_colon_falls_back() {
        let line = "some random text without colon separator";
        let entry = parse_logcat_line(line).unwrap();
        assert_eq!(entry.message, line);
        assert!(entry.timestamp.is_empty());
        assert!(entry.tag.is_empty());
    }

    #[test]
    fn parse_short_prefix_falls_back() {
        let line = "short: message here";
        let entry = parse_logcat_line(line).unwrap();
        // Not enough prefix parts for structured parse, falls back
        assert_eq!(entry.message, "short: message here");
    }

    #[test]
    fn parse_error_level_line() {
        let line = "03-31 12:00:00.000  9999  9999 E CrashTag: NullPointerException";
        let entry = parse_logcat_line(line).unwrap();
        assert_eq!(entry.level, "E");
        assert_eq!(entry.tag, "CrashTag");
        assert_eq!(entry.message, "NullPointerException");
    }

    #[test]
    fn parse_message_with_colons() {
        let line = "03-31 12:00:00.000  1000  1000 D NetTag  : url: https://example.com: ok";
        let entry = parse_logcat_line(line).unwrap();
        assert_eq!(entry.message, "url: https://example.com: ok");
    }
}
