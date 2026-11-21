// SPDX-License-Identifier: AGPL-3.0-only
//! Authorisation: the single place that decides who may do what.
//!
//! * Owner / Admin: manage everything in the organisation.
//! * Auditor: read-only access to every workspace, plus the audit log.
//!   Auditors can never mutate content, whatever other grants exist.
//! * Member: per-workspace role (direct or via group), highest wins.

use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrgRole {
    Owner,
    Admin,
    Member,
    Auditor,
}

impl OrgRole {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "owner" => OrgRole::Owner,
            "admin" => OrgRole::Admin,
            "member" => OrgRole::Member,
            "auditor" => OrgRole::Auditor,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OrgRole::Owner => "owner",
            OrgRole::Admin => "admin",
            OrgRole::Member => "member",
            OrgRole::Auditor => "auditor",
        }
    }

    pub fn is_admin(self) -> bool {
        matches!(self, OrgRole::Owner | OrgRole::Admin)
    }

    pub fn can_read_audit(self) -> bool {
        matches!(self, OrgRole::Owner | OrgRole::Admin | OrgRole::Auditor)
    }
}

/// Workspace membership role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WsRole {
    Viewer = 1,
    Editor = 2,
    Manager = 3,
}

impl WsRole {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "viewer" => WsRole::Viewer,
            "editor" => WsRole::Editor,
            "manager" => WsRole::Manager,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            WsRole::Viewer => "viewer",
            WsRole::Editor => "editor",
            WsRole::Manager => "manager",
        }
    }
}

/// Effective permission on a workspace (and the guides in it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Perm {
    None = 0,
    Read = 1,
    Edit = 2,
    Manage = 3,
}

impl Perm {
    /// Ensure at least `needed`. A caller without any access gets 404 so the
    /// existence of the resource is not revealed.
    pub fn require(self, needed: Perm) -> ApiResult<()> {
        if self >= needed {
            Ok(())
        } else if self == Perm::None {
            Err(ApiError::NotFound)
        } else {
            Err(ApiError::Forbidden("you do not have permission to do this"))
        }
    }
}

/// Pure policy: combine org role and workspace role.
pub fn effective(org_role: OrgRole, ws_role: Option<WsRole>) -> Perm {
    match org_role {
        OrgRole::Owner | OrgRole::Admin => Perm::Manage,
        OrgRole::Auditor => Perm::Read,
        OrgRole::Member => match ws_role {
            Some(WsRole::Manager) => Perm::Manage,
            Some(WsRole::Editor) => Perm::Edit,
            Some(WsRole::Viewer) => Perm::Read,
            None => Perm::None,
        },
    }
}

/// SQL predicate (on alias `w`) selecting workspaces visible to user `$1`
/// when `$2` (bool) is false; all org workspaces when `$2` is true.
pub const VISIBLE_WORKSPACES_SQL: &str = "w.deleted_at IS NULL AND ($2 OR EXISTS (
    SELECT 1 FROM workspace_members wm
    WHERE wm.workspace_id = w.id AND (
        (wm.principal_type = 'user' AND wm.principal_id = $1) OR
        (wm.principal_type = 'group' AND wm.principal_id IN (SELECT group_id FROM group_members WHERE user_id = $1))
    )))";

/// Whether the actor sees every workspace (admins, auditors).
pub fn sees_all(actor: &Actor) -> bool {
    matches!(
        actor.role,
        OrgRole::Owner | OrgRole::Admin | OrgRole::Auditor
    )
}

async fn member_role(
    db: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> sqlx::Result<Option<WsRole>> {
    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT role FROM workspace_members wm
         WHERE wm.workspace_id = $1 AND (
            (wm.principal_type = 'user' AND wm.principal_id = $2) OR
            (wm.principal_type = 'group' AND wm.principal_id IN (SELECT group_id FROM group_members WHERE user_id = $2)))",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(roles.iter().filter_map(|r| WsRole::parse(r)).max())
}

/// Effective permission of `actor` on a workspace. Returns NotFound if the
/// workspace does not exist in the actor's organisation.
pub async fn workspace_perm(db: &PgPool, actor: &Actor, workspace_id: Uuid) -> ApiResult<Perm> {
    let exists: Option<bool> = sqlx::query_scalar(
        "SELECT true FROM workspaces WHERE id = $1 AND org_id = $2 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(actor.org_id)
    .fetch_optional(db)
    .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }
    let ws_role = if actor.role == OrgRole::Member {
        member_role(db, actor.user_id, workspace_id).await?
    } else {
        None
    };
    Ok(effective(actor.role, ws_role))
}

/// Permission on a guide (via its workspace). Returns `(perm, workspace_id)`.
pub async fn guide_perm(db: &PgPool, actor: &Actor, guide_id: Uuid) -> ApiResult<(Perm, Uuid)> {
    let ws: Option<Uuid> = sqlx::query_scalar(
        "SELECT g.workspace_id FROM guides g JOIN workspaces w ON w.id = g.workspace_id
         WHERE g.id = $1 AND g.deleted_at IS NULL AND w.deleted_at IS NULL AND w.org_id = $2",
    )
    .bind(guide_id)
    .bind(actor.org_id)
    .fetch_optional(db)
    .await?;
    let ws = ws.ok_or(ApiError::NotFound)?;
    Ok((workspace_perm(db, actor, ws).await?, ws))
}

pub fn require_admin(actor: &Actor) -> ApiResult<()> {
    if actor.role.is_admin() {
        Ok(())
    } else {
        Err(ApiError::Forbidden("administrator role required"))
    }
}

pub fn require_audit_reader(actor: &Actor) -> ApiResult<()> {
    if actor.role.can_read_audit() {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "auditor or administrator role required",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_matrix() {
        use OrgRole::*;
        assert_eq!(effective(Owner, None), Perm::Manage);
        assert_eq!(effective(Admin, Some(WsRole::Viewer)), Perm::Manage);
        // Auditors are read-only even if someone grants them a manager role.
        assert_eq!(effective(Auditor, Some(WsRole::Manager)), Perm::Read);
        assert_eq!(effective(Member, None), Perm::None);
        assert_eq!(effective(Member, Some(WsRole::Viewer)), Perm::Read);
        assert_eq!(effective(Member, Some(WsRole::Editor)), Perm::Edit);
        assert_eq!(effective(Member, Some(WsRole::Manager)), Perm::Manage);
    }

    #[test]
    fn require_hides_existence() {
        assert!(matches!(
            Perm::None.require(Perm::Read),
            Err(ApiError::NotFound)
        ));
        assert!(matches!(
            Perm::Read.require(Perm::Edit),
            Err(ApiError::Forbidden(_))
        ));
        assert!(Perm::Manage.require(Perm::Edit).is_ok());
    }
}
