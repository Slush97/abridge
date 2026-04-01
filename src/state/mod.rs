use anyhow::{Context, Result};
use serde::Serialize;

use crate::adb;
use crate::cli::{CrashArgs, StateArgs};

#[derive(Debug, Serialize)]
pub struct DeviceState {
    pub current_activity: String,
    pub resumed_activities: Vec<String>,
    pub fragment_backstack: String,
    pub display_info: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryInfo>,
}

#[derive(Debug, Serialize)]
pub struct MemoryInfo {
    pub total_ram: String,
    pub free_ram: String,
    pub available_ram: String,
}

#[derive(Debug, Serialize)]
pub struct CrashReport {
    pub stacktrace: String,
    pub current_activity: String,
    pub recent_logcat: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_path: Option<String>,
}

/// Get the currently focused activity.
pub fn current_activity() -> Result<String> {
    let output = adb::shell_str("dumpsys activity activities | grep mResumedActivity")
        .context("Failed to get current activity")?;
    Ok(output.trim().to_string())
}

/// Get all resumed activities.
pub fn resumed_activities() -> Result<Vec<String>> {
    let output =
        adb::shell_str("dumpsys activity activities | grep -E 'mResumedActivity|ResumedActivity'")?;
    Ok(output.lines().map(|l| l.trim().to_string()).collect())
}

/// Get fragment backstack info for the foreground app.
pub fn fragment_backstack() -> Result<String> {
    let output = adb::shell_str(
        "dumpsys activity top | grep -E 'Added Fragments|Back Stack|#[0-9]+:' | head -20",
    )?;
    Ok(output.trim().to_string())
}

/// Get display/resolution info.
pub fn display_info() -> Result<String> {
    // Use wm size/density for concise output; fall back to dumpsys
    let size = adb::shell_str("wm size").context("Failed to get display size")?;
    let density = adb::shell_str("wm density").context("Failed to get display density")?;
    let result = format!("{} {}", size.trim(), density.trim());
    if result.trim().is_empty() {
        let output = adb::shell_str("dumpsys display | grep -E 'mBaseDisplayInfo'")?;
        // Truncate to first 500 chars to avoid massive responses
        Ok(output.trim().chars().take(500).collect())
    } else {
        Ok(result)
    }
}

/// Get memory stats.
pub fn memory_info() -> Result<MemoryInfo> {
    let output = adb::shell_str("cat /proc/meminfo | head -3")?;
    let lines: Vec<&str> = output.lines().collect();

    Ok(MemoryInfo {
        total_ram: lines.first().unwrap_or(&"unknown").trim().to_string(),
        free_ram: lines.get(1).unwrap_or(&"unknown").trim().to_string(),
        available_ram: lines.get(2).unwrap_or(&"unknown").trim().to_string(),
    })
}

/// Get full device state snapshot.
pub fn get_state(include_memory: bool) -> Result<DeviceState> {
    let memory = if include_memory {
        Some(memory_info()?)
    } else {
        None
    };

    Ok(DeviceState {
        current_activity: current_activity()?,
        resumed_activities: resumed_activities()?,
        fragment_backstack: fragment_backstack()?,
        display_info: display_info()?,
        memory,
    })
}

/// Get recent crash info.
pub fn get_crash_report(include_screenshot: bool) -> Result<CrashReport> {
    let stacktrace = adb::shell_str("logcat -b crash -d -t 50")
        .unwrap_or_else(|_| "No crash log available".to_string());

    let activity = current_activity().unwrap_or_else(|_| "unknown".to_string());

    let recent = adb::shell_str("logcat -d -t 30 *:E").unwrap_or_default();
    let recent_logcat: Vec<String> = recent.lines().map(|l| l.to_string()).collect();

    let screenshot_path = if include_screenshot {
        if let Ok(png) = crate::screen::capture_screenshot() {
            let path = std::env::temp_dir()
                .join(format!(
                    "adbridge_crash_{}.png",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ))
                .to_string_lossy()
                .to_string();
            if std::fs::write(&path, &png).is_ok() {
                Some(path)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    Ok(CrashReport {
        stacktrace,
        current_activity: activity,
        recent_logcat,
        screenshot_path,
    })
}

/// CLI entry point for `state` command.
pub async fn run(args: StateArgs) -> Result<()> {
    let state = get_state(args.memory)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&state)?);
    } else {
        println!("Current Activity: {}", state.current_activity);
        if !state.resumed_activities.is_empty() {
            println!("\nResumed Activities:");
            for a in &state.resumed_activities {
                println!("  {a}");
            }
        }
        if !state.fragment_backstack.is_empty() {
            println!("\nFragment Backstack:\n{}", state.fragment_backstack);
        }
        println!("\nDisplay: {}", state.display_info);
        if let Some(ref mem) = state.memory {
            println!("\nMemory:");
            println!("  {}", mem.total_ram);
            println!("  {}", mem.free_ram);
            println!("  {}", mem.available_ram);
        }
    }

    Ok(())
}

/// CLI entry point for `crash` command.
pub async fn crash(args: CrashArgs) -> Result<()> {
    let report = get_crash_report(true)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("=== Crash Report ===\n");
        println!("Current Activity: {}\n", report.current_activity);
        println!("--- Crash Log ---");
        println!("{}\n", report.stacktrace);
        println!("--- Recent Errors ({}) ---", report.recent_logcat.len());
        for line in &report.recent_logcat {
            println!("  {line}");
        }
        if let Some(ref path) = report.screenshot_path {
            println!("\nScreenshot saved to {path}");
        }
    }

    Ok(())
}
