// SPDX-License-Identifier: AGPL-3.0-only
//! Microsoft Graph directory client (app-only token) for Entra group sync.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use serde_json::Value;

use crate::microsoft::AppConfig;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirGroup {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    /// "Security group" or "Microsoft 365 group".
    pub kind: String,
    /// Synced from on-premises Active Directory.
    pub on_prem: bool,
}

#[derive(Debug)]
pub enum GraphError {
    NotFound,
    Failed(String),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::NotFound => write!(f, "not found in the directory"),
            GraphError::Failed(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for GraphError {}

/// True for a concrete tenant (GUID or domain), false for multi-tenant aliases.
pub fn tenant_is_specific(tenant: &str) -> bool {
    let t = tenant.trim().to_ascii_lowercase();
    !t.is_empty() && !matches!(t.as_str(), "organizations" | "common" | "consumers")
}

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRY_WAIT: u64 = 10;

fn failed(e: impl std::fmt::Display) -> GraphError {
    GraphError::Failed(e.to_string())
}

/// App-only Graph token, cached until 5 minutes before it expires.
pub async fn app_token(st: &AppState, app: &AppConfig) -> Result<String, GraphError> {
    let mut cache = st.graph_token.lock().await;
    if let Some((tok, until)) = cache.as_ref()
        && Instant::now() < *until
    {
        return Ok(tok.clone());
    }
    let res = reqwest::Client::new()
        .post(format!(
            "{}/{}/oauth2/v2.0/token",
            st.cfg.ms_login_url, app.tenant_id
        ))
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", app.client_id.as_str()),
            ("client_secret", app.client_secret.as_str()),
            ("scope", "https://graph.microsoft.com/.default"),
        ])
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| failed(format!("could not reach Microsoft: {e}")))?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = body["error_description"]
            .as_str()
            .or(body["error"].as_str())
            .unwrap_or("token request failed")
            .lines()
            .next()
            .unwrap_or_default();
        return Err(failed(format!("Microsoft rejected the app token: {msg}")));
    }
    let tok = body["access_token"]
        .as_str()
        .ok_or_else(|| failed("Microsoft returned no access token"))?
        .to_string();
    let ttl = body["expires_in"].as_u64().unwrap_or(0);
    let until = Instant::now() + Duration::from_secs(ttl.saturating_sub(300));
    *cache = Some((tok.clone(), until));
    Ok(tok)
}

/// GET a Graph URL, retrying once on 429/503 (honouring `Retry-After`, capped).
async fn get_json(
    st: &AppState,
    app: &AppConfig,
    url: &str,
    query: &[(&str, &str)],
    eventual: bool,
) -> Result<Value, GraphError> {
    let http = reqwest::Client::new();
    let url = if query.is_empty() {
        reqwest::Url::parse(url)
    } else {
        reqwest::Url::parse_with_params(url, query)
    }
    .map_err(failed)?;
    for attempt in 0..2 {
        let token = app_token(st, app).await?;
        let mut req = http.get(url.clone()).bearer_auth(token).timeout(TIMEOUT);
        if eventual {
            req = req.header("ConsistencyLevel", "eventual");
        }
        let res = req
            .send()
            .await
            .map_err(|e| failed(format!("could not reach Microsoft Graph: {e}")))?;
        let status = res.status();
        if status == StatusCode::NOT_FOUND {
            return Err(GraphError::NotFound);
        }
        if matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
        ) && attempt == 0
        {
            let wait = res
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(1)
                .min(MAX_RETRY_WAIT);
            tokio::time::sleep(Duration::from_secs(wait)).await;
            continue;
        }
        if status == StatusCode::UNAUTHORIZED {
            // The cached token may have been revoked; drop it.
            *st.graph_token.lock().await = None;
        }
        if !status.is_success() {
            return Err(failed(format!("Microsoft Graph returned {status}")));
        }
        return res.json().await.map_err(failed);
    }
    Err(failed("Microsoft Graph is unavailable"))
}

/// All items of a listing, following `@odata.nextLink`.
async fn list_all(
    st: &AppState,
    app: &AppConfig,
    path: &str,
    query: &[(&str, &str)],
    eventual: bool,
) -> Result<Vec<Value>, GraphError> {
    let mut items = Vec::new();
    let first = format!("{}{path}", st.cfg.ms_graph_url);
    let mut body = get_json(st, app, &first, query, eventual).await?;
    loop {
        if let Some(v) = body["value"].as_array() {
            items.extend(v.iter().cloned());
        }
        match body["@odata.nextLink"].as_str() {
            Some(next) => {
                let next = next.to_string();
                body = get_json(st, app, &next, &[], eventual).await?;
            }
            None => return Ok(items),
        }
    }
}

fn dir_group(v: &Value) -> Option<DirGroup> {
    let unified = v["groupTypes"]
        .as_array()
        .is_some_and(|t| t.iter().any(|x| x == "Unified"));
    Some(DirGroup {
        id: v["id"].as_str()?.to_string(),
        display_name: v["displayName"].as_str().unwrap_or_default().to_string(),
        description: v["description"].as_str().map(String::from),
        kind: if unified {
            "Microsoft 365 group"
        } else {
            "Security group"
        }
        .into(),
        on_prem: v["onPremisesSyncEnabled"].as_bool().unwrap_or(false),
    })
}

const GROUP_SELECT: &str = "id,displayName,description,groupTypes,onPremisesSyncEnabled";

/// Groups whose name contains `q` (top 20).
pub async fn search_groups(
    st: &AppState,
    app: &AppConfig,
    q: &str,
) -> Result<Vec<DirGroup>, GraphError> {
    let term = q.replace(['"', '\\'], "");
    let search = format!("\"displayName:{term}\"");
    let url = format!("{}/groups", st.cfg.ms_graph_url);
    let body = get_json(
        st,
        app,
        &url,
        &[
            ("$search", search.as_str()),
            ("$select", GROUP_SELECT),
            ("$top", "20"),
        ],
        true,
    )
    .await?;
    Ok(body["value"]
        .as_array()
        .map(|v| v.iter().filter_map(dir_group).take(20).collect())
        .unwrap_or_default())
}

/// Ids go into URL paths, so only GUIDs are accepted.
fn check_id(id: &str) -> Result<(), GraphError> {
    if crate::microsoft::is_guid(id) {
        Ok(())
    } else {
        Err(failed("invalid id"))
    }
}

pub async fn get_group(st: &AppState, app: &AppConfig, id: &str) -> Result<DirGroup, GraphError> {
    check_id(id)?;
    let url = format!("{}/groups/{id}", st.cfg.ms_graph_url);
    let body = get_json(st, app, &url, &[("$select", GROUP_SELECT)], false).await?;
    dir_group(&body).ok_or_else(|| failed("unexpected group response"))
}

fn ids(items: &[Value]) -> HashSet<String> {
    items
        .iter()
        .filter_map(|v| v["id"].as_str().map(String::from))
        .collect()
}

/// Every user in the group, including through nested groups.
pub async fn group_user_ids(
    st: &AppState,
    app: &AppConfig,
    group_id: &str,
) -> Result<HashSet<String>, GraphError> {
    check_id(group_id)?;
    let path = format!("/groups/{group_id}/transitiveMembers/microsoft.graph.user");
    let items = list_all(st, app, &path, &[("$select", "id"), ("$top", "999")], true).await?;
    Ok(ids(&items))
}

/// Every group the user belongs to, including through nesting.
pub async fn user_group_ids(
    st: &AppState,
    app: &AppConfig,
    oid: &str,
) -> Result<HashSet<String>, GraphError> {
    check_id(oid)?;
    let path = format!("/users/{oid}/transitiveMemberOf/microsoft.graph.group");
    let items = list_all(st, app, &path, &[("$select", "id"), ("$top", "999")], true).await?;
    Ok(ids(&items))
}

/// `accountEnabled` per user; users Graph does not return are absent (deleted).
pub async fn accounts_enabled(
    st: &AppState,
    app: &AppConfig,
    oids: &[String],
) -> Result<HashMap<String, bool>, GraphError> {
    let mut out = HashMap::new();
    for chunk in oids.chunks(15) {
        let list = chunk
            .iter()
            .map(|o| format!("'{}'", o.replace('\'', "")))
            .collect::<Vec<_>>()
            .join(",");
        let filter = format!("id in ({list})");
        let items = list_all(
            st,
            app,
            "/users",
            &[
                ("$filter", filter.as_str()),
                ("$select", "id,accountEnabled"),
            ],
            false,
        )
        .await?;
        for u in items {
            if let Some(id) = u["id"].as_str() {
                out.insert(
                    id.to_string(),
                    u["accountEnabled"].as_bool().unwrap_or(true),
                );
            }
        }
    }
    Ok(out)
}
