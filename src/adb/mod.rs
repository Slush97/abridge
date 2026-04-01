pub mod connection;

use adb_client::server::ADBServer;
use adb_client::ADBDeviceExt;
use anyhow::{Context, Result};
use std::sync::Mutex;

/// Global target device serial. When set, all ADB commands target this device.
/// When None, the first connected device is used (original behavior).
static TARGET_DEVICE: Mutex<Option<String>> = Mutex::new(None);

/// Set the target device serial for all subsequent ADB commands.
/// Pass None to revert to first-connected-device behavior.
pub fn set_target_device(device: Option<String>) {
    *TARGET_DEVICE.lock().unwrap() = device;
}

/// Get a connected ADB server instance (connects to local adb server on default port).
pub fn server() -> Result<ADBServer> {
    let server = ADBServer::default();
    Ok(server)
}

/// Get the target device handle, respecting the global device selection.
fn get_target_device(server: &mut ADBServer) -> Result<adb_client::server_device::ADBServerDevice> {
    let serial = TARGET_DEVICE.lock().unwrap().clone();
    match serial {
        Some(ref s) => server.get_device_by_name(s).with_context(|| {
            format!("Device '{s}' not found. Check serial with `adbridge devices`.")
        }),
        None => server
            .get_device()
            .context("No device connected. Is a device/emulator attached via ADB?"),
    }
}

/// Execute a shell command on the target device and return stdout.
pub fn shell(command: &str) -> Result<Vec<u8>> {
    let mut server = server()?;
    let mut device = get_target_device(&mut server)?;

    let mut output = Vec::new();
    device
        .shell_command(&command, Some(&mut output), None)
        .context("Failed to execute shell command on device")?;

    Ok(output)
}

/// Execute a shell command and return output as a String.
pub fn shell_str(command: &str) -> Result<String> {
    let output = shell(command)?;
    Ok(String::from_utf8_lossy(&output).to_string())
}
