// SPDX-License-Identifier: AGPL-3.0-only
//! Entra group sync: keeps linked groups' members in line with Microsoft.

use std::collections::HashSet;

use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit::{self, AuditActor, Event};
use crate::error::ApiError;
use crate::graph::{self, GraphError};
use crate::microsoft::Provider;
use crate::state::AppState;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncCounts {
    pub added: u32,
    pub removed: u32,
}

fn db_err(e: impl std::fmt::Display) -> GraphError {
    GraphError::Failed(format!("database error: {e}"))
}

const MAX_NAME: usize = 100;

/// A group name free in the organisation: `base`, else `base (Microsoft)`,
/// else `base (Microsoft 2)`, ... Group `own` (if any) does not count as a clash.
pub async fn free_name(
    db: &mut PgConnection,
    org: Uuid,
    base: &str,
    own: Option<Uuid>,
) -> Result<String, sqlx::Error> {
    let base: String = base.trim().chars().take(MAX_NAME - 16).collect();
    let mut n = 0u32;
    loop {
        let cand = match n {
            0 => base.clone(),
            1 => format!("{base} (Microsoft)"),
            k => format!("{base} (Microsoft {k})"),
        };
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM groups WHERE org_id = $1 AND name = $2
                            AND ($3::uuid IS NULL OR id <> $3))",
        )
        .bind(org)
        .bind(&cand)
        .bind(own)
        .fetch_one(&mut *db)
        .await?;
        if !taken {
            return Ok(cand);
        }
        n += 1;
    }
}

/// Make the members of a linked group match Microsoft, and refresh its name.
pub async fn sync_group(
    st: &AppState,
    p: &Provider,
    group_id: Uuid,
    actor: &AuditActor,
) -> Result<SyncCounts, GraphError> {
    let (org, external): (Uuid, Option<String>) =
        sqlx::query_as("SELECT org_id, external_id FROM groups WHERE id = $1 AND source = 'idp'")
            .bind(group_id)
            .fetch_optional(&st.db)
            .await
            .map_err(db_err)?
            .ok_or(GraphError::NotFound)?;
    let external = external.ok_or_else(|| GraphError::Failed("group has no external id".into()))?;

    let dir = graph::get_group(st, &p.app, &external).await?;
    let oids: Vec<String> = graph::group_user_ids(st, &p.app, &external)
        .await?
        .into_iter()
        .collect();

    let mut tx = st.db.begin().await.map_err(db_err)?;
    // Audit chain lock first, before any row this transaction writes.
    audit::lock(&mut tx).await.map_err(db_err)?;
    let want: HashSet<Uuid> = sqlx::query_scalar::<_, Uuid>(
        "SELECT u.id FROM user_identities i JOIN users u ON u.id = i.user_id
         WHERE i.idp_id = $1 AND i.subject = ANY($2) AND u.deleted_at IS NULL",
    )
    .bind(p.id)
    .bind(&oids)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?
    .into_iter()
    .collect();
    let have: HashSet<Uuid> =
        sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?
            .into_iter()
            .collect();
    let mut added: Vec<Uuid> = want.difference(&have).copied().collect();
    let mut removed: Vec<Uuid> = have.difference(&want).copied().collect();
    added.sort();
    removed.sort();

    sqlx::query("DELETE FROM group_members WHERE group_id = $1 AND user_id = ANY($2)")
        .bind(group_id)
        .bind(&removed)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO group_members (group_id, user_id) SELECT $1, unnest($2::uuid[])")
        .bind(group_id)
        .bind(&added)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    for (action, ids) in [
        ("group.member.added", &added),
        ("group.member.removed", &removed),
    ] {
        for uid in ids {
            audit::record(
                &mut tx,
                actor,
                Event::new(action)
                    .target("group", group_id)
                    .meta(json!({"user_id": uid, "source": "microsoft"})),
            )
            .await
            .map_err(db_err)?;
        }
    }
    let name = free_name(&mut tx, org, &dir.display_name, Some(group_id))
        .await
        .map_err(db_err)?;
    sqlx::query(
        "UPDATE groups SET name = $2, last_synced_at = now(), missing_since = NULL WHERE id = $1",
    )
    .bind(group_id)
    .bind(name)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(SyncCounts {
        added: added.len() as u32,
        removed: removed.len() as u32,
    })
}

/// Set one user's membership of every linked (non-missing) group from Microsoft.
/// Called at sign-in, so a person's access is right from their first request.
pub async fn sync_user(
    st: &AppState,
    p: &Provider,
    user_id: Uuid,
    oid: &str,
) -> Result<(), GraphError> {
    let Some((email, status, org)): Option<(String, String, Uuid)> = sqlx::query_as(
        "SELECT email::text, status, org_id FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&st.db)
    .await
    .map_err(db_err)?
    else {
        return Ok(());
    };
    if status == "disabled" {
        return Ok(());
    }
    let theirs: Vec<String> = graph::user_group_ids(st, &p.app, oid)
        .await?
        .into_iter()
        .collect();

    let mut tx = st.db.begin().await.map_err(db_err)?;
    // Audit chain lock first, before any row this transaction writes.
    audit::lock(&mut tx).await.map_err(db_err)?;
    let linked: Vec<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT g.id, g.external_id FROM groups g
         JOIN users u ON u.org_id = g.org_id
         WHERE u.id = $1 AND g.source = 'idp' AND g.missing_since IS NULL",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    let have: HashSet<Uuid> =
        sqlx::query_scalar::<_, Uuid>("SELECT group_id FROM group_members WHERE user_id = $1")
            .bind(user_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?
            .into_iter()
            .collect();
    let actor = AuditActor {
        id: Some(user_id),
        label: email,
        ip: None,
    };
    for (gid, external) in linked {
        let want = external.is_some_and(|e| theirs.contains(&e));
        let (sql, action) = match (want, have.contains(&gid)) {
            (true, false) => (
                "INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
                "group.member.added",
            ),
            (false, true) => (
                "DELETE FROM group_members WHERE group_id = $1 AND user_id = $2",
                "group.member.removed",
            ),
            _ => continue,
        };
        let done = sqlx::query(sql)
            .bind(gid)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
        if done == 0 {
            // Someone else (a full sync) got there first; nothing to record.
            continue;
        }
        audit::record(
            &mut tx,
            &actor,
            Event::new(action)
                .target("group", gid)
                .meta(json!({"user_id": user_id, "source": "microsoft"})),
        )
        .await
        .map_err(db_err)?;
    }
    recompute_roles(&mut tx, org, Some(user_id), &actor)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

/// The role each Microsoft-linked, non-owner, non-disabled user should have, as
/// `(user, current, wanted)`. `over` pretends one group has a different
/// `grants_role` (for previews). `lock` takes row locks on the users.
async fn role_targets(
    tx: &mut PgConnection,
    org: Uuid,
    only_user: Option<Uuid>,
    over: Option<(Uuid, Option<&str>)>,
    lock: bool,
) -> sqlx::Result<Vec<(Uuid, String, String)>> {
    let (og, ov) = (over.map(|o| o.0), over.and_then(|o| o.1));
    let has_over = over.is_some();
    // grants_role of group g, with the override applied.
    let eff = "CASE WHEN $4 AND g.id = $2 THEN $3 ELSE g.grants_role END";
    let any: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM groups g
                        WHERE g.org_id = $1 AND g.source = 'idp' AND {eff} IS NOT NULL)"
    ))
    .bind(org)
    .bind(og)
    .bind(ov)
    .bind(has_over)
    .fetch_one(&mut *tx)
    .await?;
    if !any {
        return Ok(Vec::new());
    }
    sqlx::query_as(&format!(
        "SELECT u.id, u.org_role, r.role
         FROM users u
         CROSS JOIN LATERAL (
             SELECT CASE WHEN bool_or(({eff}) = 'admin') THEN 'admin'
                         WHEN bool_or(({eff}) = 'auditor') THEN 'auditor'
                         ELSE 'member' END AS role
             FROM group_members gm JOIN groups g ON g.id = gm.group_id
             WHERE gm.user_id = u.id AND g.org_id = u.org_id AND g.source = 'idp'
         ) r
         WHERE u.org_id = $1 AND u.deleted_at IS NULL AND u.org_role <> 'owner'
           AND u.status <> 'disabled' AND ($5::uuid IS NULL OR u.id = $5)
           AND EXISTS (SELECT 1 FROM user_identities i
                       JOIN identity_providers ip ON ip.id = i.idp_id
                       WHERE i.user_id = u.id AND ip.org_id = u.org_id)
         ORDER BY u.id {}",
        if lock { "FOR UPDATE OF u" } else { "" }
    ))
    .bind(org)
    .bind(og)
    .bind(ov)
    .bind(has_over)
    .bind(only_user)
    .fetch_all(&mut *tx)
    .await
}

/// Put roles in line with the linked groups' `grants_role`. Returns how many
/// roles changed. Only Microsoft-linked, non-owner, non-disabled users are touched.
/// With no role-granting group left, managed users go back to manual and keep
/// their role. `only_user` limits the work to one person (sign-in).
///
/// Lock order: takes the audit chain lock first, then user rows, the same order
/// as every other path that audits about users (see `audit::lock`).
pub async fn recompute_roles(
    tx: &mut PgConnection,
    org: Uuid,
    only_user: Option<Uuid>,
    actor: &AuditActor,
) -> sqlx::Result<u32> {
    audit::lock(tx).await?;
    // Owners are never managed.
    sqlx::query(
        "UPDATE users SET role_source = 'manual'
         WHERE org_id = $1 AND org_role = 'owner' AND role_source = 'idp'
           AND ($2::uuid IS NULL OR id = $2)",
    )
    .bind(org)
    .bind(only_user)
    .execute(&mut *tx)
    .await?;
    let any: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM groups
                        WHERE org_id = $1 AND source = 'idp' AND grants_role IS NOT NULL)",
    )
    .bind(org)
    .fetch_one(&mut *tx)
    .await?;
    if !any {
        sqlx::query(
            "UPDATE users SET role_source = 'manual'
             WHERE org_id = $1 AND role_source = 'idp' AND ($2::uuid IS NULL OR id = $2)",
        )
        .bind(org)
        .bind(only_user)
        .execute(&mut *tx)
        .await?;
        return Ok(0);
    }
    let rows = role_targets(tx, org, only_user, None, true).await?;
    let mut changed = 0;
    for (id, from, to) in rows {
        sqlx::query("UPDATE users SET role_source = 'idp', org_role = $2 WHERE id = $1")
            .bind(id)
            .bind(&to)
            .execute(&mut *tx)
            .await?;
        if from != to {
            changed += 1;
            audit::record(
                &mut *tx,
                actor,
                Event::new("user.role.changed")
                    .target("user", id)
                    .meta(json!({"from": from, "to": to, "source": "microsoft"})),
            )
            .await?;
        }
    }
    Ok(changed)
}

/// How many people would change role if group `group` had `grants_role`.
/// Read-only: no locks, no writes, no audit events.
pub async fn preview_role_changes(
    db: &mut PgConnection,
    org: Uuid,
    group: Uuid,
    grants_role: Option<&str>,
) -> sqlx::Result<u32> {
    let rows = role_targets(db, org, None, Some((group, grants_role)), false).await?;
    Ok(rows.iter().filter(|(_, from, to)| from != to).count() as u32)
}

/// Recompute one person's role in its own transaction. Needs no Graph call, so
/// sign-in can still apply the mapping when Microsoft cannot be reached.
pub async fn recompute_user(st: &AppState, user_id: Uuid) -> sqlx::Result<u32> {
    let row: Option<(String, Uuid)> = sqlx::query_as(
        "SELECT email::text, org_id FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&st.db)
    .await?;
    let Some((email, org)) = row else {
        return Ok(0);
    };
    let actor = AuditActor {
        id: Some(user_id),
        label: email,
        ip: None,
    };
    let mut tx = st.db.begin().await?;
    let n = recompute_roles(&mut tx, org, Some(user_id), &actor).await?;
    tx.commit().await?;
    Ok(n)
}

/// How a Graph failure is shown to an administrator.
pub fn api_err(e: GraphError) -> ApiError {
    match e {
        GraphError::NotFound => ApiError::bad("That group was not found in Microsoft"),
        GraphError::Failed(m) => ApiError::bad(format!("Microsoft: {m}")),
    }
}

// ---- full sync ----

use serde::Serialize;
use time::OffsetDateTime;

/// What one run of the full sync did.
#[derive(Debug, Default, Clone, Serialize)]
pub struct RunSummary {
    pub groups: u32,
    pub added: u32,
    pub removed: u32,
    pub roles_changed: u32,
    pub disabled: u32,
    pub missing: u32,
    pub error: Option<String>,
    pub brake: Option<u32>,
}

const SYNC_LOCK_KEY: i64 = 0x6f70_6e72;
const FAILED_LOG_EVERY: time::Duration = time::Duration::hours(24);

/// True when a run is due: never ran, or the last run is over an hour old.
pub fn due(last: Option<OffsetDateTime>, now: OffsetDateTime) -> bool {
    last.is_none_or(|l| l < now - time::Duration::hours(1))
}

/// The group is gone from Microsoft: empty it and remember since when.
async fn mark_missing(
    st: &AppState,
    group_id: Uuid,
    actor: &AuditActor,
) -> Result<u32, GraphError> {
    let mut tx = st.db.begin().await.map_err(db_err)?;
    // Audit chain lock first, before any row this transaction writes.
    audit::lock(&mut tx).await.map_err(db_err)?;
    let removed: Vec<Uuid> =
        sqlx::query_scalar("DELETE FROM group_members WHERE group_id = $1 RETURNING user_id")
            .bind(group_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
    for uid in &removed {
        audit::record(
            &mut tx,
            actor,
            Event::new("group.member.removed")
                .target("group", group_id)
                .meta(json!({"user_id": uid, "source": "microsoft"})),
        )
        .await
        .map_err(db_err)?;
    }
    let first: Option<String> = sqlx::query_scalar(
        "UPDATE groups g SET missing_since = now(), last_synced_at = now()
         FROM (SELECT id, missing_since AS was, name FROM groups WHERE id = $1 FOR UPDATE) o
         WHERE g.id = o.id AND o.was IS NULL RETURNING o.name",
    )
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    if let Some(name) = first {
        audit::record(
            &mut tx,
            actor,
            Event::new("idp.group.missing")
                .target("group", group_id)
                .meta(json!({"name": name})),
        )
        .await
        .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(removed.len() as u32)
}

/// Sync every linked group. Takes a database-wide lock so overlapping runs
/// (scheduler, "Sync now", another instance) never interleave. The run itself
/// is detached, so cancelling the caller (a dropped HTTP request, shutdown)
/// cannot abandon it half-way with the lock still held.
pub async fn sync_all(
    st: &AppState,
    actor: &AuditActor,
    confirm_brake: bool,
) -> anyhow::Result<RunSummary> {
    let (st, actor) = (st.clone(), actor.clone());
    tokio::spawn(async move { sync_all_inner(&st, &actor, confirm_brake).await }).await?
}

async fn sync_all_inner(
    st: &AppState,
    actor: &AuditActor,
    _confirm_brake: bool,
) -> anyhow::Result<RunSummary> {
    let mut sum = RunSummary::default();
    let Some(org): Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM organizations ORDER BY created_at LIMIT 1")
            .fetch_optional(&st.db)
            .await?
    else {
        return Ok(sum);
    };
    let Some(p) = crate::microsoft::load(st, &st.db, org)
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?
        .filter(|p| p.group_sync_enabled)
    else {
        return Ok(sum);
    };
    if !graph::tenant_is_specific(&p.app.tenant_id) {
        let msg =
            "Group sync needs your specific tenant ID (not organizations, common or consumers).";
        sqlx::query(
            "UPDATE identity_providers SET group_sync_last_at = now(), group_sync_last_error = $2
             WHERE id = $1",
        )
        .bind(p.id)
        .bind(msg)
        .execute(&st.db)
        .await?;
        sum.error = Some(msg.into());
        return Ok(sum);
    }

    let mut lock = st.db.acquire().await?;
    let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(SYNC_LOCK_KEY)
        .fetch_one(&mut *lock)
        .await?;
    if !got {
        sum.error = Some("a sync is already running".into());
        return Ok(sum);
    }
    let res = run_locked(st, org, &p, actor, &mut sum).await;
    let unlocked = sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
        .bind(SYNC_LOCK_KEY)
        .fetch_one(&mut *lock)
        .await;
    if !matches!(unlocked, Ok(true)) {
        // Never hand a connection that may still hold the lock back to the pool.
        let _ = lock.detach();
    }
    res?;
    Ok(sum)
}

async fn run_locked(
    st: &AppState,
    org: Uuid,
    p: &Provider,
    actor: &AuditActor,
    sum: &mut RunSummary,
) -> anyhow::Result<()> {
    let (prev_error, logged_at): (Option<String>, Option<OffsetDateTime>) = sqlx::query_as(
        "SELECT group_sync_last_error, group_sync_failed_logged_at
         FROM identity_providers WHERE id = $1",
    )
    .bind(p.id)
    .fetch_one(&st.db)
    .await?;

    let groups: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM groups WHERE org_id = $1 AND source = 'idp' ORDER BY name, id",
    )
    .bind(org)
    .fetch_all(&st.db)
    .await?;
    for gid in groups {
        sum.groups += 1;
        let r = match sync_group(st, p, gid, actor).await {
            Ok(c) => Ok((c.added, c.removed)),
            Err(GraphError::NotFound) => {
                sum.missing += 1;
                mark_missing(st, gid, actor).await.map(|n| (0, n))
            }
            Err(e) => Err(e),
        };
        match r {
            Ok((a, rm)) => {
                sum.added += a;
                sum.removed += rm;
            }
            Err(e) => {
                sum.error = Some(match e {
                    GraphError::Failed(m) => m,
                    GraphError::NotFound => "group not found".into(),
                });
                break;
            }
        }
    }

    let roles: sqlx::Result<u32> = async {
        let mut tx = st.db.begin().await?;
        let n = recompute_roles(&mut tx, org, None, actor).await?;
        tx.commit().await?;
        Ok(n)
    }
    .await;
    match roles {
        Ok(n) => sum.roles_changed = n,
        Err(e) if sum.error.is_none() => sum.error = Some(format!("database error: {e}")),
        Err(_) => {}
    }

    sqlx::query(
        "UPDATE identity_providers SET group_sync_last_at = now(),
            group_sync_last_ok_at = CASE WHEN $2::text IS NULL THEN now() ELSE group_sync_last_ok_at END,
            group_sync_last_error = $2
         WHERE id = $1",
    )
    .bind(p.id)
    .bind(&sum.error)
    .execute(&st.db)
    .await?;

    if let Some(err) = &sum.error {
        let now = OffsetDateTime::now_utc();
        let log = prev_error.is_none() || logged_at.is_none_or(|t| t < now - FAILED_LOG_EVERY);
        if log {
            let mut tx = st.db.begin().await?;
            audit::record(
                &mut tx,
                actor,
                Event::new("idp.sync.failed")
                    .target("identity_provider", p.id)
                    .meta(json!({"error": err})),
            )
            .await?;
            sqlx::query(
                "UPDATE identity_providers SET group_sync_failed_logged_at = now() WHERE id = $1",
            )
            .bind(p.id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
        }
    }
    Ok(())
}

/// Hourly background sync, checked every five minutes.
pub fn spawn_scheduler(st: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            tick.tick().await;
            if let Err(e) = maybe_run(&st).await {
                tracing::warn!(error = %e, "scheduled group sync failed");
            }
        }
    });
}

async fn maybe_run(st: &AppState) -> anyhow::Result<()> {
    let row: Option<(Option<OffsetDateTime>,)> = sqlx::query_as(
        "SELECT group_sync_last_at FROM identity_providers WHERE group_sync_enabled LIMIT 1",
    )
    .fetch_optional(&st.db)
    .await?;
    if let Some((last,)) = row
        && due(last, OffsetDateTime::now_utc())
    {
        let actor = AuditActor {
            id: None,
            label: "Microsoft sync".into(),
            ip: None,
        };
        sync_all(st, &actor, false).await?;
    }
    Ok(())
}
