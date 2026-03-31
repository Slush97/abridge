use anyhow::Result;
use base64::Engine;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

/// Deserialize a bool that might arrive as a string "true"/"false" from MCP clients.
fn bool_from_string_or_bool<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<bool, D::Error> {
    use serde::de;

    struct BoolVisitor;
    impl<'de> de::Visitor<'de> for BoolVisitor {
        type Value = bool;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a boolean or string \"true\"/\"false\"")
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<bool, E> { Ok(v) }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<bool, E> {
            match v { "true" => Ok(true), "false" => Ok(false), _ => Err(E::custom(format!("expected true/false, got {v}"))) }
        }
    }
    deserializer.deserialize_any(BoolVisitor)
}

#[derive(Debug, Clone)]
pub struct AbridgeMcp {
    tool_router: ToolRouter<Self>,
}

impl AbridgeMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScreenshotParams {
    /// Whether to run OCR on the screenshot
    #[serde(default, deserialize_with = "bool_from_string_or_bool")]
    pub ocr: bool,
    /// Whether to include the view hierarchy
    #[serde(default, deserialize_with = "bool_from_string_or_bool")]
    pub hierarchy: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogcatParams {
    /// Filter by app package name
    pub app: Option<String>,
    /// Filter by log tag
    pub tag: Option<String>,
    /// Minimum log level (verbose, debug, info, warn, error, fatal)
    #[serde(default = "default_level")]
    pub level: String,
    /// Number of recent lines
    #[serde(default = "default_lines")]
    pub lines: u32,
}

fn default_level() -> String {
    "verbose".to_string()
}

fn default_lines() -> u32 {
    50
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InputParams {
    /// Input type: "text", "tap", "swipe", "key", "clip"
    pub r#type: String,
    /// The value: text content, key name, or coordinates as "x,y" or "x1,y1,x2,y2"
    pub value: String,
    /// Duration for swipe in ms (optional)
    pub duration: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CrashParams {
    /// Path to source root for stacktrace mapping
    pub source_root: Option<String>,
}

#[tool_router]
impl AbridgeMcp {
    #[tool(description = "Capture a screenshot from the connected Android device. Returns the image, optional OCR text, and optional view hierarchy XML.")]
    async fn device_screenshot(
        &self,
        Parameters(params): Parameters<ScreenshotParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let png_data = crate::screen::capture_screenshot()
            .map_err(|e| rmcp::ErrorData::new(rmcp::model::ErrorCode::INTERNAL_ERROR, format!("Screenshot failed: {e}"), None))?;

        let mut contents = Vec::new();

        // Return image as an MCP image content block
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);
        contents.push(Content::image(b64, "image/png"));

        // OCR text
        if params.ocr {
            match crate::screen::ocr_image(&png_data) {
                Ok(text) => contents.push(Content::text(format!("--- OCR Text ---\n{text}"))),
                Err(e) => contents.push(Content::text(format!("OCR failed: {e}"))),
            }
        }

        // View hierarchy
        if params.hierarchy {
            match crate::screen::dump_hierarchy() {
                Ok(xml) => contents.push(Content::text(format!("--- View Hierarchy ---\n{xml}"))),
                Err(e) => contents.push(Content::text(format!("Hierarchy dump failed: {e}"))),
            }
        }

        Ok(CallToolResult::success(contents))
    }

    #[tool(description = "Get filtered logcat entries from the connected Android device. Can filter by app, tag, and log level.")]
    async fn device_logcat(
        &self,
        Parameters(params): Parameters<LogcatParams>,
    ) -> String {
        match crate::logcat::fetch(
            params.app.as_deref(),
            params.tag.as_deref(),
            &params.level,
            params.lines,
        ) {
            Ok(result) => serde_json::to_string_pretty(&result).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error reading logcat: {e}"),
        }
    }

    #[tool(description = "Get current device state: focused activity, fragment backstack, display info, and memory stats.")]
    async fn device_state(&self) -> String {
        match crate::state::get_state(true) {
            Ok(result) => serde_json::to_string_pretty(&result).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error getting device state: {e}"),
        }
    }

    #[tool(description = "Send input to the Android device. Types: 'text' (type text), 'tap' (value='x,y'), 'swipe' (value='x1,y1,x2,y2'), 'key' (value=home/back/enter/menu), 'clip' (set clipboard).")]
    async fn device_input(
        &self,
        Parameters(params): Parameters<InputParams>,
    ) -> String {
        let result = match params.r#type.as_str() {
            "text" => crate::input::input_text(&params.value),
            "tap" => {
                let coords: Vec<u32> = params
                    .value
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                if coords.len() == 2 {
                    crate::input::tap(coords[0], coords[1])
                } else {
                    return "Error: tap value must be 'x,y'".to_string();
                }
            }
            "swipe" => {
                let coords: Vec<u32> = params
                    .value
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                if coords.len() == 4 {
                    crate::input::swipe(
                        coords[0],
                        coords[1],
                        coords[2],
                        coords[3],
                        params.duration.unwrap_or(300),
                    )
                } else {
                    return "Error: swipe value must be 'x1,y1,x2,y2'".to_string();
                }
            }
            "key" => crate::input::key(&params.value),
            "clip" => crate::input::set_clipboard(&params.value),
            other => return format!("Unknown input type: {other}"),
        };

        match result {
            Ok(()) => format!("OK: {} '{}'", params.r#type, params.value),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get the most recent crash report: stacktrace, current activity, recent error logs, and a screenshot saved to /tmp.")]
    async fn device_crash_report(
        &self,
        Parameters(_params): Parameters<CrashParams>,
    ) -> String {
        // Don't include base64 screenshot — save to file to avoid token limits
        match crate::state::get_crash_report(false) {
            Ok(mut report) => {
                // Save screenshot to temp file instead
                if let Ok(png) = crate::screen::capture_screenshot() {
                    let path = format!(
                        "/tmp/abridge_crash_{}.png",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis()
                    );
                    if std::fs::write(&path, &png).is_ok() {
                        report.screenshot_base64 = Some(format!("saved:{path}"));
                    }
                }
                serde_json::to_string_pretty(&report).unwrap_or_else(|e| e.to_string())
            }
            Err(e) => format!("Error getting crash report: {e}"),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AbridgeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Android Bridge for AI-Assisted Development. Provides device screenshot/OCR, logcat, input control, state inspection, and crash reports.")
    }
}

/// Start the MCP server on stdio.
pub async fn serve() -> Result<()> {
    tracing::info!("Starting abridge MCP server on stdio");

    let service = AbridgeMcp::new();
    let server = service
        .serve(rmcp::transport::stdio())
        .await?;
    server.waiting().await?;

    Ok(())
}
