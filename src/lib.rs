#![deny(unsafe_code)]

//! # adbridge
//!
//! Android Bridge for AI-Assisted Development.
//!
//! `adbridge` provides programmatic access to Android devices over ADB:
//! screenshots, OCR, UI element parsing, logcat, input control, device state,
//! and crash reports. It works as a standalone CLI, an MCP server for AI
//! assistants, or a Rust library you can embed in your own tools.
//!
//! ## Library Quick Start
//!
//! ```rust,no_run
//! # fn main() -> anyhow::Result<()> {
//! // Screenshot + OCR
//! let png = adbridge::screen::capture_screenshot()?;
//! let text = adbridge::screen::ocr_image(&png)?;
//! println!("{}", adbridge::screen::clean_ocr_text(&text));
//!
//! // Parse interactive UI elements with tap coordinates
//! let xml = adbridge::screen::dump_hierarchy()?;
//! let elements = adbridge::screen::elements::parse_elements(&xml, true);
//! for el in &elements {
//!     println!("{el}"); // e.g. "[1] Button "Login" (540, 750)"
//! }
//!
//! // Tap, type, send keys
//! adbridge::input::tap(540, 750)?;
//! adbridge::input::input_text("hello")?;
//! adbridge::input::key("enter")?;
//!
//! // Read logcat errors
//! let logs = adbridge::logcat::fetch(Some("com.example"), None, "error", 20)?;
//! for e in &logs.entries {
//!     println!("{}/{}: {}", e.level, e.tag, e.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Pure Functions (No Device Required)
//!
//! Several functions work offline for processing ADB output in your own pipelines:
//!
//! - [`screen::strip_hierarchy`] -- shrink view hierarchy XML by removing default attributes
//! - [`screen::clean_ocr_text`] -- filter OCR noise from Tesseract output
//! - [`screen::compress_screenshot`] -- downscale and JPEG-compress PNG screenshots
//! - [`screen::elements::parse_elements`] -- parse hierarchy XML into [`screen::elements::UiElement`]s
//!
//! ## Prerequisites
//!
//! - ADB server running (`adb start-server`)
//! - Tesseract OCR installed (for OCR features only)
//! - See the [README](https://github.com/Slush97/adbridge) for full setup instructions.

pub mod adb;
pub mod cli;
pub mod input;
pub mod logcat;
pub mod mcp;
pub mod screen;
pub mod state;
