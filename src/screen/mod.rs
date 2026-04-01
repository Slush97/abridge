pub mod elements;

use anyhow::{Context, Result};
use base64::Engine;
use serde::Serialize;

use crate::adb;
use crate::cli::ScreenArgs;

#[derive(Debug, Serialize)]
pub struct ScreenCapture {
    /// Base64-encoded PNG screenshot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_base64: Option<String>,

    /// OCR-extracted text from the screen
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_text: Option<String>,

    /// View hierarchy XML
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hierarchy: Option<String>,

    /// Parsed interactive UI elements (compact text format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elements: Option<String>,

    /// Path where screenshot was saved (if --output used)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_to: Option<String>,
}

/// Take a screenshot from the device as raw PNG data.
///
/// The device must be connected and accessible via ADB.
///
/// # Examples
///
/// ```rust,no_run
/// # fn main() -> anyhow::Result<()> {
/// let png = adbridge::screen::capture_screenshot()?;
/// std::fs::write("screenshot.png", &png)?;
/// # Ok(())
/// # }
/// ```
pub fn capture_screenshot() -> Result<Vec<u8>> {
    adb::shell("screencap -p").context("Failed to capture screenshot")
}

/// Dump the view hierarchy via uiautomator as XML.
///
/// Tries `/dev/tty` first (zero-copy, works on most devices). Falls back to
/// dumping to a temp file and reading it back (required on Android 16+).
///
/// The returned XML can be processed with [`strip_hierarchy`] to reduce size
/// or [`elements::parse_elements`] to extract interactive UI elements.
///
/// # Examples
///
/// ```rust,no_run
/// # fn main() -> anyhow::Result<()> {
/// let xml = adbridge::screen::dump_hierarchy()?;
///
/// // Reduce size for AI consumption
/// let stripped = adbridge::screen::strip_hierarchy(&xml);
///
/// // Or extract interactive elements
/// let elements = adbridge::screen::elements::parse_elements(&xml, true);
/// for el in &elements {
///     println!("{el}");
/// }
/// # Ok(())
/// # }
/// ```
pub fn dump_hierarchy() -> Result<String> {
    let output = adb::shell_str("uiautomator dump /dev/tty 2>/dev/null")
        .context("Failed to dump view hierarchy")?;

    // On most devices the XML is written to /dev/tty and captured in output.
    // On Android 16+ only the status line comes back. Detect by checking for XML.
    if output.contains("<?xml") {
        // Strip the trailing status line if present
        if let Some(end) = output.rfind("</hierarchy>") {
            Ok(output[..end + "</hierarchy>".len()].to_string())
        } else {
            Ok(output)
        }
    } else {
        // Fallback: dump to file, cat it back, clean up
        let tmp = "/data/local/tmp/adbridge_hierarchy.xml";
        adb::shell_str(&format!(
            "uiautomator dump {tmp} >/dev/null 2>&1 && cat {tmp} && rm -f {tmp}"
        ))
        .context("Failed to dump view hierarchy via temp file")
    }
}

/// Compress a PNG screenshot to JPEG at reduced resolution.
///
/// Downscales the image to `max_width` (preserving aspect ratio) and encodes
/// it as JPEG at the given `quality` (0-100). Useful for reducing token usage
/// when sending screenshots to AI models.
///
/// # Examples
///
/// ```rust,no_run
/// # fn main() -> anyhow::Result<()> {
/// let png = adbridge::screen::capture_screenshot()?;
/// let jpeg = adbridge::screen::compress_screenshot(&png, 720, 80)?;
/// println!("PNG: {} bytes -> JPEG: {} bytes", png.len(), jpeg.len());
/// # Ok(())
/// # }
/// ```
pub fn compress_screenshot(png_data: &[u8], max_width: u32, quality: u8) -> Result<Vec<u8>> {
    let img = image::load_from_memory(png_data).context("Failed to decode screenshot")?;

    let img = if img.width() > max_width {
        let scale = max_width as f64 / img.width() as f64;
        let new_height = (img.height() as f64 * scale) as u32;
        img.resize(max_width, new_height, image::imageops::FilterType::Triangle)
    } else {
        img
    };

    let mut buf = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    img.write_with_encoder(encoder)
        .context("Failed to encode JPEG")?;
    Ok(buf)
}

/// Strip default/false attributes from uiautomator hierarchy XML to reduce size.
///
/// Removes attributes that carry no information: boolean attributes at their
/// default values (`checkable="false"`, `enabled="true"`, etc.), empty string
/// attributes (`text=""`, `content-desc=""`), and the `index` attribute.
/// Typically reduces XML size by 50%+.
///
/// Non-`<node>` elements (like `<hierarchy>`) keep all their attributes.
/// Returns the input unchanged if it cannot be parsed as XML.
///
/// # Examples
///
/// ```
/// use adbridge::screen::strip_hierarchy;
///
/// let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
/// <hierarchy rotation="0">
///   <node text="Login" class="android.widget.Button" clickable="true"
///         checkable="false" enabled="true" bounds="[200,700][880,800]" />
/// </hierarchy>"#;
///
/// let stripped = strip_hierarchy(xml);
///
/// // Default-value attributes are removed
/// assert!(!stripped.contains("checkable="));
/// assert!(!stripped.contains("enabled="));
///
/// // Non-default attributes are preserved
/// assert!(stripped.contains(r#"text="Login""#));
/// assert!(stripped.contains(r#"clickable="true""#));
/// assert!(stripped.contains(r#"bounds="[200,700][880,800]""#));
///
/// // Always smaller than the original
/// assert!(stripped.len() < xml.len());
/// ```
pub fn strip_hierarchy(xml: &str) -> String {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return xml.to_string(),
    };

    let mut out = String::with_capacity(xml.len() / 2);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    write_element(&doc.root_element(), &mut out, 0);
    out
}

fn write_element(node: &roxmltree::Node, out: &mut String, depth: usize) {
    let indent = "  ".repeat(depth);
    out.push_str(&indent);
    out.push('<');
    out.push_str(node.tag_name().name());

    for attr in node.attributes() {
        // Only strip attributes on <node> elements; keep all attrs on <hierarchy> etc.
        if node.tag_name().name() != "node" || should_keep_attr(attr.name(), attr.value()) {
            out.push(' ');
            out.push_str(attr.name());
            out.push_str("=\"");
            out.push_str(&escape_xml_attr(attr.value()));
            out.push('"');
        }
    }

    let children: Vec<_> = node.children().filter(|c| c.is_element()).collect();
    if children.is_empty() {
        out.push_str(" />\n");
    } else {
        out.push_str(">\n");
        for child in &children {
            write_element(child, out, depth + 1);
        }
        out.push_str(&indent);
        out.push_str("</");
        out.push_str(node.tag_name().name());
        out.push_str(">\n");
    }
}

/// Whether a uiautomator `<node>` attribute should be kept in stripped output.
fn should_keep_attr(name: &str, value: &str) -> bool {
    // Skip empty string attributes (text="", content-desc="", resource-id="", hint="")
    if value.is_empty() {
        return false;
    }

    // Boolean attributes that default to "false" -- skip when at default
    const FALSE_BY_DEFAULT: &[&str] = &[
        "checkable",
        "checked",
        "clickable",
        "focusable",
        "focused",
        "scrollable",
        "long-clickable",
        "password",
        "selected",
    ];
    if FALSE_BY_DEFAULT.contains(&name) && value == "false" {
        return false;
    }

    // enabled defaults to "true" -- skip when at default
    if name == "enabled" && value == "true" {
        return false;
    }

    // index is noise for structural understanding
    if name == "index" {
        return false;
    }

    true
}

fn escape_xml_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Clean OCR output by removing lines that are mostly non-alphanumeric noise.
///
/// Tesseract often produces garbage characters when run against non-text areas
/// of a screenshot (icons, gradients, wallpapers). This function filters out
/// lines where less than 40% of characters are alphanumeric or whitespace.
///
/// # Examples
///
/// ```
/// use adbridge::screen::clean_ocr_text;
///
/// let raw_ocr = "Settings\n!!@@##$$%%\nWi-Fi\n{{{|||}}}\nBluetooth";
/// let cleaned = clean_ocr_text(raw_ocr);
///
/// assert!(cleaned.contains("Settings"));
/// assert!(cleaned.contains("Wi-Fi"));
/// assert!(cleaned.contains("Bluetooth"));
/// assert!(!cleaned.contains("!!@@"));
/// assert!(!cleaned.contains("{{{"));
/// ```
pub fn clean_ocr_text(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return false;
            }
            let alnum = trimmed
                .chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace())
                .count();
            let total = trimmed.chars().count();
            // Keep lines where at least 40% of characters are alphanumeric or whitespace
            alnum * 100 / total.max(1) >= 40
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run OCR on a PNG image buffer using Tesseract.
///
/// Writes the image to a temp file, runs Tesseract, and returns the extracted
/// text. Requires Tesseract and tessdata to be installed on the system.
///
/// # Examples
///
/// ```rust,no_run
/// # fn main() -> anyhow::Result<()> {
/// let png = adbridge::screen::capture_screenshot()?;
/// let text = adbridge::screen::ocr_image(&png)?;
/// let clean = adbridge::screen::clean_ocr_text(&text);
/// println!("{clean}");
/// # Ok(())
/// # }
/// ```
pub fn ocr_image(png_data: &[u8]) -> Result<String> {
    use leptess::LepTess;
    use std::io::Write;

    let tmp_path = std::env::temp_dir().join(format!("adbridge_ocr_{}.png", std::process::id()));
    {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(png_data)?;
    }

    let mut lt = LepTess::new(None, "eng")
        .context("Failed to initialize Tesseract. Is tesseract-ocr and tessdata installed?")?;
    lt.set_image(tmp_path.to_str().context("Invalid temp path")?)
        .context("Failed to load image for OCR")?;

    let text = lt.get_utf8_text().context("OCR failed")?;
    std::fs::remove_file(&tmp_path).ok();

    Ok(text)
}

/// Full screen capture pipeline.
/// If `include_base64` is false, the screenshot is saved to a temp file instead.
pub fn capture(
    ocr: bool,
    hierarchy: bool,
    elems: bool,
    include_base64: bool,
) -> Result<ScreenCapture> {
    let png_data = capture_screenshot()?;

    let image_base64 = if include_base64 {
        Some(base64::engine::general_purpose::STANDARD.encode(&png_data))
    } else {
        None
    };

    let saved_to = if !include_base64 {
        let path = std::env::temp_dir()
            .join(format!(
                "adbridge_screenshot_{}.png",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ))
            .to_string_lossy()
            .to_string();
        std::fs::write(&path, &png_data)?;
        Some(path)
    } else {
        None
    };

    let ocr_text = if ocr {
        Some(ocr_image(&png_data)?)
    } else {
        None
    };

    // Fetch hierarchy once if either hierarchy or elements is requested
    let hierarchy_xml = if hierarchy || elems {
        Some(dump_hierarchy()?)
    } else {
        None
    };

    let elements_text = if elems {
        let xml = hierarchy_xml.as_deref().unwrap_or("");
        let parsed = elements::parse_elements(xml, true);
        Some(elements::format_elements(&parsed))
    } else {
        None
    };

    Ok(ScreenCapture {
        image_base64,
        ocr_text,
        hierarchy: if hierarchy { hierarchy_xml } else { None },
        elements: elements_text,
        saved_to,
    })
}

/// CLI entry point.
pub async fn run(args: ScreenArgs) -> Result<()> {
    let include_base64 = args.output.is_none() && args.json;
    let mut result = capture(args.ocr, args.hierarchy, args.elements, include_base64)?;

    if let Some(ref path) = args.output {
        // Re-read the already-saved temp file or capture fresh if base64 was used
        let png_data = if let Some(ref tmp) = result.saved_to {
            std::fs::read(tmp)?
        } else {
            capture_screenshot()?
        };
        std::fs::write(path, &png_data)?;
        result.saved_to = Some(path.clone());
        result.image_base64 = None;
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        if let Some(ref path) = result.saved_to {
            println!("Screenshot saved to {path}");
        }
        if let Some(ref text) = result.ocr_text {
            println!("--- OCR Text ---");
            println!("{text}");
        }
        if let Some(ref xml) = result.hierarchy {
            println!("--- View Hierarchy ---");
            println!("{xml}");
        }
        if let Some(ref elems) = result.elements {
            println!("--- UI Elements ---");
            println!("{elems}");
        }
        if result.saved_to.is_none()
            && result.ocr_text.is_none()
            && result.hierarchy.is_none()
            && result.elements.is_none()
        {
            println!(
                "Screenshot captured ({} bytes base64). Use --output to save, --ocr for text, --hierarchy for layout, --elements for interactive elements.",
                result.image_base64.as_ref().map(|s| s.len()).unwrap_or(0)
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HIERARCHY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<hierarchy rotation="0">
  <node index="0" text="" resource-id="" class="android.widget.FrameLayout"
        package="com.example" content-desc="" checkable="false" checked="false"
        clickable="false" enabled="true" focusable="false" focused="false"
        scrollable="false" long-clickable="false" password="false" selected="false"
        bounds="[0,0][1080,2340]">
    <node index="0" text="Login" resource-id="com.example:id/login_btn"
          class="android.widget.Button" package="com.example" content-desc="Log in"
          checkable="false" checked="false" clickable="true" enabled="true"
          focusable="true" focused="false" scrollable="false" long-clickable="false"
          password="false" selected="false" bounds="[200,700][880,800]" />
    <node index="1" text="" resource-id="com.example:id/password"
          class="android.widget.EditText" package="com.example" content-desc=""
          checkable="false" checked="false" clickable="true" enabled="false"
          focusable="true" focused="true" scrollable="false" long-clickable="false"
          password="true" selected="false" bounds="[100,550][980,650]" />
  </node>
</hierarchy>"#;

    #[test]
    fn strip_hierarchy_removes_default_attrs() {
        let stripped = strip_hierarchy(SAMPLE_HIERARCHY);

        // Default-false booleans should be gone
        assert!(!stripped.contains("checkable=\"false\""));
        assert!(!stripped.contains("checked=\"false\""));
        assert!(!stripped.contains("selected=\"false\""));
        assert!(!stripped.contains("scrollable=\"false\""));
        assert!(!stripped.contains("long-clickable=\"false\""));

        // enabled="true" (default) should be gone
        assert!(!stripped.contains("enabled=\"true\""));

        // Empty attributes should be gone
        assert!(!stripped.contains("content-desc=\"\""));
        assert!(!stripped.contains("text=\"\""));
        assert!(!stripped.contains("resource-id=\"\""));

        // index should be gone
        assert!(!stripped.contains("index="));
    }

    #[test]
    fn strip_hierarchy_keeps_non_default_attrs() {
        let stripped = strip_hierarchy(SAMPLE_HIERARCHY);

        // Non-default booleans should remain
        assert!(stripped.contains("clickable=\"true\""));
        assert!(stripped.contains("focusable=\"true\""));
        assert!(stripped.contains("focused=\"true\""));
        assert!(stripped.contains("password=\"true\""));
        assert!(stripped.contains("enabled=\"false\""));

        // Non-empty text attributes
        assert!(stripped.contains("text=\"Login\""));
        assert!(stripped.contains("content-desc=\"Log in\""));
        assert!(stripped.contains("resource-id=\"com.example:id/login_btn\""));
        assert!(stripped.contains("class=\"android.widget.Button\""));
        assert!(stripped.contains("bounds=\"[200,700][880,800]\""));

        // Hierarchy root attrs preserved
        assert!(stripped.contains("rotation=\"0\""));
    }

    #[test]
    fn strip_hierarchy_is_smaller() {
        let stripped = strip_hierarchy(SAMPLE_HIERARCHY);
        assert!(
            stripped.len() < SAMPLE_HIERARCHY.len(),
            "stripped ({}) should be smaller than original ({})",
            stripped.len(),
            SAMPLE_HIERARCHY.len()
        );
    }

    #[test]
    fn strip_hierarchy_preserves_structure() {
        let stripped = strip_hierarchy(SAMPLE_HIERARCHY);
        // Should still be parseable XML
        assert!(stripped.contains("<hierarchy"));
        assert!(stripped.contains("</hierarchy>"));
        assert!(stripped.contains("<node"));
        // Verify nesting: parent node should contain children
        assert!(stripped.contains("</node>"));
    }

    #[test]
    fn strip_hierarchy_invalid_xml_passthrough() {
        let bad = "not xml at all";
        assert_eq!(strip_hierarchy(bad), bad);
    }

    #[test]
    fn clean_ocr_removes_noise() {
        let input = "Hello World\n!@#$%^&*()\n<>{}[]|\\~`\nGood text here\n...---...\n";
        let cleaned = clean_ocr_text(input);
        assert!(cleaned.contains("Hello World"));
        assert!(cleaned.contains("Good text here"));
        assert!(!cleaned.contains("!@#$%^&*()"));
        assert!(!cleaned.contains("<>{}[]|\\~`"));
    }

    #[test]
    fn clean_ocr_keeps_normal_text() {
        let input = "Settings\nWi-Fi\nBluetooth\nVersion 1.2.3";
        let cleaned = clean_ocr_text(input);
        assert!(cleaned.contains("Settings"));
        assert!(cleaned.contains("Wi-Fi"));
        assert!(cleaned.contains("Version 1.2.3"));
    }

    #[test]
    fn clean_ocr_empty_returns_empty() {
        assert!(clean_ocr_text("").is_empty());
        assert!(clean_ocr_text("   \n  \n   ").is_empty());
    }

    #[test]
    fn clean_ocr_all_noise_returns_empty() {
        let noise = "~!@#\n$%^&\n***\n|||";
        assert!(clean_ocr_text(noise).is_empty());
    }
}
