pub mod connection;

use adb_client::server::ADBServer;
use adb_client::ADBDeviceExt;
use anyhow::{Context, Result};
use std::sync::Mutex;

/// Global target device serial. When set, all ADB commands target this device.
/// When `None`, the first connected device is used (original behavior).
static TARGET_DEVICE: Mutex<Option<String>> = Mutex::new(None);

/// Set the target device serial for all subsequent ADB commands.
///
/// Pass `None` to revert to first-connected-device behavior. When multiple
/// devices are connected, use the serial from [`connection::list_devices`] to
/// target a specific one.
///
/// # Examples
///
/// ```rust,no_run
/// // Target a specific device
/// adbridge::adb::set_target_device(Some("emulator-5554".into()));
///
/// // Revert to first-connected-device behavior
/// adbridge::adb::set_target_device(None);
/// ```
pub fn set_target_device(device: Option<String>) {
    *TARGET_DEVICE.lock().unwrap_or_else(|e| e.into_inner()) = device;
}

/// Get a connected ADB server instance (connects to local adb server on default port).
pub fn server() -> Result<ADBServer> {
    let server = ADBServer::default();
    Ok(server)
}

/// Get the target device handle, respecting the global device selection.
fn get_target_device(server: &mut ADBServer) -> Result<adb_client::server_device::ADBServerDevice> {
    let serial = TARGET_DEVICE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    match serial {
        Some(ref s) => server.get_device_by_name(s).with_context(|| {
            format!("Device '{s}' not found. Check serial with `adbridge devices`.")
        }),
        None => server
            .get_device()
            .context("No device connected. Is a device/emulator attached via ADB?"),
    }
}

/// Execute a shell command on the target device and return stdout as bytes.
///
/// This is the low-level interface for running arbitrary commands on the device.
/// For string output, use [`shell_str`] instead.
///
/// # Examples
///
/// ```rust,no_run
/// # fn main() -> anyhow::Result<()> {
/// let output = adbridge::adb::shell("screencap -p")?;
/// std::fs::write("screenshot.png", &output)?;
/// # Ok(())
/// # }
/// ```
pub fn shell(command: &str) -> Result<Vec<u8>> {
    let mut server = server()?;
    let mut device = get_target_device(&mut server)?;

    let mut output = Vec::new();
    device
        .shell_command(&command, Some(&mut output), None)
        .context("Failed to execute shell command on device")?;

    Ok(output)
}

/// Execute a shell command and return output as a UTF-8 string.
///
/// Lossy conversion is used, so invalid UTF-8 bytes are replaced with the
/// Unicode replacement character.
///
/// # Examples
///
/// ```rust,no_run
/// # fn main() -> anyhow::Result<()> {
/// let model = adbridge::adb::shell_str("getprop ro.product.model")?;
/// println!("Device: {}", model.trim());
/// # Ok(())
/// # }
/// ```
pub fn shell_str(command: &str) -> Result<String> {
    let output = shell(command)?;
    Ok(String::from_utf8_lossy(&output).to_string())
}

/// Execute a shell command on a specific device by serial and return stdout as bytes.
pub fn shell_on(serial: &str, command: &str) -> Result<Vec<u8>> {
    let mut server = server()?;
    let mut device = server.get_device_by_name(serial).with_context(|| {
        format!("Device '{serial}' not found. Check serial with `adbridge devices`.")
    })?;

    let mut output = Vec::new();
    device
        .shell_command(&command, Some(&mut output), None)
        .context("Failed to execute shell command on device")?;

    Ok(output)
}

/// Execute a shell command on a specific device by serial and return output as a UTF-8 string.
pub fn shell_str_on(serial: &str, command: &str) -> Result<String> {
    let output = shell_on(serial, command)?;
    Ok(String::from_utf8_lossy(&output).to_string())
}
