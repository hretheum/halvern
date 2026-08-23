//! Checking for, and installing, a newer Halvern.
//!
//! ## Why this exists at all
//!
//! Until this module, a person who downloaded 0.4.0 had no way to receive
//! 0.4.1. Every fix reached new downloads only, while the people who arrived
//! first kept the broken build — which is the wrong way round, since they are
//! the ones who took the risk.
//!
//! ## What it sends, exactly
//!
//! A check is one HTTP GET for a static file on GitHub Releases:
//! `releases/latest/download/latest.json`. It is the same request a browser
//! makes when someone clicks a download link. It carries no identifier, no
//! meeting data, no usage figures, and no body — there is nothing in it that
//! distinguishes one Halvern from another beyond what any file download
//! reveals: an IP address, a User-Agent, and a time.
//!
//! That is a real disclosure and it is written down in PRIVACY_POLICY.md
//! rather than buried here. It is also why the check does not run on a timer:
//! a periodic call would turn "does this person have Halvern open" into a
//! signal with a heartbeat, and when someone has meetings is exactly the thing
//! this product exists to keep to itself. Once per launch, and only when the
//! user has left the setting on.
//!
//! ## Why the signature matters more than the transport
//!
//! HTTPS proves the bytes came from GitHub. It does not prove GitHub, or
//! anyone who reaches the release assets, handed over a Halvern we built. The
//! updater verifies an Ed25519 signature against the public key compiled into
//! `tauri.conf.json`, and refuses anything that fails — so replacing the
//! download requires the private key, which lives in one GitHub secret and on
//! one machine.

use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::UpdaterExt;

/// What the interface needs to decide whether to offer anything.
#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    /// `None` when nothing newer exists.
    pub new_version: Option<String>,
    /// Release notes, when the release carried any.
    pub notes: Option<String>,
}

/// Ask GitHub whether a newer release exists.
///
/// Returns `available: false` rather than an error when the answer is "no",
/// so the caller does not have to tell a quiet success from a failed check.
/// A failed check *is* an error: it should be visible when the user asked, and
/// ignored when the launch check asked on their behalf.
#[tauri::command]
pub async fn check_for_update<R: Runtime>(app: AppHandle<R>) -> Result<UpdateInfo, String> {
    let current_version = app.package_info().version.to_string();

    let updater = app
        .updater()
        .map_err(|e| format!("Could not build the updater: {}", e))?;

    match updater.check().await {
        Ok(Some(update)) => {
            log::info!(
                "Update available: {} -> {}",
                current_version,
                update.version
            );
            Ok(UpdateInfo {
                available: true,
                current_version,
                new_version: Some(update.version.clone()),
                notes: update.body.clone(),
            })
        }
        Ok(None) => {
            log::info!("No update available; running {}", current_version);
            Ok(UpdateInfo {
                available: false,
                current_version,
                new_version: None,
                notes: None,
            })
        }
        Err(e) => Err(format!("Update check failed: {}", e)),
    }
}

/// Download the update, verify its signature, and install it.
///
/// Returns once the bytes are in place. The caller restarts the app — this
/// function deliberately does not, because deciding when to close a window is
/// the interface's business and doing it here could interrupt a recording.
#[tauri::command]
pub async fn install_update<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let updater = app
        .updater()
        .map_err(|e| format!("Could not build the updater: {}", e))?;

    let update = updater
        .check()
        .await
        .map_err(|e| format!("Update check failed: {}", e))?
        .ok_or_else(|| "There is no update to install.".to_string())?;

    log::info!("Installing update {}", update.version);

    // Signature verification happens inside `download_and_install` against the
    // public key in tauri.conf.json. A tampered or unsigned artifact fails
    // here rather than being written anywhere.
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| format!("Update installation failed: {}", e))?;

    log::info!("Update {} installed; restart to run it", update.version);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape the interface reads is part of the contract with it.
    #[test]
    fn update_info_serialises_with_the_fields_the_interface_expects() {
        let json = serde_json::to_value(UpdateInfo {
            available: true,
            current_version: "0.4.0".into(),
            new_version: Some("0.4.1".into()),
            notes: Some("Fixes".into()),
        })
        .expect("UpdateInfo should serialise");

        assert_eq!(json["available"], true);
        assert_eq!(json["current_version"], "0.4.0");
        assert_eq!(json["new_version"], "0.4.1");
        assert_eq!(json["notes"], "Fixes");
    }

    /// "Nothing newer" is a successful answer, not an absent one, so the
    /// interface can tell it apart from a check that failed.
    #[test]
    fn no_update_available_still_reports_the_running_version() {
        let json = serde_json::to_value(UpdateInfo {
            available: false,
            current_version: "0.4.0".into(),
            new_version: None,
            notes: None,
        })
        .expect("UpdateInfo should serialise");

        assert_eq!(json["available"], false);
        assert_eq!(json["current_version"], "0.4.0");
        assert!(json["new_version"].is_null());
    }
}
