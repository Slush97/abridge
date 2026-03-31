# abridge

Android Bridge for AI-Assisted Development. A CLI tool and MCP server that gives AI coding assistants direct access to your Android device: screenshots, OCR, logcat, input control, and device state inspection.

No more manually screenshotting, copy-pasting logs, or describing what's on screen. Just ask your AI assistant to look at the device.

## What it does

abridge runs as a **standalone CLI** or as an **MCP server** that exposes your connected Android device as structured, queryable tools. Any MCP-compatible AI tool (Claude Code, Cursor, Cline, etc.) can then:

- Capture screenshots and extract text via OCR
- Read filtered logcat entries
- Send taps, swipes, keystrokes, and text to the device
- Inspect the current activity, fragment backstack, and memory stats
- List connected devices with model and version info
- Pull crash reports with full context

```
                          ADB
 ┌──────────────┐   native protocol   ┌──────────────┐
 │   Android    │◄────────────────────►│   abridge    │
 │   Device     │                      │   daemon     │
 └──────────────┘                      └──────┬───────┘
                                              │
                                       ┌──────┴───────┐
                                       │              │
                                     CLI          MCP Server
                                  (human)        (AI tools)
```

## Install

### Prerequisites

- **Rust toolchain** (1.75+): https://rustup.rs
- **ADB server** running (`adb start-server`)
- **Tesseract OCR** (for `--ocr` feature):
  ```sh
  # Arch
  sudo pacman -S tesseract tesseract-data-eng

  # Ubuntu/Debian
  sudo apt install tesseract-ocr libtesseract-dev libleptonica-dev

  # macOS
  brew install tesseract
  ```

### From source

```sh
git clone https://github.com/Slush97/abridge.git
cd abridge
cargo install --path .
```

### Verify

```sh
abridge --version
abridge --help
```

## CLI Usage

### Screenshot + OCR

```sh
# Capture screenshot, save to file
abridge screen --output screenshot.png

# Capture with OCR text extraction
abridge screen --ocr

# Full context: screenshot + OCR + view hierarchy as JSON
abridge screen --ocr --hierarchy --json
```

### Logcat

```sh
# Recent errors
abridge log --level error --lines 20

# Filter by app
abridge log --app com.myapp --level warn

# Filter by tag, JSON output
abridge log --tag NetworkManager --json
```

### Input

```sh
# Type text on device
abridge input text "hello world"

# Tap coordinates
abridge input tap 540 1200

# Swipe (scroll down)
abridge input swipe 540 1500 540 500 --duration 300

# Hardware keys
abridge input key home
abridge input key back

# Set clipboard
abridge input clip "copied text"
```

### Device State

```sh
# Current activity, fragments, display info
abridge state

# Include memory stats, as JSON
abridge state --memory --json
```

### Devices

```sh
# List connected devices
abridge devices

# As JSON
abridge devices --json
```

### Crash Report

```sh
# Full crash context: stacktrace + recent errors + screenshot
abridge crash --json

# Pipe into an AI for analysis
abridge crash --json | claude "what caused this crash?"
```

## MCP Server

abridge exposes 6 tools over MCP's stdio transport:

| Tool | Description |
|------|-------------|
| `device_screenshot` | Capture screenshot with optional OCR and view hierarchy |
| `device_logcat` | Filtered logcat entries by app, tag, and level |
| `device_state` | Current activity, fragment backstack, display, memory |
| `device_input` | Send text, taps, swipes, keys, or clipboard to device |
| `device_info` | List connected devices with model and version info |
| `device_crash_report` | Stacktrace + screenshot + recent errors |

### Claude Code

Add to `~/.mcp.json`:

```json
{
  "mcpServers": {
    "abridge": {
      "command": "abridge",
      "args": ["serve"]
    }
  }
}
```

Restart Claude Code. You can now say things like:

> "What's on the phone screen right now?"
>
> "Check the logcat for errors in my app"
>
> "Tap the login button and tell me what happens"
>
> "The app just crashed, what went wrong?"

### Other MCP clients

Any client supporting MCP stdio transport can use abridge. The server starts with:

```sh
abridge serve
```

## How it works

- **ADB communication** via [`adb_client`](https://crates.io/crates/adb_client), native Rust ADB protocol with no `adb` binary dependency (ADB server still required)
- **OCR** via [`leptess`](https://crates.io/crates/leptess), FFI bindings to Tesseract/Leptonica
- **MCP server** via [`rmcp`](https://crates.io/crates/rmcp), the official Rust MCP SDK
- **CLI** via [`clap`](https://crates.io/crates/clap)

All device commands go through `adb shell` under the hood. The tool structures the raw output into JSON that AI assistants can reason about.

## Project Structure

```
src/
├── main.rs            Entry point, CLI dispatch
├── cli.rs             Clap argument definitions
├── adb/
│   ├── mod.rs         Core ADB shell commands
│   └── connection.rs  Device discovery and info
├── screen/mod.rs      Screenshot, OCR, view hierarchy
├── logcat/mod.rs      Log parsing and filtering
├── input/mod.rs       Text, tap, swipe, key, clipboard
├── state/mod.rs       Activity state, memory, crash reports
└── mcp/mod.rs         MCP server with 6 tools (stdio)
```

## License

MIT
