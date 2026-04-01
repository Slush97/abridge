use anyhow::{Context, Result};

use crate::adb;
use crate::cli::{InputAction, InputArgs};

/// Map friendly key names to Android keyevent codes.
fn keycode_for(name: &str) -> Result<u32> {
    match name.to_lowercase().as_str() {
        "home" => Ok(3),
        "back" => Ok(4),
        "call" => Ok(5),
        "endcall" => Ok(6),
        "dpad_center" | "enter" => Ok(66),
        "menu" => Ok(82),
        "search" => Ok(84),
        "power" => Ok(26),
        "volup" | "volume_up" => Ok(24),
        "voldown" | "volume_down" => Ok(25),
        "tab" => Ok(61),
        "delete" | "backspace" => Ok(67),
        "recent" | "app_switch" => Ok(187),
        "camera" => Ok(27),
        _ => anyhow::bail!("Unknown key name: {name}. Use: home, back, enter, menu, power, volup, voldown, tab, delete, recent"),
    }
}

/// Escape text for `adb shell input text`.
/// Spaces become %s, and shell metacharacters are backslash-escaped.
fn escape_for_input(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            ' ' => "%s".to_string(),
            '\'' | '"' | '\\' | '`' | '$' | '!' | '(' | ')' | '&' | '|' | ';' | '<' | '>' | '{'
            | '}' | '[' | ']' | '#' | '~' | '?' | '*' => format!("\\{c}"),
            _ => c.to_string(),
        })
        .collect()
}

/// Send text input to the device.
pub fn input_text(text: &str) -> Result<()> {
    let escaped = escape_for_input(text);
    adb::shell_str(&format!("input text {escaped}")).context("Failed to send text input")?;
    Ok(())
}

/// Send a tap at coordinates.
pub fn tap(x: u32, y: u32) -> Result<()> {
    adb::shell_str(&format!("input tap {x} {y}")).context("Failed to send tap")?;
    Ok(())
}

/// Send a swipe gesture.
pub fn swipe(x1: u32, y1: u32, x2: u32, y2: u32, duration_ms: u32) -> Result<()> {
    adb::shell_str(&format!("input swipe {x1} {y1} {x2} {y2} {duration_ms}"))
        .context("Failed to send swipe")?;
    Ok(())
}

/// Send a key event.
pub fn key(name: &str) -> Result<()> {
    let code = keycode_for(name)?;
    adb::shell_str(&format!("input keyevent {code}")).context("Failed to send key event")?;
    Ok(())
}

/// Push text to device clipboard via a broadcast.
///
/// Returns a status message indicating whether the broadcast was received.
/// Requires the Clipper app (available on F-Droid) for reliable operation.
pub fn set_clipboard(text: &str) -> Result<String> {
    let escaped = text.replace('\'', "'\\''");
    let output = adb::shell_str(&format!("am broadcast -a clipper.set -e text '{escaped}'"))
        .context("Failed to send clipboard broadcast")?;

    if output.contains("result=-1") || output.contains("data=") {
        Ok("Clipboard set".to_string())
    } else {
        Ok("Clipboard broadcast sent but no receiver confirmed it. \
             Install the Clipper app (F-Droid) for reliable clipboard support, \
             or use 'text' input type to type directly."
            .to_string())
    }
}

/// CLI entry point.
pub async fn run(args: InputArgs) -> Result<()> {
    match args.action {
        InputAction::Text { value } => {
            input_text(&value)?;
            println!("Typed: {value}");
        }
        InputAction::Tap { x, y } => {
            tap(x, y)?;
            println!("Tapped at ({x}, {y})");
        }
        InputAction::Swipe {
            x1,
            y1,
            x2,
            y2,
            duration,
        } => {
            swipe(x1, y1, x2, y2, duration)?;
            println!("Swiped ({x1},{y1}) -> ({x2},{y2}) in {duration}ms");
        }
        InputAction::Key { name } => {
            key(&name)?;
            println!("Sent key: {name}");
        }
        InputAction::Clip { text } => {
            let msg = set_clipboard(&text)?;
            println!("{msg}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycode_known_keys() {
        assert_eq!(keycode_for("home").unwrap(), 3);
        assert_eq!(keycode_for("back").unwrap(), 4);
        assert_eq!(keycode_for("enter").unwrap(), 66);
        assert_eq!(keycode_for("menu").unwrap(), 82);
        assert_eq!(keycode_for("power").unwrap(), 26);
        assert_eq!(keycode_for("volup").unwrap(), 24);
        assert_eq!(keycode_for("voldown").unwrap(), 25);
        assert_eq!(keycode_for("tab").unwrap(), 61);
        assert_eq!(keycode_for("delete").unwrap(), 67);
        assert_eq!(keycode_for("recent").unwrap(), 187);
        assert_eq!(keycode_for("camera").unwrap(), 27);
        assert_eq!(keycode_for("call").unwrap(), 5);
        assert_eq!(keycode_for("endcall").unwrap(), 6);
        assert_eq!(keycode_for("search").unwrap(), 84);
    }

    #[test]
    fn keycode_aliases() {
        assert_eq!(keycode_for("dpad_center").unwrap(), 66);
        assert_eq!(keycode_for("volume_up").unwrap(), 24);
        assert_eq!(keycode_for("volume_down").unwrap(), 25);
        assert_eq!(keycode_for("backspace").unwrap(), 67);
        assert_eq!(keycode_for("app_switch").unwrap(), 187);
    }

    #[test]
    fn keycode_case_insensitive() {
        assert_eq!(keycode_for("HOME").unwrap(), 3);
        assert_eq!(keycode_for("Back").unwrap(), 4);
        assert_eq!(keycode_for("ENTER").unwrap(), 66);
    }

    #[test]
    fn keycode_unknown_is_err() {
        assert!(keycode_for("nonexistent").is_err());
        assert!(keycode_for("").is_err());
    }

    #[test]
    fn escape_plain_text() {
        assert_eq!(escape_for_input("hello"), "hello");
        assert_eq!(escape_for_input("abc123"), "abc123");
    }

    #[test]
    fn escape_spaces_become_percent_s() {
        assert_eq!(escape_for_input("hello world"), "hello%sworld");
        assert_eq!(escape_for_input("a b c"), "a%sb%sc");
    }

    #[test]
    fn escape_shell_metacharacters() {
        assert_eq!(escape_for_input("a'b"), "a\\'b");
        assert_eq!(escape_for_input("a\"b"), "a\\\"b");
        assert_eq!(escape_for_input("a$b"), "a\\$b");
        assert_eq!(escape_for_input("a&b"), "a\\&b");
        assert_eq!(escape_for_input("a|b"), "a\\|b");
        assert_eq!(escape_for_input("a;b"), "a\\;b");
        assert_eq!(escape_for_input("a<b"), "a\\<b");
        assert_eq!(escape_for_input("a>b"), "a\\>b");
    }

    #[test]
    fn escape_all_special_chars() {
        let special = "'\"\\`$!()&|;<>{}[]#~?*";
        let escaped = escape_for_input(special);
        for c in special.chars() {
            assert!(
                escaped.contains(&format!("\\{c}")),
                "char '{c}' should be escaped"
            );
        }
    }

    #[test]
    fn escape_mixed_content() {
        assert_eq!(escape_for_input("hello world!"), "hello%sworld\\!");
    }
}
