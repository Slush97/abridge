use anyhow::Result;
use base64::Engine;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use serde::Deserialize;

/// Deserialize a bool that might arrive as a string "true"/"false" from MCP clients.
fn bool_from_string_or_bool<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<bool, D::Error> {
    use serde::de;

    struct BoolVisitor;
    impl<'de> de::Visitor<'de> for BoolVisitor {
        type Value = bool;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a boolean or string \"true\"/\"false\"")
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<bool, E> {
            Ok(v)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<bool, E> {
            match v {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(E::custom(format!("expected true/false, got {v}"))),
            }
        }
    }
    deserializer.deserialize_any(BoolVisitor)
}

/// Convert an error into an MCP internal error response.
fn mcp_err(e: impl std::fmt::Display) -> rmcp::ErrorData {
    rmcp::ErrorData::new(rmcp::model::ErrorCode::INTERNAL_ERROR, e.to_string(), None)
}

/// Create an MCP invalid-params error response.
fn mcp_invalid(msg: &str) -> rmcp::ErrorData {
    rmcp::ErrorData::new(
        rmcp::model::ErrorCode::INVALID_PARAMS,
        msg.to_string(),
        None,
    )
}

#[derive(Debug, Clone)]
pub struct AbridgeMcp {
    tool_router: ToolRouter<Self>,
}

impl Default for AbridgeMcp {
    fn default() -> Self {
        Self::new()
    }
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
    /// Target device serial (uses first connected device if omitted)
    pub device: Option<String>,
    /// Whether to run OCR on the screenshot
    #[serde(default, deserialize_with = "bool_from_string_or_bool")]
    pub ocr: bool,
    /// Whether to include the view hierarchy
    #[serde(default, deserialize_with = "bool_from_string_or_bool")]
    pub hierarchy: bool,
    /// Whether to include parsed interactive UI elements with tap coordinates
    #[serde(default, deserialize_with = "bool_from_string_or_bool")]
    pub elements: bool,
    /// Return full-resolution PNG instead of compressed JPEG (default: false)
    #[serde(default, deserialize_with = "bool_from_string_or_bool")]
    pub full_resolution: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogcatParams {
    /// Target device serial (uses first connected device if omitted)
    pub device: Option<String>,
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
    /// Target device serial (uses first connected device if omitted)
    pub device: Option<String>,
    /// Input type: "text", "tap", "swipe", "key", "clip"
    pub r#type: String,
    /// The value: text content, key name, or coordinates as "x,y" or "x1,y1,x2,y2"
    pub value: String,
    /// Duration for swipe in ms (optional)
    pub duration: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShellParams {
    /// Target device serial (uses first connected device if omitted)
    pub device: Option<String>,
    /// Shell command to execute on the device (e.g., "getprop ro.build.fingerprint")
    pub command: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CrashParams {
    /// Target device serial (uses first connected device if omitted)
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StateParams {
    /// Target device serial (uses first connected device if omitted)
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeviceInfoParams {}

#[tool_router]
impl AbridgeMcp {
    #[tool(
        description = "Capture a screenshot from the connected Android device. Returns the image, optional OCR text, optional view hierarchy XML, and optional parsed interactive UI elements with tap coordinates."
    )]
    async fn device_screenshot(
        &self,
        Parameters(params): Parameters<ScreenshotParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        crate::adb::set_target_device(params.device.clone());
        let png_data = crate::screen::capture_screenshot()
            .map_err(|e| crate::mcp::mcp_err(format!("Screenshot failed: {e}")))?;

        let mut contents = Vec::new();

        // Return full-res PNG or compressed JPEG based on flag
        let (image_data, mime) = if params.full_resolution {
            (png_data.clone(), "image/png")
        } else {
            match crate::screen::compress_screenshot(&png_data, 720, 80) {
                Ok(jpeg) => (jpeg, "image/jpeg"),
                Err(_) => (png_data.clone(), "image/png"),
            }
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&image_data);
        contents.push(Content::image(b64, mime));

        // OCR text (cleaned to remove noise lines)
        if params.ocr {
            match crate::screen::ocr_image(&png_data) {
                Ok(text) => {
                    let cleaned = crate::screen::clean_ocr_text(&text);
                    if !cleaned.is_empty() {
                        contents.push(Content::text(format!("--- OCR Text ---\n{cleaned}")));
                    } else {
                        contents.push(Content::text(
                            "--- OCR Text ---\n(no readable text detected)".to_string(),
                        ));
                    }
                }
                Err(e) => contents.push(Content::text(format!("OCR failed: {e}"))),
            }
        }

        // Auto-include elements when no structural flags are set
        let include_elements = params.elements || !params.hierarchy;

        // Fetch hierarchy once if hierarchy, elements, or auto-elements is needed
        let hierarchy_xml = if params.hierarchy || include_elements {
            match crate::screen::dump_hierarchy() {
                Ok(xml) => Some(xml),
                Err(e) => {
                    contents.push(Content::text(format!("Hierarchy dump failed: {e}")));
                    None
                }
            }
        } else {
            None
        };

        // View hierarchy (stripped of default/false attributes)
        if params.hierarchy {
            if let Some(ref xml) = hierarchy_xml {
                let stripped = crate::screen::strip_hierarchy(xml);
                contents.push(Content::text(format!("--- View Hierarchy ---\n{stripped}")));
            }
        }

        // Parsed interactive elements
        if include_elements {
            if let Some(ref xml) = hierarchy_xml {
                let parsed = crate::screen::elements::parse_elements(xml, true);
                let text = crate::screen::elements::format_elements(&parsed);
                contents.push(Content::text(format!("--- UI Elements ---\n{text}")));
            }
        }

        Ok(CallToolResult::success(contents))
    }

    #[tool(
        description = "Get filtered logcat entries from the connected Android device. Can filter by app, tag, and log level."
    )]
    async fn device_logcat(
        &self,
        Parameters(params): Parameters<LogcatParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        crate::adb::set_target_device(params.device.clone());
        let result = crate::logcat::fetch(
            params.app.as_deref(),
            params.tag.as_deref(),
            &params.level,
            params.lines,
        )
        .map_err(crate::mcp::mcp_err)?;

        let json = serde_json::to_string_pretty(&result).map_err(crate::mcp::mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get current device state: focused activity, fragment backstack, display info, and memory stats."
    )]
    async fn device_state(
        &self,
        Parameters(params): Parameters<StateParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        crate::adb::set_target_device(params.device);
        let result = crate::state::get_state(true).map_err(crate::mcp::mcp_err)?;
        let json = serde_json::to_string_pretty(&result).map_err(crate::mcp::mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Send input to the Android device. Types: 'text' (type text), 'tap' (value='x,y'), 'swipe' (value='x1,y1,x2,y2'), 'key' (value=home/back/enter/menu), 'clip' (set clipboard)."
    )]
    async fn device_input(
        &self,
        Parameters(params): Parameters<InputParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        crate::adb::set_target_device(params.device.clone());
        let message = match params.r#type.as_str() {
            "text" => {
                crate::input::input_text(&params.value).map_err(crate::mcp::mcp_err)?;
                format!("OK: text '{}'", params.value)
            }
            "tap" => {
                let coords: Vec<u32> = params
                    .value
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                if coords.len() != 2 {
                    return Err(crate::mcp::mcp_invalid("tap value must be 'x,y'"));
                }
                crate::input::tap(coords[0], coords[1]).map_err(crate::mcp::mcp_err)?;
                format!("OK: tap '{}'", params.value)
            }
            "swipe" => {
                let coords: Vec<u32> = params
                    .value
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                if coords.len() != 4 {
                    return Err(crate::mcp::mcp_invalid("swipe value must be 'x1,y1,x2,y2'"));
                }
                crate::input::swipe(
                    coords[0],
                    coords[1],
                    coords[2],
                    coords[3],
                    params.duration.unwrap_or(300),
                )
                .map_err(crate::mcp::mcp_err)?;
                format!("OK: swipe '{}'", params.value)
            }
            "key" => {
                crate::input::key(&params.value).map_err(crate::mcp::mcp_err)?;
                format!("OK: key '{}'", params.value)
            }
            "clip" => crate::input::set_clipboard(&params.value).map_err(crate::mcp::mcp_err)?,
            other => {
                return Err(crate::mcp::mcp_invalid(&format!(
                    "Unknown input type: {other}"
                )))
            }
        };

        Ok(CallToolResult::success(vec![Content::text(message)]))
    }

    #[tool(
        description = "List connected Android devices with model, Android version, and SDK version."
    )]
    async fn device_info(
        &self,
        Parameters(_params): Parameters<DeviceInfoParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let devices = crate::adb::connection::list_devices().map_err(crate::mcp::mcp_err)?;
        let json = serde_json::to_string_pretty(&devices).map_err(crate::mcp::mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get the most recent crash report: stacktrace, current activity, recent error logs, and a screenshot saved to /tmp."
    )]
    async fn device_crash_report(
        &self,
        Parameters(params): Parameters<CrashParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        crate::adb::set_target_device(params.device);
        let report = crate::state::get_crash_report(true).map_err(crate::mcp::mcp_err)?;
        let json = serde_json::to_string_pretty(&report).map_err(crate::mcp::mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Run a raw ADB shell command on the device. Use for one-off queries not covered by other tools (e.g., getprop, pm list, dumpsys). Returns stdout as text."
    )]
    async fn device_shell(
        &self,
        Parameters(params): Parameters<ShellParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        crate::adb::set_target_device(params.device);
        let output = crate::adb::shell_str(&params.command).map_err(crate::mcp::mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
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
    tracing::info!("Starting adbridge MCP server on stdio");

    let service = AbridgeMcp::new();
    let server = service.serve(rmcp::transport::stdio()).await?;
    server.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_deserialize_from_true() {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct T {
            #[serde(deserialize_with = "bool_from_string_or_bool")]
            v: bool,
        }
        let t: T = serde_json::from_str(r#"{"v": true}"#).unwrap();
        assert!(t.v);
    }

    #[test]
    fn bool_deserialize_from_false() {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct T {
            #[serde(deserialize_with = "bool_from_string_or_bool")]
            v: bool,
        }
        let t: T = serde_json::from_str(r#"{"v": false}"#).unwrap();
        assert!(!t.v);
    }

    #[test]
    fn bool_deserialize_from_string_true() {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct T {
            #[serde(deserialize_with = "bool_from_string_or_bool")]
            v: bool,
        }
        let t: T = serde_json::from_str(r#"{"v": "true"}"#).unwrap();
        assert!(t.v);
    }

    #[test]
    fn bool_deserialize_from_string_false() {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct T {
            #[serde(deserialize_with = "bool_from_string_or_bool")]
            v: bool,
        }
        let t: T = serde_json::from_str(r#"{"v": "false"}"#).unwrap();
        assert!(!t.v);
    }

    #[test]
    fn bool_deserialize_invalid_string_is_err() {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct T {
            #[serde(deserialize_with = "bool_from_string_or_bool")]
            v: bool,
        }
        assert!(serde_json::from_str::<T>(r#"{"v": "yes"}"#).is_err());
        assert!(serde_json::from_str::<T>(r#"{"v": "1"}"#).is_err());
    }
}
