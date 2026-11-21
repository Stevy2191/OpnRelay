// SPDX-License-Identifier: AGPL-3.0-only
//! OneDrive (Microsoft Graph) upload of encrypted backup files.
//!
//! The connection uses delegated permissions (`Files.ReadWrite`,
//! `offline_access`) granted by an administrator once; the refresh token is
//! stored encrypted. Files are uploaded with resumable upload sessions and
//! older backups beyond the retention count are deleted.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::microsoft;
use crate::state::AppState;

/// Upload chunk size: must be a multiple of 320 KiB (Graph requirement).
const CHUNK: usize = 320 * 1024 * 32; // 10 MiB

pub fn seal_token(st: &AppState, org: Uuid, token: &str) -> Result<Vec<u8>> {
    opnrelay_crypto::seal(
        &st.secrets_key,
        token.as_bytes(),
        &[b"onedrive:".as_slice(), org.as_bytes()].concat(),
    )
    .map_err(|e| anyhow!("{e}"))
}

fn open_token(st: &AppState, org: Uuid, enc: &[u8]) -> Result<String> {
    let b = opnrelay_crypto::open(
        &st.secrets_key,
        enc,
        &[b"onedrive:".as_slice(), org.as_bytes()].concat(),
    )
    .map_err(|e| anyhow!("{e}"))?;
    Ok(String::from_utf8(b)?)
}

fn path_segment(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~ ".contains(&b) {
                if b == b' ' {
                    "%20".to_string()
                } else {
                    (b as char).to_string()
                }
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Folder names: plain, no path tricks.
pub fn valid_folder(f: &str) -> bool {
    let f = f.trim();
    !f.is_empty()
        && f.len() <= 100
        && !f.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        && f != "."
        && f != ".."
}

/// Fresh access token (rotating the stored refresh token).
async fn access_token(st: &AppState, org: Uuid) -> Result<String> {
    let enc: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT onedrive_token_enc FROM backup_settings WHERE org_id = $1")
            .bind(org)
            .fetch_one(&st.db)
            .await?;
    let refresh = open_token(
        st,
        org,
        &enc.ok_or_else(|| anyhow!("OneDrive is not connected"))?,
    )?;
    let provider = microsoft::load(st, &st.db, org)
        .await
        .map_err(|e| anyhow!("{e}"))?
        .ok_or_else(|| anyhow!("the Microsoft app is not configured"))?;
    let t = microsoft::refresh(st, &provider.app, &refresh, microsoft::ONEDRIVE_SCOPE)
        .await
        .map_err(|e| anyhow!("{e}; reconnect OneDrive in Admin → Backups"))?;
    if let Some(new) = &t.refresh_token {
        sqlx::query("UPDATE backup_settings SET onedrive_token_enc = $2 WHERE org_id = $1")
            .bind(org)
            .bind(seal_token(st, org, new)?)
            .execute(&st.db)
            .await?;
    }
    t.access_token
        .ok_or_else(|| anyhow!("Microsoft returned no access token"))
}

pub async fn account_name(st: &AppState, access: &str) -> Result<String> {
    let me: Value = reqwest::Client::new()
        .get(format!("{}/me", st.cfg.ms_graph_url))
        .bearer_auth(access)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(me["userPrincipalName"]
        .as_str()
        .or(me["mail"].as_str())
        .unwrap_or("OneDrive")
        .to_string())
}

/// Upload one backup file, then delete old backups beyond `keep_remote`.
pub async fn upload_backup(st: &AppState, org: Uuid, file: &Path) -> Result<()> {
    let (folder, keep): (String, i32) = sqlx::query_as(
        "SELECT onedrive_folder, keep_remote FROM backup_settings WHERE org_id = $1",
    )
    .bind(org)
    .fetch_one(&st.db)
    .await?;
    let access = access_token(st, org).await?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("bad file name"))?;
    let graph = &st.cfg.ms_graph_url;

    let session: Value = http
        .post(format!(
            "{graph}/me/drive/root:/{}/{}:/createUploadSession",
            path_segment(&folder),
            path_segment(name)
        ))
        .bearer_auth(&access)
        .json(&json!({"item": {"@microsoft.graph.conflictBehavior": "replace"}}))
        .send()
        .await?
        .error_for_status()
        .context("could not start the OneDrive upload")?
        .json()
        .await?;
    let upload_url = session["uploadUrl"]
        .as_str()
        .ok_or_else(|| anyhow!("OneDrive returned no upload URL"))?
        .to_string();

    let total = tokio::fs::metadata(file).await?.len();
    let mut f = tokio::fs::File::open(file).await?;
    let mut offset: u64 = 0;
    let mut buf = vec![0u8; CHUNK];
    while offset < total {
        let mut n = 0;
        while n < CHUNK && offset + (n as u64) < total {
            let k = f.read(&mut buf[n..]).await?;
            if k == 0 {
                break;
            }
            n += k;
        }
        let end = offset + n as u64 - 1;
        // The upload URL is pre-authorised; Graph requires no bearer token here.
        let r = http
            .put(&upload_url)
            .header("Content-Range", format!("bytes {offset}-{end}/{total}"))
            .body(buf[..n].to_vec())
            .send()
            .await?;
        if !r.status().is_success() {
            bail!("OneDrive rejected part of the upload ({})", r.status());
        }
        offset = end + 1;
    }

    // Retention: keep the newest `keep` backups in the folder.
    let list: Value = http
        .get(format!(
            "{graph}/me/drive/root:/{}:/children?$select=id,name&$top=999",
            path_segment(&folder)
        ))
        .bearer_auth(&access)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut items: Vec<(String, String)> = list["value"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| {
                    Some((
                        i["name"].as_str()?.to_string(),
                        i["id"].as_str()?.to_string(),
                    ))
                })
                .filter(|(n, _)| crate::backup::is_backup_name(n))
                .collect()
        })
        .unwrap_or_default();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, id) in items.iter().skip(keep.max(1) as usize) {
        let _ = http
            .delete(format!("{graph}/me/drive/items/{}", path_segment(id)))
            .bearer_auth(&access)
            .send()
            .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_paths() {
        assert!(valid_folder("OpnRelay Backups"));
        assert!(!valid_folder("../x"));
        assert!(!valid_folder("a/b"));
        assert_eq!(path_segment("OpnRelay Backups"), "OpnRelay%20Backups");
        assert_eq!(CHUNK % (320 * 1024), 0);
    }
}
