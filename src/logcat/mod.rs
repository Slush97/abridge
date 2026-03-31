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

    // Try to parse the standard threadtime format
    let parts: Vec<&str> = line.splitn(6, char::is_whitespace).collect();
    if parts.len() < 6 {
        return Some(LogEntry {
            timestamp: String::new(),
            pid: String::new(),
            tid: String::new(),
            level: String::new(),
            tag: String::new(),
            message: line.to_string(),
        });
    }

    // More robust parsing: find level character and tag
    let rest = line;
    if let Some(colon_pos) = rest.find(": ") {
        let prefix = &rest[..colon_pos];
        let message = rest[colon_pos + 2..].to_string();

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
        // Get PID(s) for the package — pidof can return multiple space-separated PIDs
        let pid_output = adb::shell_str(&format!("pidof {package}"))?;
        let pid = pid_output.trim();
        if pid.is_empty() {
            anyhow::bail!("App {package} is not running");
        }
        // Use the first PID (main process). --pid only accepts one value.
        let main_pid = pid.split_whitespace().next().unwrap_or(pid);
        format!("logcat -d -v threadtime --pid={main_pid} {level_filter} -t {lines}")
    } else {
        format!("logcat -d -v threadtime {level_filter} -t {lines}")
    };

    let output = adb::shell_str(&cmd).context("Failed to read logcat")?;

    let mut entries: Vec<LogEntry> = output
        .lines()
        .filter_map(parse_logcat_line)
        .collect();

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
