use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "adbridge",
    about = "Android Bridge for AI-Assisted Development",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Capture screenshot with optional OCR and view hierarchy
    Screen(ScreenArgs),

    /// Stream or query logcat with filtering
    Log(LogArgs),

    /// Send input to device (keyboard, tap, swipe, clipboard)
    Input(InputArgs),

    /// Query current device/app state
    State(StateArgs),

    /// Get crash context (stacktrace + screenshot + recent actions)
    Crash(CrashArgs),

    /// List connected Android devices
    Devices(DevicesArgs),

    /// Start MCP server (stdio transport)
    Serve,
}

#[derive(clap::Args)]
pub struct DevicesArgs {
    /// Output as JSON
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct ScreenArgs {
    /// Run OCR on the screenshot
    #[arg(long, default_value_t = false)]
    pub ocr: bool,

    /// Include view hierarchy from uiautomator
    #[arg(long, default_value_t = false)]
    pub hierarchy: bool,

    /// List interactive UI elements with tap coordinates
    #[arg(long, default_value_t = false)]
    pub elements: bool,

    /// Save screenshot to file instead of stdout
    #[arg(short, long)]
    pub output: Option<String>,

    /// Output as JSON (for piping)
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct LogArgs {
    /// Filter by application package name
    #[arg(long)]
    pub app: Option<String>,

    /// Filter by log tag
    #[arg(long)]
    pub tag: Option<String>,

    /// Minimum log level (verbose, debug, info, warn, error, fatal)
    #[arg(long, default_value = "verbose")]
    pub level: String,

    /// Number of recent lines to show (0 = stream live)
    #[arg(short, long, default_value_t = 50)]
    pub lines: u32,

    /// Output as JSON
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct InputArgs {
    #[command(subcommand)]
    pub action: InputAction,
}

#[derive(Subcommand)]
pub enum InputAction {
    /// Type text on the device
    Text {
        /// The text to type
        value: String,
    },
    /// Tap at screen coordinates
    Tap {
        /// X coordinate
        x: u32,
        /// Y coordinate
        y: u32,
    },
    /// Swipe between coordinates
    Swipe {
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        /// Duration in milliseconds
        #[arg(short, long, default_value_t = 300)]
        duration: u32,
    },
    /// Send a key event (home, back, enter, etc.)
    Key {
        /// Key name (home, back, enter, menu, power, volup, voldown)
        name: String,
    },
    /// Push text to device clipboard
    Clip {
        /// Text to set on clipboard
        text: String,
    },
}

#[derive(clap::Args)]
pub struct StateArgs {
    /// Output as JSON
    #[arg(long, default_value_t = false)]
    pub json: bool,

    /// Include memory statistics
    #[arg(long, default_value_t = false)]
    pub memory: bool,
}

#[derive(clap::Args)]
pub struct CrashArgs {
    /// Path to source root for mapping stacktrace to code
    #[arg(long)]
    pub source: Option<String>,

    /// Output as JSON
    #[arg(long, default_value_t = false)]
    pub json: bool,
}
