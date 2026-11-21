// SPDX-License-Identifier: AGPL-3.0-only
//! Settings (plain JSON) and the refresh token (OS keychain).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const SERVICE: &str = "OpnRelay Recorder";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    pub server_url: Option<String>,
    pub last_workspace: Option<String>,
    pub capture_mode: Option<String>,
}

fn dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("dev", "OpnRelay", "OpnRelay")
        .map(|d| d.config_dir().to_path_buf())
}

impl Settings {
    pub fn load() -> Settings {
        let mut s: Settings = dir()
            .and_then(|d| std::fs::read(d.join("settings.json")).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // IT can pre-configure the server (MSI property, MDM profile, env).
        if let Ok(url) = std::env::var("OPNRELAY_SERVER_URL")
            && let Some(url) = opnrelay_client::normalize_server(&url)
        {
            s.server_url = Some(url);
        }
        s
    }

    pub fn save(&self) {
        if let Some(d) = dir() {
            let _ = std::fs::create_dir_all(&d);
            if let Ok(json) = serde_json::to_vec_pretty(self) {
                let _ = std::fs::write(d.join("settings.json"), json);
            }
        }
    }
}

/// Where the refresh token is kept, for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStore {
    Keychain,
    /// Fallback when no OS keychain is available (e.g. a Linux session
    /// without a Secret Service). File is readable only by the user.
    File,
}

fn token_file() -> Option<PathBuf> {
    dir().map(|d| d.join("token"))
}

pub fn save_refresh(server: &str, token: &str) -> TokenStore {
    if keyring::Entry::new(SERVICE, server)
        .and_then(|e| e.set_password(token))
        .is_ok()
    {
        if let Some(f) = token_file() {
            let _ = std::fs::remove_file(f);
        }
        return TokenStore::Keychain;
    }
    if let Some(f) = token_file() {
        let _ = std::fs::create_dir_all(f.parent().expect("has parent"));
        let _ = std::fs::write(&f, token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
        }
    }
    TokenStore::File
}

pub fn load_refresh(server: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, server)
        .and_then(|e| e.get_password())
        .ok()
        .or_else(|| token_file().and_then(|f| std::fs::read_to_string(f).ok()))
        .filter(|t| !t.trim().is_empty())
}

pub fn clear_refresh(server: &str) {
    if let Ok(e) = keyring::Entry::new(SERVICE, server) {
        let _ = e.delete_credential();
    }
    if let Some(f) = token_file() {
        let _ = std::fs::remove_file(f);
    }
}
