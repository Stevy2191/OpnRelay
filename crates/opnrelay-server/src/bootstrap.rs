// SPDX-License-Identifier: AGPL-3.0-only
//! First-run setup: organisation and owner account.

use anyhow::{Context, Result, bail};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::audit::{self, AuditActor, Event};

/// Return the organisation id, creating the organisation on first run.
pub async fn ensure_org(db: &PgPool, name: &str) -> Result<Uuid> {
    if let Some(id) =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM organizations ORDER BY created_at LIMIT 1")
            .fetch_optional(db)
            .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    let mut tx = db.begin().await?;
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, $2)")
        .bind(id)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO org_policies (org_id) VALUES ($1)")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO branding (org_id) VALUES ($1)")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &AuditActor::system(),
        Event::new("org.created")
            .target("organization", id)
            .meta(json!({"name": name})),
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Create an owner account and return a one-time link to set its password.
pub async fn create_owner(
    db: &PgPool,
    public_url: &str,
    org_name: &str,
    email: &str,
    name: &str,
) -> Result<String> {
    let email = email.trim().to_lowercase();
    if !crate::util::valid_email(&email) {
        bail!("invalid email address");
    }
    let org = ensure_org(db, org_name).await?;
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(db)
        .await?;
    let mut tx = db.begin().await?;
    let user_id = match exists {
        Some(id) => {
            sqlx::query("UPDATE users SET org_role = 'owner', deleted_at = NULL WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            id
        }
        None => {
            let id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO users (id, org_id, email, display_name, org_role, status) VALUES ($1,$2,$3,$4,'owner','invited')",
            )
            .bind(id)
            .bind(org)
            .bind(&email)
            .bind(name)
            .execute(&mut *tx)
            .await?;
            id
        }
    };
    let status: String = sqlx::query_scalar("SELECT status FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
    let purpose = if status == "invited" {
        "invite"
    } else {
        "password_reset"
    };
    let url = crate::routes::admin::create_link(&mut tx, public_url, org, user_id, None, purpose)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("creating setup link")?;
    audit::record(
        &mut tx,
        &AuditActor::system(),
        Event::new("user.owner.bootstrap")
            .target("user", user_id)
            .meta(json!({"email": email})),
    )
    .await?;
    tx.commit().await?;
    Ok(url)
}
