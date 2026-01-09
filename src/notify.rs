#[cfg(target_os = "macos")]
use anyhow::Context;
use anyhow::Result;
#[cfg(target_os = "macos")]
use mac_notification_sys::{send_notification as send_mac_notification, set_application};

#[cfg(target_os = "macos")]
pub fn send_notification(title: &str, message: &str) -> Result<()> {
    let bundle = mac_notification_sys::get_bundle_identifier_or_default("com.apple.Terminal");
    set_application(&bundle).with_context(|| "Failed to set application bundle")?;

    send_mac_notification(title, None, message, None)
        .with_context(|| format!("Failed to send notification: {title} - {message}"))?;

    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn send_notification(_title: &str, _message: &str) -> Result<()> {
    // Notifications are only supported on macOS
    Ok(())
}
