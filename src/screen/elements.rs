use std::fmt;

/// A parsed UI element from the uiautomator view hierarchy.
///
/// Each element represents a node from the Android view tree with its
/// properties, bounding box, and computed center coordinates (for tapping).
///
/// Use [`parse_elements`] to extract elements from hierarchy XML, and
/// the [`Display`](std::fmt::Display) impl to render them as compact text.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UiElement {
    pub index: u32,
    pub class: String,
    pub text: String,
    pub content_desc: String,
    pub resource_id: String,
    pub clickable: bool,
    pub focusable: bool,
    pub scrollable: bool,
    pub checkable: bool,
    pub enabled: bool,
    pub bounds: ((u32, u32), (u32, u32)),
    pub center: (u32, u32),
}

impl UiElement {
    /// Whether this element is considered interactive (tappable/focusable/scrollable).
    pub fn is_interactive(&self) -> bool {
        self.clickable || self.focusable || self.scrollable || self.checkable
    }
}

impl fmt::Display for UiElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] ", self.index)?;

        if self.is_interactive() {
            write!(f, "{}", self.class)?;
        } else {
            // Non-interactive landmark -- bracket the type
            write!(f, "[{}]", self.class)?;
        }

        // Primary label: text, falling back to content_desc
        if !self.text.is_empty() {
            write!(f, " \"{}\"", self.text)?;
            // Show content_desc only if different from text
            if !self.content_desc.is_empty() && self.content_desc != self.text {
                write!(f, " desc:\"{}\"", self.content_desc)?;
            }
        } else if !self.content_desc.is_empty() {
            write!(f, " desc:\"{}\"", self.content_desc)?;
        }

        // Show resource ID only when there's no text label
        if self.text.is_empty() && self.content_desc.is_empty() && !self.resource_id.is_empty() {
            write!(f, " id:{}", self.resource_id)?;
        }

        // Coordinates
        write!(f, " ({}, {})", self.center.0, self.center.1)?;

        // Annotate non-obvious properties (clickable is implied by being in the list)
        let mut props = Vec::new();
        if self.focusable && !self.clickable {
            props.push("focusable");
        }
        if self.scrollable {
            props.push("scrollable");
        }
        if self.checkable {
            props.push("checkable");
        }
        if !self.enabled {
            props.push("disabled");
        }
        if !props.is_empty() {
            write!(f, " {}", props.join(" "))?;
        }

        Ok(())
    }
}

/// Shorten an Android class name: "android.widget.Button" -> "Button".
fn short_class(class: &str) -> String {
    class.rsplit('.').next().unwrap_or(class).to_string()
}

/// Parse bounds="[x1,y1][x2,y2]" into ((x1,y1),(x2,y2)).
fn parse_bounds(bounds: &str) -> Option<((u32, u32), (u32, u32))> {
    // Format: "[x1,y1][x2,y2]"
    let stripped = bounds.replace('[', "").replace(']', ",");
    let nums: Vec<u32> = stripped
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() == 4 {
        Some(((nums[0], nums[1]), (nums[2], nums[3])))
    } else {
        None
    }
}

/// Strip the package prefix from a resource ID: "com.app:id/login_btn" -> "login_btn".
fn short_resource_id(id: &str) -> String {
    if let Some(pos) = id.rfind('/') {
        id[pos + 1..].to_string()
    } else {
        id.to_string()
    }
}

/// Parse uiautomator XML into a flat list of UI elements.
///
/// If `interactive_only` is true, only elements that are clickable, focusable,
/// scrollable, or checkable are included, plus non-interactive elements that
/// have visible text or content descriptions (as landmarks for orientation).
///
/// Each element gets a sequential index (starting at 1) and pre-computed
/// center coordinates for easy tapping.
///
/// # Examples
///
/// ```
/// use adbridge::screen::elements::parse_elements;
///
/// let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
/// <hierarchy rotation="0">
///   <node text="Login" resource-id="com.app:id/btn" class="android.widget.Button"
///         clickable="true" focusable="true" bounds="[200,700][880,800]" />
///   <node text="" resource-id="com.app:id/spacer" class="android.view.View"
///         clickable="false" focusable="false" bounds="[0,800][1080,810]" />
/// </hierarchy>"#;
///
/// // interactive_only filters out non-interactive, unlabeled elements
/// let elements = parse_elements(xml, true);
/// assert_eq!(elements.len(), 1);
/// assert_eq!(elements[0].text, "Login");
/// assert_eq!(elements[0].center, (540, 750));
/// assert!(elements[0].clickable);
///
/// // All mode includes every element with valid bounds
/// let all = parse_elements(xml, false);
/// assert_eq!(all.len(), 2);
/// ```
pub fn parse_elements(xml: &str, interactive_only: bool) -> Vec<UiElement> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let mut elements = Vec::new();
    let mut index = 0u32;

    for node in doc.descendants() {
        if !node.is_element() || node.tag_name().name() != "node" {
            continue;
        }

        let text = node.attribute("text").unwrap_or("").to_string();
        let content_desc = node.attribute("content-desc").unwrap_or("").to_string();
        let resource_id = short_resource_id(node.attribute("resource-id").unwrap_or(""));
        let class = short_class(node.attribute("class").unwrap_or("View"));
        let clickable = node.attribute("clickable") == Some("true");
        let focusable = node.attribute("focusable") == Some("true");
        let scrollable = node.attribute("scrollable") == Some("true");
        let checkable = node.attribute("checkable") == Some("true");
        let enabled = node.attribute("enabled") != Some("false");

        let bounds_str = node.attribute("bounds").unwrap_or("");
        let bounds = match parse_bounds(bounds_str) {
            Some(b) => b,
            None => continue,
        };

        let center = (
            (bounds.0 .0 + bounds.1 .0) / 2,
            (bounds.0 .1 + bounds.1 .1) / 2,
        );

        let is_interactive = clickable || focusable || scrollable || checkable;
        let has_label = !text.is_empty() || !content_desc.is_empty();

        if interactive_only && !is_interactive && !has_label {
            continue;
        }

        index += 1;
        elements.push(UiElement {
            index,
            class,
            text,
            content_desc,
            resource_id,
            clickable,
            focusable,
            scrollable,
            checkable,
            enabled,
            bounds,
            center,
        });
    }

    elements
}

/// Format a list of elements as a compact, human-readable text listing.
///
/// Each element is printed on one line with its index, class, label, center
/// coordinates, and any notable properties. Interactive elements show their
/// class name directly; non-interactive landmarks have their class in brackets.
///
/// # Examples
///
/// ```
/// use adbridge::screen::elements::{parse_elements, format_elements};
///
/// let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
/// <hierarchy rotation="0">
///   <node text="Login" resource-id="com.app:id/btn" class="android.widget.Button"
///         clickable="true" focusable="true" bounds="[200,700][880,800]" />
///   <node text="Welcome" class="android.widget.TextView"
///         clickable="false" focusable="false" bounds="[100,400][980,500]" />
/// </hierarchy>"#;
///
/// let elements = parse_elements(xml, true);
/// let output = format_elements(&elements);
///
/// // Interactive element shown directly
/// assert!(output.contains(r#"Button "Login" (540, 750)"#));
/// // Non-interactive landmark has class in brackets
/// assert!(output.contains(r#"[TextView] "Welcome""#));
/// ```
pub fn format_elements(elements: &[UiElement]) -> String {
    elements
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<hierarchy rotation="0">
  <node index="0" text="" resource-id="" class="android.widget.FrameLayout"
        package="com.example" content-desc="" checkable="false" checked="false"
        clickable="false" enabled="true" focusable="false" focused="false"
        scrollable="false" long-clickable="false" password="false" selected="false"
        bounds="[0,0][1080,2340]">
    <node index="0" text="Username" resource-id="com.example:id/username"
          class="android.widget.EditText" package="com.example" content-desc=""
          checkable="false" checked="false" clickable="true" enabled="true"
          focusable="true" focused="false" scrollable="false" long-clickable="false"
          password="false" selected="false" bounds="[100,400][980,500]" />
    <node index="1" text="Password" resource-id="com.example:id/password"
          class="android.widget.EditText" package="com.example" content-desc=""
          checkable="false" checked="false" clickable="true" enabled="true"
          focusable="true" focused="false" scrollable="false" long-clickable="false"
          password="true" selected="false" bounds="[100,550][980,650]" />
    <node index="2" text="Login" resource-id="com.example:id/login_btn"
          class="android.widget.Button" package="com.example" content-desc=""
          checkable="false" checked="false" clickable="true" enabled="true"
          focusable="true" focused="false" scrollable="false" long-clickable="false"
          password="false" selected="false" bounds="[200,700][880,800]" />
    <node index="3" text="Forgot password?" resource-id=""
          class="android.widget.TextView" package="com.example" content-desc=""
          checkable="false" checked="false" clickable="true" enabled="true"
          focusable="false" focused="false" scrollable="false" long-clickable="false"
          password="false" selected="false" bounds="[300,850][780,920]" />
    <node index="4" text="" resource-id="com.example:id/logo"
          class="android.widget.ImageView" package="com.example"
          content-desc="Company logo"
          checkable="false" checked="false" clickable="false" enabled="true"
          focusable="false" focused="false" scrollable="false" long-clickable="false"
          password="false" selected="false" bounds="[340,100][740,300]" />
    <node index="5" text="" resource-id="com.example:id/spacer"
          class="android.view.View" package="com.example" content-desc=""
          checkable="false" checked="false" clickable="false" enabled="true"
          focusable="false" focused="false" scrollable="false" long-clickable="false"
          password="false" selected="false" bounds="[0,920][1080,940]" />
    <node index="6" text="" resource-id="com.example:id/scroll_container"
          class="android.widget.ScrollView" package="com.example" content-desc=""
          checkable="false" checked="false" clickable="false" enabled="true"
          focusable="false" focused="false" scrollable="true" long-clickable="false"
          password="false" selected="false" bounds="[0,940][1080,2340]" />
    <node index="7" text="Remember me" resource-id="com.example:id/remember"
          class="android.widget.CheckBox" package="com.example" content-desc=""
          checkable="true" checked="false" clickable="true" enabled="false"
          focusable="true" focused="false" scrollable="false" long-clickable="false"
          password="false" selected="false" bounds="[100,1000][500,1080]" />
  </node>
</hierarchy>"#;

    #[test]
    fn parse_bounds_valid() {
        assert_eq!(
            parse_bounds("[100,200][300,400]"),
            Some(((100, 200), (300, 400)))
        );
    }

    #[test]
    fn parse_bounds_invalid() {
        assert_eq!(parse_bounds("bad"), None);
        assert_eq!(parse_bounds("[100,200]"), None);
        assert_eq!(parse_bounds(""), None);
    }

    #[test]
    fn short_class_strips_package() {
        assert_eq!(short_class("android.widget.Button"), "Button");
        assert_eq!(short_class("Button"), "Button");
        assert_eq!(short_class("com.custom.views.MyButton"), "MyButton");
    }

    #[test]
    fn short_resource_id_strips_prefix() {
        assert_eq!(short_resource_id("com.example:id/login_btn"), "login_btn");
        assert_eq!(short_resource_id("login_btn"), "login_btn");
        assert_eq!(short_resource_id(""), "");
    }

    #[test]
    fn parse_elements_interactive_only() {
        let elements = parse_elements(SAMPLE_XML, true);

        // Should include: Username, Password, Login, "Forgot password?",
        // "Company logo" (has content_desc), ScrollView (scrollable),
        // "Remember me" (checkable)
        // Should exclude: root FrameLayout (no label, not interactive),
        // spacer View (no label, not interactive)
        assert_eq!(elements.len(), 7);

        // First element: Username EditText
        assert_eq!(elements[0].text, "Username");
        assert_eq!(elements[0].class, "EditText");
        assert!(elements[0].clickable);
        assert!(elements[0].focusable);
        assert_eq!(elements[0].center, (540, 450));

        // Login button
        let login = elements.iter().find(|e| e.text == "Login").unwrap();
        assert_eq!(login.class, "Button");
        assert_eq!(login.center, (540, 750));

        // Logo image (non-interactive landmark with content_desc)
        let logo = elements
            .iter()
            .find(|e| e.content_desc == "Company logo")
            .unwrap();
        assert_eq!(logo.class, "ImageView");
        assert!(!logo.is_interactive());

        // ScrollView (interactive via scrollable, no label)
        let scroll = elements.iter().find(|e| e.class == "ScrollView").unwrap();
        assert!(scroll.scrollable);
        assert!(scroll.is_interactive());

        // Disabled checkbox
        let checkbox = elements.iter().find(|e| e.text == "Remember me").unwrap();
        assert!(checkbox.checkable);
        assert!(!checkbox.enabled);
    }

    #[test]
    fn parse_elements_all() {
        let all = parse_elements(SAMPLE_XML, false);
        let interactive = parse_elements(SAMPLE_XML, true);
        // All mode should include more elements (root FrameLayout, spacer)
        assert!(all.len() > interactive.len());
    }

    #[test]
    fn format_compact_output() {
        let elements = parse_elements(SAMPLE_XML, true);
        let output = format_elements(&elements);

        // Check key formatting rules
        assert!(output.contains("[1] EditText \"Username\" (540, 450)"));
        assert!(output.contains("[3] Button \"Login\" (540, 750)"));

        // Forgot password is clickable TextView -- should show as interactive
        assert!(output.contains("TextView \"Forgot password?\""));

        // Logo: non-interactive, so class is bracketed
        assert!(output.contains("[ImageView] desc:\"Company logo\""));

        // ScrollView: no text, shows resource id
        assert!(output.contains("ScrollView id:scroll_container"));
        assert!(output.contains("scrollable"));

        // Disabled checkbox
        assert!(output.contains("CheckBox \"Remember me\""));
        assert!(output.contains("disabled"));
        assert!(output.contains("checkable"));
    }

    #[test]
    fn center_calculation() {
        let elements = parse_elements(SAMPLE_XML, true);
        let login = elements.iter().find(|e| e.text == "Login").unwrap();
        // bounds [200,700][880,800] -> center (540, 750)
        assert_eq!(login.center, (540, 750));
        assert_eq!(login.bounds, ((200, 700), (880, 800)));
    }

    #[test]
    fn empty_xml_returns_empty() {
        assert!(parse_elements("", true).is_empty());
        assert!(parse_elements("not xml at all", true).is_empty());
    }

    #[test]
    fn indices_are_sequential() {
        let elements = parse_elements(SAMPLE_XML, true);
        for (i, el) in elements.iter().enumerate() {
            assert_eq!(el.index, (i + 1) as u32);
        }
    }
}
