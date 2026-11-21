// SPDX-License-Identifier: AGPL-3.0-only
//! Audit log access for administrators and auditors.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use sqlx::QueryBuilder;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::audit::{self, AuditRow, Event, VerifyReport};
use crate::auth::CurrentUser;
use crate::authz::require_audit_reader;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/audit/events", get(events))
        .route("/audit/verify", get(verify))
        .route("/audit/export", get(export))
}

#[derive(Deserialize)]
struct Filter {
    actor_id: Option<Uuid>,
    action: Option<String>,
    target_id: Option<Uuid>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    from: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    to: Option<OffsetDateTime>,
    before_seq: Option<i64>,
    limit: Option<i64>,
}

async fn query(st: &AppState, f: &Filter, max: i64) -> ApiResult<Vec<AuditRow>> {
    let mut qb = QueryBuilder::new(
        "SELECT seq, ts, actor_id, actor_label, actor_ip, action, target_type, target_id, metadata, prev_hash, hash
         FROM audit_events WHERE true",
    );
    if let Some(a) = f.actor_id {
        qb.push(" AND actor_id = ").push_bind(a);
    }
    if let Some(a) = f.action.as_deref().filter(|s| !s.is_empty()) {
        if a.len() > 100 {
            return Err(ApiError::bad("action filter too long"));
        }
        // Prefix match: "guide." matches all guide events.
        let like = format!(
            "{}%",
            a.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        qb.push(" AND action LIKE ").push_bind(like);
    }
    if let Some(t) = f.target_id {
        qb.push(" AND target_id = ").push_bind(t);
    }
    if let Some(t) = f.from {
        qb.push(" AND ts >= ").push_bind(t);
    }
    if let Some(t) = f.to {
        qb.push(" AND ts < ").push_bind(t);
    }
    if let Some(s) = f.before_seq {
        qb.push(" AND seq < ").push_bind(s);
    }
    qb.push(" ORDER BY seq DESC LIMIT ")
        .push_bind(f.limit.unwrap_or(100).clamp(1, max));
    Ok(qb.build_query_as::<AuditRow>().fetch_all(&st.db).await?)
}

async fn events(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Query(f): Query<Filter>,
) -> ApiResult<Json<Vec<AuditRow>>> {
    require_audit_reader(&a)?;
    Ok(Json(query(&st, &f, 500).await?))
}

async fn verify(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<VerifyReport>> {
    require_audit_reader(&a)?;
    let report = audit::verify(&st.db).await?;
    audit::record_now(
        &st.db,
        &a.audit(),
        Event::new("audit.verified")
            .meta(serde_json::to_value(&report).map_err(anyhow::Error::from)?),
    )
    .await?;
    Ok(Json(report))
}

fn csv_field(s: &str) -> String {
    // Quote everything; neutralise spreadsheet formula injection.
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[derive(Deserialize)]
struct ExportQuery {
    #[serde(default)]
    format: Option<String>,
    #[serde(flatten)]
    filter: FilterFlat,
}

#[derive(Deserialize)]
struct FilterFlat {
    actor_id: Option<Uuid>,
    action: Option<String>,
    target_id: Option<Uuid>,
}

async fn export(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Query(q): Query<ExportQuery>,
) -> ApiResult<Response> {
    require_audit_reader(&a)?;
    let f = Filter {
        actor_id: q.filter.actor_id,
        action: q.filter.action,
        target_id: q.filter.target_id,
        from: None,
        to: None,
        before_seq: None,
        limit: Some(100_000),
    };
    let rows = query(&st, &f, 100_000).await?;
    let format = q.format.as_deref().unwrap_or("csv");
    audit::record_now(
        &st.db,
        &a.audit(),
        Event::new("audit.exported")
            .meta(serde_json::json!({"format": format, "rows": rows.len()})),
    )
    .await?;
    let mut headers = HeaderMap::new();
    let (body, ctype, ext) = match format {
        "json" => (
            serde_json::to_vec_pretty(&rows).map_err(anyhow::Error::from)?,
            "application/json",
            "json",
        ),
        "csv" => {
            let mut out = String::from(
                "seq,ts,actor_id,actor_label,actor_ip,action,target_type,target_id,metadata,prev_hash,hash\n",
            );
            for r in &rows {
                let line = [
                    r.seq.to_string(),
                    r.ts.format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                    r.actor_id.map(|u| u.to_string()).unwrap_or_default(),
                    r.actor_label.clone(),
                    r.actor_ip.map(|i| i.ip().to_string()).unwrap_or_default(),
                    r.action.clone(),
                    r.target_type.clone(),
                    r.target_id.map(|u| u.to_string()).unwrap_or_default(),
                    r.metadata.to_string(),
                    hex::encode(&r.prev_hash),
                    hex::encode(&r.hash),
                ]
                .iter()
                .map(|s| csv_field(s))
                .collect::<Vec<_>>()
                .join(",");
                out.push_str(&line);
                out.push('\n');
            }
            (out.into_bytes(), "text/csv; charset=utf-8", "csv")
        }
        _ => return Err(ApiError::bad("format must be csv or json")),
    };
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"opnrelay-audit.{ext}\""))
            .expect("ascii"),
    );
    Ok((headers, body).into_response())
}

#[cfg(test)]
mod tests {
    use super::csv_field;

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_field("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_field("=cmd()"), "\"'=cmd()\"");
    }
}
