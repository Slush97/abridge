use adb_client::server::ADBServer;
use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::DevicesArgs;

#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub serial: String,
    pub model: String,
    pub android_version: String,
    pub sdk_version: String,
}

/// List connected devices with model, Android version, and SDK version.
///
/// # Examples
///
/// ```rust,no_run
/// # fn main() -> anyhow::Result<()> {
/// let devices = adbridge::adb::connection::list_devices()?;
/// for d in &devices {
///     println!("{} - {} (Android {})", d.serial, d.model, d.android_version);
/// }
/// # Ok(())
/// # }
/// ```
pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    let mut server = ADBServer::default();
    let devices = server
        .devices()
        .context("Failed to query ADB for devices")?;

    let mut infos = Vec::new();
    for device in devices {
        let serial = device.identifier.to_string();
        infos.push(DeviceInfo {
            serial,
            model: String::new(),
            android_version: String::new(),
            sdk_version: String::new(),
        });
    }

    // Enrich with props from each device (queried by serial)
    for info in &mut infos {
        if let Ok(model) = super::shell_str_on(&info.serial, "getprop ro.product.model") {
            info.model = model.trim().to_string();
        }
        if let Ok(ver) = super::shell_str_on(&info.serial, "getprop ro.build.version.release") {
            info.android_version = ver.trim().to_string();
        }
        if let Ok(sdk) = super::shell_str_on(&info.serial, "getprop ro.build.version.sdk") {
            info.sdk_version = sdk.trim().to_string();
        }
    }

    Ok(infos)
}

/// CLI entry point for `devices` command.
pub async fn run(args: DevicesArgs) -> Result<()> {
    let devices = list_devices()?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&devices)?);
    } else if devices.is_empty() {
        println!("No devices connected");
    } else {
        for d in &devices {
            println!(
                "{} - {} (Android {} / SDK {})",
                d.serial, d.model, d.android_version, d.sdk_version
            );
        }
    }

    Ok(())
}
