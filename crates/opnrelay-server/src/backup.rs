// SPDX-License-Identifier: AGPL-3.0-only
//! Full backups (database + screenshots) and restore.
//!
//! A backup file (`opnrelay-backup-YYYYMMDD-HHMMSS.orbk`) is a tar archive,
//! encrypted with `opnrelay_crypto::stream` using a key derived from the
//! master key, containing:
//!
//! * `manifest.json`: format version, schema version, tables and columns
//! * `db/<table>.copy`: a consistent snapshot in PostgreSQL COPY text format
//! * `media/<storage key>`: screenshot/logo objects (already encrypted at rest)

use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use futures_util::TryStreamExt;
use opnrelay_crypto::SecretKey;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Connection, PgConnection, PgPool};
use time::OffsetDateTime;
use time::macros::format_description;
use uuid::Uuid;

use crate::audit::{self, AuditActor, Event};
use crate::state::AppState;

pub const FORMAT: &str = "opnrelay-backup";
pub const EXTENSION: &str = "orbk";

/// Tables in restore order (parents first). Ephemeral tables (sessions,
/// one-time codes, SSO states, jobs) are deliberately not backed up.
pub const TABLES: &[&str] = &[
    "organizations",
    "org_policies",
    "branding",
    "backup_settings",
    "users",
    "user_preferences",
    "mfa_factors",
    "recovery_codes",
    "invitations",
    "groups",
    "group_members",
    "identity_providers",
    "user_identities",
    "workspaces",
    "workspace_members",
    "folders",
    "media",
    "guides",
    "steps",
    "guide_versions",
    "share_links",
    "device_grants",
    "device_refresh_history",
    "audit_events",
];

static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub app_version: String,
    pub schema_version: i64,
    pub created_at: String,
    pub tables: Vec<TableInfo>,
    pub media_files: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: u64,
}

pub fn backup_key(master: &SecretKey) -> SecretKey {
    master.derive("backup-v1")
}

pub fn is_backup_name(name: &str) -> bool {
    name.starts_with("opnrelay-backup-")
        && name.ends_with(&format!(".{EXTENSION}"))
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
}

async fn columns(conn: &mut PgConnection, table: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns
         WHERE table_schema = current_schema() AND table_name = $1 ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(&mut *conn)
    .await?)
}

fn quote_cols(cols: &[String]) -> String {
    cols.iter()
        .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Local backup files, newest first.
pub async fn list_local(dir: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            let name = e.file_name().to_string_lossy().to_string();
            if is_backup_name(&name)
                && let Ok(m) = e.metadata().await
            {
                out.push((name, m.len()));
            }
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

/// Create an encrypted backup file in `st.cfg.backup_dir`. Returns its path.
pub async fn create(st: &AppState) -> Result<PathBuf> {
    let dir = st.cfg.backup_dir.clone();
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("creating {}", dir.display()))?;

    // 1. Consistent snapshot of every table.
    let mut conn = st.db.acquire().await?;
    sqlx::query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *conn)
        .await?;
    let mut dumps: Vec<(String, Vec<u8>)> = Vec::new();
    let mut tables = Vec::new();
    let mut media: Vec<String> = Vec::new();
    let snapshot = async {
        for t in TABLES {
            let cols = columns(&mut conn, t).await?;
            let mut data = Vec::new();
            let mut stream = conn
                .copy_out_raw(&format!("COPY \"{t}\" ({}) TO STDOUT", quote_cols(&cols)))
                .await?;
            while let Some(chunk) = stream.try_next().await? {
                data.extend_from_slice(&chunk);
            }
            drop(stream);
            let rows = data.iter().filter(|b| **b == b'\n').count() as u64;
            tables.push(TableInfo {
                name: t.to_string(),
                columns: cols,
                rows,
            });
            dumps.push((format!("db/{t}.copy"), data));
        }
        media = sqlx::query_scalar("SELECT storage_key FROM media")
            .fetch_all(&mut *conn)
            .await?;
        let schema: i64 = sqlx::query_scalar(
            "SELECT COALESCE(max(version), 0) FROM _sqlx_migrations WHERE success",
        )
        .fetch_one(&mut *conn)
        .await?;
        anyhow::Ok(schema)
    };
    let schema = snapshot.await;
    sqlx::query("COMMIT").execute(&mut *conn).await?;
    let schema = schema?;
    drop(conn);

    let now = OffsetDateTime::now_utc();
    let name = format!(
        "opnrelay-backup-{}.{EXTENSION}",
        now.format(format_description!(
            "[year][month][day]-[hour][minute][second]"
        ))?
    );
    let manifest = Manifest {
        format: FORMAT.into(),
        version: 1,
        app_version: env!("CARGO_PKG_VERSION").into(),
        schema_version: schema,
        created_at: now.format(&time::format_description::well_known::Rfc3339)?,
        tables,
        media_files: media.len(),
    };

    // 2. Tar + encrypt on a blocking thread (file I/O, CPU).
    let storage_root = st.cfg.storage_dir.clone();
    let key = backup_key(&st.cfg.master_key);
    let final_path = dir.join(&name);
    let tmp_tar = dir.join(format!(".tmp-{}.tar", Uuid::now_v7()));
    let tmp_enc = dir.join(format!(".tmp-{}.{EXTENSION}", Uuid::now_v7()));
    let (tt, te, fp) = (tmp_tar.clone(), tmp_enc.clone(), final_path.clone());
    let res = tokio::task::spawn_blocking(move || -> Result<()> {
        {
            let mut tar = tar::Builder::new(BufWriter::new(std::fs::File::create(&tt)?));
            let add = |tar: &mut tar::Builder<BufWriter<std::fs::File>>,
                       path: &str,
                       data: &[u8]|
             -> Result<()> {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o600);
                h.set_mtime(now.unix_timestamp().max(0) as u64);
                h.set_cksum();
                tar.append_data(&mut h, path, data)?;
                Ok(())
            };
            add(
                &mut tar,
                "manifest.json",
                &serde_json::to_vec_pretty(&manifest)?,
            )?;
            for (path, data) in &dumps {
                add(&mut tar, path, data)?;
            }
            for key in &media {
                if key.split('/').any(|p| p.is_empty() || p == "..") {
                    continue;
                }
                match std::fs::read(storage_root.join(key)) {
                    Ok(bytes) => add(&mut tar, &format!("media/{key}"), &bytes)?,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            tar.into_inner()?
                .into_inner()
                .map_err(|e| anyhow!("{e}"))?
                .sync_all()?;
        }
        let input = BufReader::new(std::fs::File::open(&tt)?);
        let out = std::fs::File::create(&te)?;
        opnrelay_crypto::stream::encrypt(&key, input, BufWriter::new(&out))
            .map_err(|e| anyhow!("{e}"))?;
        out.sync_all()?;
        std::fs::rename(&te, &fp)?;
        Ok(())
    })
    .await?;
    let _ = tokio::fs::remove_file(&tmp_tar).await;
    let _ = tokio::fs::remove_file(&tmp_enc).await;
    res?;
    Ok(final_path)
}

/// Delete local backups beyond the newest `keep`.
pub async fn prune_local(dir: &Path, keep: usize) -> Result<usize> {
    let files = list_local(dir).await;
    let mut removed = 0;
    for (name, _) in files.iter().skip(keep) {
        tokio::fs::remove_file(dir.join(name)).await?;
        removed += 1;
    }
    Ok(removed)
}

#[derive(sqlx::FromRow)]
struct Settings {
    org_id: Uuid,
    keep_local: i32,
    onedrive_enabled: bool,
}

/// Run a backup now (local, then OneDrive if connected) and record the
/// outcome. Only one backup runs at a time.
pub async fn run(st: &AppState, actor: AuditActor) -> Result<PathBuf> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        bail!("a backup is already running");
    }
    let result = run_inner(st).await;
    RUNNING.store(false, Ordering::SeqCst);
    let s: Option<Settings> =
        sqlx::query_as("SELECT org_id, keep_local, onedrive_enabled FROM backup_settings LIMIT 1")
            .fetch_optional(&st.db)
            .await?;
    let org = s.map(|s| s.org_id);
    match &result {
        Ok((path, size, remote)) => {
            sqlx::query(
                "UPDATE backup_settings SET last_run_at = now(), last_status = 'ok', last_error = NULL,
                    last_file = $1, last_size = $2, last_remote_at = CASE WHEN $3 THEN now() ELSE last_remote_at END",
            )
            .bind(path.file_name().map(|n| n.to_string_lossy().to_string()))
            .bind(*size as i64)
            .bind(*remote)
            .execute(&st.db)
            .await?;
            audit::record_now(
                &st.db,
                &actor,
                Event::new("backup.completed").meta(json!({"file": path.file_name().map(|n| n.to_string_lossy()), "bytes": size, "onedrive": remote})),
            )
            .await?;
        }
        Err(e) => {
            sqlx::query("UPDATE backup_settings SET last_run_at = now(), last_status = 'failed', last_error = $1")
                .bind(e.to_string().chars().take(500).collect::<String>())
                .execute(&st.db)
                .await?;
            audit::record_now(
                &st.db,
                &actor,
                Event::new("backup.failed").meta(json!({"error": e.to_string()})),
            )
            .await?;
        }
    }
    let _ = org;
    result.map(|(p, _, _)| p)
}

async fn run_inner(st: &AppState) -> Result<(PathBuf, u64, bool)> {
    let s: Settings =
        sqlx::query_as("SELECT org_id, keep_local, onedrive_enabled FROM backup_settings LIMIT 1")
            .fetch_one(&st.db)
            .await
            .context("backup settings missing")?;
    let path = create(st).await?;
    let size = tokio::fs::metadata(&path).await?.len();
    let mut remote = false;
    if s.onedrive_enabled {
        crate::onedrive::upload_backup(st, s.org_id, &path)
            .await
            .context("OneDrive upload failed (the local backup was kept)")?;
        remote = true;
    }
    prune_local(&st.cfg.backup_dir, s.keep_local.max(1) as usize).await?;
    Ok((path, size, remote))
}

/// Background scheduler: once a day at the configured hour (server time, UTC).
pub fn spawn_scheduler(st: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            tick.tick().await;
            if let Err(e) = maybe_run(&st).await {
                tracing::warn!(error = %e, "scheduled backup check failed");
            }
        }
    });
}

async fn maybe_run(st: &AppState) -> Result<()> {
    let due: Option<bool> = sqlx::query_scalar(
        "SELECT enabled AND extract(hour FROM now() AT TIME ZONE 'UTC') >= schedule_hour
                AND (last_run_at IS NULL OR (last_run_at AT TIME ZONE 'UTC')::date < (now() AT TIME ZONE 'UTC')::date)
         FROM backup_settings LIMIT 1",
    )
    .fetch_optional(&st.db)
    .await?;
    if due == Some(true) {
        let _ = run(st, AuditActor::system()).await;
    }
    Ok(())
}

/// Restore a backup into an **empty** database (fresh install) and storage
/// directory. Returns the manifest.
pub async fn restore(
    db: &PgPool,
    storage_dir: &Path,
    master: &SecretKey,
    file: &Path,
) -> Result<Manifest> {
    let has_org: Option<Uuid> = sqlx::query_scalar("SELECT id FROM organizations LIMIT 1")
        .fetch_optional(db)
        .await?;
    if has_org.is_some() {
        bail!(
            "the database already contains an organisation; restore only into a fresh installation"
        );
    }
    // Decrypt and unpack on a blocking thread; media goes straight to storage.
    let key = backup_key(master);
    let (file, storage) = (file.to_path_buf(), storage_dir.to_path_buf());
    let (manifest, dumps) = tokio::task::spawn_blocking(
        move || -> Result<(Manifest, std::collections::HashMap<String, Vec<u8>>)> {
            let tmp = storage.join(format!(".restore-{}.tar", Uuid::now_v7()));
            std::fs::create_dir_all(&storage)?;
            {
                let out = BufWriter::new(std::fs::File::create(&tmp)?);
                opnrelay_crypto::stream::decrypt(
                    &key,
                    BufReader::new(std::fs::File::open(&file)?),
                    out,
                )
                .map_err(|_| {
                    anyhow!("cannot decrypt the backup: wrong master key or damaged file")
                })?;
            }
            let mut manifest: Option<Manifest> = None;
            let mut dumps = std::collections::HashMap::new();
            let mut ar = tar::Archive::new(BufReader::new(std::fs::File::open(&tmp)?));
            for entry in ar.entries()? {
                let mut e = entry?;
                let path = e.path()?.to_string_lossy().to_string();
                if path.split('/').any(|p| p == ".." || p.is_empty()) {
                    bail!("unsafe path in backup: {path}");
                }
                let mut data = Vec::new();
                e.read_to_end(&mut data)?;
                if path == "manifest.json" {
                    manifest = Some(serde_json::from_slice(&data)?);
                } else if let Some(key) = path.strip_prefix("media/") {
                    let dest = storage.join(key);
                    std::fs::create_dir_all(dest.parent().expect("parent"))?;
                    std::fs::write(dest, data)?;
                } else if path.starts_with("db/") {
                    dumps.insert(path, data);
                }
            }
            let _ = std::fs::remove_file(&tmp);
            let m = manifest.ok_or_else(|| anyhow!("backup has no manifest"))?;
            if m.format != FORMAT || m.version != 1 {
                bail!("unsupported backup format");
            }
            Ok((m, dumps))
        },
    )
    .await??;

    let mut conn = db.acquire().await?;
    let current_schema: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(version), 0) FROM _sqlx_migrations WHERE success")
            .fetch_one(&mut *conn)
            .await?;
    if manifest.schema_version > current_schema {
        bail!("this backup is from a newer OpnRelay version; upgrade first");
    }
    let mut tx = conn.begin().await?;
    // Load rows without firing triggers or FK checks (circular references);
    // requires the database owner role used by the standard deployment.
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await?;
    for t in &manifest.tables {
        if !TABLES.contains(&t.name.as_str()) {
            bail!("unknown table in backup: {}", t.name);
        }
        let current = columns(&mut tx, &t.name).await?;
        if let Some(missing) = t.columns.iter().find(|c| !current.contains(c)) {
            bail!("column {}.{missing} no longer exists", t.name);
        }
        let data = dumps
            .get(&format!("db/{}.copy", t.name))
            .ok_or_else(|| anyhow!("missing data for {}", t.name))?;
        let mut copy = tx
            .copy_in_raw(&format!(
                "COPY \"{}\" ({}) FROM STDIN",
                t.name,
                quote_cols(&t.columns)
            ))
            .await?;
        copy.send(data.as_slice()).await?;
        copy.finish().await?;
    }
    for (table, col) in [("audit_events", "seq"), ("jobs", "id")] {
        sqlx::query(&format!(
            "SELECT setval(pg_get_serial_sequence('{table}', '{col}'), GREATEST((SELECT COALESCE(max({col}), 0) FROM {table}), 1))"
        ))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    audit::record_now(
        db,
        &AuditActor::system(),
        Event::new("backup.restored").meta(
            json!({"created_at": manifest.created_at, "schema_version": manifest.schema_version}),
        ),
    )
    .await?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(is_backup_name("opnrelay-backup-20261121-020000.orbk"));
        assert!(!is_backup_name("opnrelay-backup-../x.orbk"));
        assert!(!is_backup_name("other.orbk"));
    }
}
