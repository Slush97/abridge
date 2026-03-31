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

/// Take a screenshot from the device.
pub fn capture_screenshot() -> Result<Vec<u8>> {
    adb::shell("screencap -p").context("Failed to capture screenshot")
}

/// Dump the view hierarchy via uiautomator.
///
/// Tries `/dev/tty` first (zero-copy, works on most devices). Falls back to
/// dumping to a temp file and reading it back (required on Android 16+).
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

/// Run OCR on a PNG image buffer using leptess.
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
