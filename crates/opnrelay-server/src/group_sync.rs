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

/// How a Graph failure is shown to an administrator.
pub fn api_err(e: GraphError) -> ApiError {
    match e {
        GraphError::NotFound => ApiError::bad("That group was not found in Microsoft"),
        GraphError::Failed(m) => ApiError::bad(format!("Microsoft: {m}")),
    }
}
