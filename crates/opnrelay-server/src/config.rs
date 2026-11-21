// SPDX-License-Identifier: AGPL-3.0-only
//! Runtime configuration, read from environment variables.
//!
//! Every secret variable `X` can alternatively be supplied as `X_FILE`
//! pointing at a file (Docker secrets).

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use opnrelay_crypto::SecretKey;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    /// External URL users reach the server at, without trailing slash.
    pub public_url: String,
    pub database_url: String,
    pub master_key: SecretKey,
    pub storage_dir: PathBuf,
    /// Set the `Secure` attribute (and `__Host-` prefix) on cookies.
    pub cookie_secure: bool,
    /// Trust `X-Forwarded-For` from the reverse proxy for client IPs.
    pub trust_proxy: bool,
    pub max_upload_bytes: usize,
    /// Folder with desktop recorder / extension packages offered on the
    /// Downloads page (filled by the installer and `opnrelay update`).
    pub downloads_dir: Option<PathBuf>,
    /// Where local backups are written.
    pub backup_dir: PathBuf,
    /// Microsoft identity platform and Graph base URLs (overridable for tests).
    pub ms_login_url: String,
    pub ms_graph_url: String,
}

fn var(name: &str) -> Result<Option<String>> {
    if let Ok(v) = std::env::var(name) {
        return Ok(Some(v));
    }
    let file_var = format!("{name}_FILE");
    if let Ok(path) = std::env::var(&file_var) {
        let v =
            std::fs::read_to_string(&path).with_context(|| format!("reading {file_var}={path}"))?;
        return Ok(Some(v.trim().to_string()));
    }
    Ok(None)
}

fn required(name: &str) -> Result<String> {
    var(name)?.with_context(|| format!("{name} (or {name}_FILE) must be set"))
}

fn bool_var(name: &str, default: bool) -> Result<bool> {
    Ok(match var(name)?.as_deref() {
        None => default,
        Some("1" | "true" | "yes" | "on") => true,
        Some("0" | "false" | "no" | "off") => false,
        Some(other) => bail!("{name}: expected true/false, got {other:?}"),
    })
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let public_url = required("OPNRELAY_PUBLIC_URL")?
            .trim_end_matches('/')
            .to_string();
        let https = public_url.starts_with("https://");
        let bind = var("OPNRELAY_BIND")?
            .unwrap_or_else(|| "0.0.0.0:8080".into())
            .parse()
            .context("OPNRELAY_BIND must be host:port")?;
        let master_key = SecretKey::from_text(&required("OPNRELAY_MASTER_KEY")?)
            .context("OPNRELAY_MASTER_KEY must be 32 bytes as 64 hex chars or base64url")?;
        let storage = var("OPNRELAY_STORAGE")?.unwrap_or_else(|| "fs:///data/media".into());
        let storage_dir = match storage.strip_prefix("fs://") {
            Some(path) => PathBuf::from(path),
            None => bail!("OPNRELAY_STORAGE: only fs:// storage is supported in this version"),
        };
        let max_upload_mb: usize = var("OPNRELAY_MAX_UPLOAD_MB")?
            .map(|v| v.parse())
            .transpose()
            .context("OPNRELAY_MAX_UPLOAD_MB must be a number")?
            .unwrap_or(50);
        Ok(Self {
            bind,
            database_url: required("OPNRELAY_DATABASE_URL")?,
            master_key,
            storage_dir,
            cookie_secure: bool_var("OPNRELAY_COOKIE_SECURE", https)?,
            trust_proxy: bool_var("OPNRELAY_TRUST_PROXY", false)?,
            max_upload_bytes: max_upload_mb * 1024 * 1024,
            downloads_dir: var("OPNRELAY_DOWNLOADS_DIR")?.map(PathBuf::from),
            backup_dir: var("OPNRELAY_BACKUP_DIR")?
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/data/backups")),
            ms_login_url: var("OPNRELAY_MS_LOGIN_URL")?
                .unwrap_or_else(|| "https://login.microsoftonline.com".into())
                .trim_end_matches('/')
                .to_string(),
            ms_graph_url: var("OPNRELAY_MS_GRAPH_URL")?
                .unwrap_or_else(|| "https://graph.microsoft.com/v1.0".into())
                .trim_end_matches('/')
                .to_string(),
            public_url,
        })
    }

    pub fn session_cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            "__Host-opnrelay_session"
        } else {
            "opnrelay_session"
        }
    }
}
