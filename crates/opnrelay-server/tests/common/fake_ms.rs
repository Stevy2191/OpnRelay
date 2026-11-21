// SPDX-License-Identifier: AGPL-3.0-only
//! A fake Microsoft identity platform + Graph (sign-in, OneDrive, directory).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::Engine;
use reqwest::redirect::Policy;
use serde_json::{Value, json};

pub const TENANT: &str = "11111111-1111-1111-1111-111111111111";
pub const APP_TOKEN: &str = "app-token";
pub const CLIENT: &str = "22222222-2222-2222-2222-222222222222";

#[derive(Default)]
pub struct Fake {
    pub base: String,
    /// code -> (nonce, oid, email, tenant)
    pub codes: HashMap<String, (String, String, String, String)>,
    /// Who the next authorize request signs in as: (oid, email, tenant).
    pub next_user: (String, String, String),
    pub uploads: HashMap<String, (String, Vec<u8>)>,
    pub files: Vec<(String, String, Vec<u8>)>, // (id, name, bytes)
    pub n: u64,
    /// Directory: group id -> group.
    pub groups: HashMap<String, FakeGroup>,
    /// Directory: user oid -> accountEnabled.
    pub users: HashMap<String, bool>,
    /// Every Graph directory call returns 503.
    pub graph_fail: bool,
    /// Only `GET /v1.0/users` returns 503.
    pub users_fail: bool,
    /// Only `transitiveMembers` listings return 503.
    pub members_fail: bool,
    /// The next Graph directory call returns 429 with `Retry-After: 1`.
    pub throttle_once: bool,
    /// Group ids that answer 404 (deleted in the directory).
    pub deleted_groups: HashSet<String>,
    /// Items per page for directory listings.
    pub page_size: usize,
    /// Number of `client_credentials` token requests served.
    pub app_token_requests: u32,
}

pub struct FakeGroup {
    pub name: String,
    /// "security" or "m365".
    pub kind: &'static str,
    pub members: Vec<String>,
    pub nested: Vec<String>,
}

/// Set who the next authorize request signs in as (in the configured tenant).
pub fn next_user(s: &S, oid: &str, email: &str) {
    s.lock().unwrap().next_user = (oid.into(), email.into(), TENANT.into());
}

pub type S = Arc<Mutex<Fake>>;

async fn authorize(
    State(s): State<S>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let mut f = s.lock().unwrap();
    f.n += 1;
    let code = format!("code{}", f.n);
    let (oid, email, tid) = f.next_user.clone();
    // Whoever signs in exists in the directory (tests may later disable or remove them).
    f.users.entry(oid.clone()).or_insert(true);
    f.codes.insert(
        code.clone(),
        (q.get("nonce").cloned().unwrap_or_default(), oid, email, tid),
    );
    Redirect::to(&format!(
        "{}?code={code}&state={}",
        q["redirect_uri"], q["state"]
    ))
}

async fn token(State(s): State<S>, Form(f): Form<HashMap<String, String>>) -> impl IntoResponse {
    let mut st = s.lock().unwrap();
    // App-only tokens accept any credentials so tests can rotate them.
    if f["grant_type"] != "client_credentials" {
        assert_eq!(f["client_id"], CLIENT);
        assert_eq!(f["client_secret"], "s3cret");
    }
    match f["grant_type"].as_str() {
        "authorization_code" => {
            assert!(!f["code_verifier"].is_empty(), "PKCE verifier must be sent");
            let Some((nonce, oid, email, tid)) = st.codes.get(&f["code"]).cloned() else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_grant"})),
                )
                    .into_response();
            };
            let claims = json!({"iss": format!("{}/{tid}/v2.0", st.base), "tid": tid, "aud": CLIENT,
                "exp": 4_000_000_000i64, "nonce": nonce, "oid": oid,
                "preferred_username": email, "name": "Morgan Medic"});
            let jwt = format!(
                "e30.{}.x",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
            );
            Json(json!({"access_token": "at", "id_token": jwt, "refresh_token": "rt1"}))
                .into_response()
        }
        "refresh_token" => {
            Json(json!({"access_token": "at2", "refresh_token": "rt2"})).into_response()
        }
        "client_credentials" => {
            st.app_token_requests += 1;
            Json(json!({"access_token": APP_TOKEN, "expires_in": 3599})).into_response()
        }
        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}

async fn me() -> Json<Value> {
    Json(json!({"userPrincipalName": "backup@acme.test"}))
}

async fn drive(
    State(s): State<S>,
    Path(rest): Path<String>,
    method: axum::http::Method,
    headers: HeaderMap,
) -> impl IntoResponse {
    assert!(
        headers.get("authorization").is_some(),
        "Graph calls need a token"
    );
    let mut f = s.lock().unwrap();
    if method == axum::http::Method::POST && rest.ends_with(":/createUploadSession") {
        // root:/{folder}/{name}:/createUploadSession
        let path = rest
            .trim_start_matches("root:/")
            .trim_end_matches(":/createUploadSession");
        let name = path.rsplit('/').next().unwrap().to_string();
        f.n += 1;
        let id = format!("up{}", f.n);
        f.uploads.insert(id.clone(), (name, Vec::new()));
        return Json(json!({"uploadUrl": format!("{}/upload/{id}", f.base)})).into_response();
    }
    if method == axum::http::Method::GET && rest.ends_with(":/children") {
        let v: Vec<Value> = f
            .files
            .iter()
            .map(|(id, name, _)| json!({"id": id, "name": name}))
            .collect();
        return Json(json!({ "value": v })).into_response();
    }
    if method == axum::http::Method::DELETE && rest.starts_with("items/") {
        let id = rest.trim_start_matches("items/").to_string();
        f.files.retain(|(i, _, _)| i != &id);
        return StatusCode::NO_CONTENT.into_response();
    }
    StatusCode::NOT_FOUND.into_response()
}

async fn upload(
    State(s): State<S>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let mut f = s.lock().unwrap();
    let range = headers["content-range"].to_str().unwrap().to_string(); // bytes a-b/total
    let total: usize = range.rsplit('/').next().unwrap().parse().unwrap();
    let done = {
        let (_, data) = f.uploads.get_mut(&id).unwrap();
        data.extend_from_slice(&body);
        data.len() == total
    };
    if done {
        let (name, data) = f.uploads.remove(&id).unwrap();
        f.n += 1;
        let fid = format!("file{}", f.n);
        f.files.push((fid, name, data));
        return StatusCode::CREATED.into_response();
    }
    StatusCode::ACCEPTED.into_response()
}

/// Shared checks for Graph directory calls: bearer, outage, throttling.
fn dir_guard(f: &mut Fake, headers: &HeaderMap) -> Option<axum::response::Response> {
    let ok = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == format!("Bearer {APP_TOKEN}"));
    if !ok {
        return Some(StatusCode::UNAUTHORIZED.into_response());
    }
    if f.graph_fail {
        return Some(StatusCode::SERVICE_UNAVAILABLE.into_response());
    }
    if f.throttle_once {
        f.throttle_once = false;
        return Some(
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", "1")],
                Json(json!({"error": {"code": "TooManyRequests"}})),
            )
                .into_response(),
        );
    }
    None
}

fn group_json(id: &str, g: &FakeGroup) -> Value {
    json!({
        "id": id,
        "displayName": g.name,
        "description": format!("{} group", g.name),
        "groupTypes": if g.kind == "m365" { json!(["Unified"]) } else { json!([]) },
        "onPremisesSyncEnabled": Value::Null,
    })
}

/// Transitive user members of `id` (nested groups resolved, cycles ignored).
fn transitive_members(f: &Fake, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen_groups = HashSet::new();
    let mut stack = vec![id.to_string()];
    while let Some(g) = stack.pop() {
        if !seen_groups.insert(g.clone()) {
            continue;
        }
        let Some(group) = f.groups.get(&g) else {
            continue;
        };
        for m in &group.members {
            if !out.contains(m) {
                out.push(m.clone());
            }
        }
        stack.extend(group.nested.iter().rev().cloned());
    }
    out
}

/// One page of `items` following `$skiptoken` (an offset), with `@odata.nextLink`.
fn page(f: &Fake, path: &str, q: &HashMap<String, String>, items: Vec<Value>) -> Json<Value> {
    let start: usize = q
        .get("$skiptoken")
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);
    let size = f.page_size.max(1);
    let end = (start + size).min(items.len());
    let mut body = json!({ "value": items[start.min(end)..end] });
    if end < items.len() {
        // Like real Graph, keep the original query ($filter, $search, ...).
        let mut params: Vec<(&String, &String)> =
            q.iter().filter(|(k, _)| *k != "$skiptoken").collect();
        params.sort();
        let end_s = end.to_string();
        let mut params: Vec<(&str, &str)> = params
            .into_iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        params.push(("$skiptoken", &end_s));
        let url = reqwest::Url::parse_with_params(&format!("{}{path}", f.base), params).unwrap();
        body["@odata.nextLink"] = json!(url.to_string());
    }
    Json(body)
}

async fn groups_search(
    State(s): State<S>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let mut f = s.lock().unwrap();
    if let Some(r) = dir_guard(&mut f, &headers) {
        return r;
    }
    if headers
        .get("consistencylevel")
        .and_then(|v| v.to_str().ok())
        != Some("eventual")
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let term = q
        .get("$search")
        .map(|v| {
            v.trim_matches('"')
                .trim_start_matches("displayName:")
                .to_lowercase()
        })
        .unwrap_or_default();
    let mut ids: Vec<&String> = f
        .groups
        .keys()
        .filter(|id| !f.deleted_groups.contains(*id))
        .filter(|id| f.groups[*id].name.to_lowercase().contains(&term))
        .collect();
    ids.sort();
    let items = ids
        .iter()
        .map(|id| group_json(id, &f.groups[*id]))
        .collect();
    page(&f, "/v1.0/groups", &q, items).into_response()
}

async fn group_get(
    State(s): State<S>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let mut f = s.lock().unwrap();
    if let Some(r) = dir_guard(&mut f, &headers) {
        return r;
    }
    match f.groups.get(&id) {
        Some(g) if !f.deleted_groups.contains(&id) => Json(group_json(&id, g)).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn group_members(
    State(s): State<S>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let mut f = s.lock().unwrap();
    if let Some(r) = dir_guard(&mut f, &headers) {
        return r;
    }
    if f.members_fail {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if f.deleted_groups.contains(&id) || !f.groups.contains_key(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let items = transitive_members(&f, &id)
        .into_iter()
        .map(|u| json!({"@odata.type": "#microsoft.graph.user", "id": u}))
        .collect();
    let path = format!("/v1.0/groups/{id}/transitiveMembers/microsoft.graph.user");
    page(&f, &path, &q, items).into_response()
}

async fn user_member_of(
    State(s): State<S>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let mut f = s.lock().unwrap();
    if let Some(r) = dir_guard(&mut f, &headers) {
        return r;
    }
    let mut ids: Vec<&String> = f
        .groups
        .keys()
        .filter(|g| !f.deleted_groups.contains(*g))
        .filter(|g| transitive_members(&f, g).contains(&id))
        .collect();
    ids.sort();
    let items = ids.iter().map(|g| group_json(g, &f.groups[*g])).collect();
    let path = format!("/v1.0/users/{id}/transitiveMemberOf/microsoft.graph.group");
    page(&f, &path, &q, items).into_response()
}

async fn users_filter(
    State(s): State<S>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let mut f = s.lock().unwrap();
    if let Some(r) = dir_guard(&mut f, &headers) {
        return r;
    }
    if f.users_fail {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    // $filter=id in ('a','b')
    let filter = q.get("$filter").cloned().unwrap_or_default();
    let ids: Vec<&str> = filter.split('\'').skip(1).step_by(2).collect();
    let items: Vec<Value> = ids
        .iter()
        .filter_map(|id| {
            f.users
                .get(*id)
                .map(|en| json!({"id": id, "accountEnabled": en}))
        })
        .collect();
    page(&f, "/v1.0/users", &q, items).into_response()
}

pub async fn fake_microsoft() -> (String, S) {
    let s: S = Arc::new(Mutex::new(Fake {
        page_size: 2,
        ..Fake::default()
    }));
    let app = Router::new()
        .route("/{tenant}/oauth2/v2.0/authorize", get(authorize))
        .route("/{tenant}/oauth2/v2.0/token", post(token))
        .route("/v1.0/me", get(me))
        .route("/v1.0/groups", get(groups_search))
        .route("/v1.0/groups/{id}", get(group_get))
        .route(
            "/v1.0/groups/{id}/transitiveMembers/microsoft.graph.user",
            get(group_members),
        )
        .route(
            "/v1.0/users/{id}/transitiveMemberOf/microsoft.graph.group",
            get(user_member_of),
        )
        .route("/v1.0/users", get(users_filter))
        .route("/v1.0/me/drive/{*rest}", axum::routing::any(drive))
        .route("/upload/{id}", put(upload))
        .with_state(s.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    s.lock().unwrap().base = base.clone();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (base, s)
}

pub fn browser() -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .redirect(Policy::none())
        .build()
        .unwrap()
}

pub fn location(r: &reqwest::Response) -> String {
    r.headers()["location"].to_str().unwrap().to_string()
}

/// Walk start -> Microsoft authorize -> callback in one browser.
pub async fn microsoft_flow(b: &reqwest::Client, start: &str) -> reqwest::Response {
    let r = b.get(start).send().await.unwrap();
    assert!(
        r.status().is_redirection(),
        "start must redirect, got {}",
        r.status()
    );
    let ms = location(&r);
    assert!(ms.contains("code_challenge_method=S256"));
    let r = b.get(&ms).send().await.unwrap();
    let cb = location(&r);
    b.get(&cb).send().await.unwrap()
}
