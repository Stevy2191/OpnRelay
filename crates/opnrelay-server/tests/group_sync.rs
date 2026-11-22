// SPDX-License-Identifier: AGPL-3.0-only
//! Entra group sync, against the shared fake Microsoft (see `common/fake_ms.rs`).

mod common;

use std::collections::HashSet;

use common::fake_ms::{FakeGroup, TENANT, fake_microsoft};
use serde_json::Value;

async fn app_token(base: &str) -> String {
    let r: Value = reqwest::Client::new()
        .post(format!("{base}/{TENANT}/oauth2/v2.0/token"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", common::fake_ms::CLIENT),
            ("client_secret", "s3cret"),
            ("scope", "https://graph.microsoft.com/.default"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    r["access_token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn fake_directory_pages_and_nests() {
    let (base, fake) = fake_microsoft().await;
    {
        let mut f = fake.lock().unwrap();
        f.page_size = 2;
        f.groups.insert(
            "G".into(),
            FakeGroup {
                name: "Medics".into(),
                kind: "security",
                members: vec!["u1".into(), "u2".into(), "u3".into()],
                nested: vec!["N".into()],
            },
        );
        f.groups.insert(
            "N".into(),
            FakeGroup {
                name: "Nested".into(),
                kind: "m365",
                members: vec!["u4".into()],
                nested: vec![],
            },
        );
    }
    let token = app_token(&base).await;
    assert_eq!(fake.lock().unwrap().app_token_requests, 1);
    let http = reqwest::Client::new();

    let mut url = format!("{base}/v1.0/groups/G/transitiveMembers/microsoft.graph.user");
    let mut seen = HashSet::new();
    let mut pages = 0;
    loop {
        let r = http.get(&url).bearer_auth(&token).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let body: Value = r.json().await.unwrap();
        pages += 1;
        for u in body["value"].as_array().unwrap() {
            seen.insert(u["id"].as_str().unwrap().to_string());
        }
        match body["@odata.nextLink"].as_str() {
            Some(n) => url = n.to_string(),
            None => break,
        }
    }
    let want: HashSet<String> = ["u1", "u2", "u3", "u4"].map(String::from).into();
    assert_eq!(seen, want);
    assert_eq!(pages, 2);

    let r = http
        .get(format!(
            "{base}/v1.0/groups/G/transitiveMembers/microsoft.graph.user"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}

/// Follow `@odata.nextLink` from `url`, returning the items of every page.
async fn all_pages(url: String, token: &str, search: bool) -> (Vec<Value>, usize) {
    let http = reqwest::Client::new();
    let (mut url, mut items, mut pages) = (url, Vec::new(), 0);
    loop {
        let mut req = http.get(&url).bearer_auth(token);
        if search {
            req = req.header("ConsistencyLevel", "eventual");
        }
        let r = req.send().await.unwrap();
        assert_eq!(r.status(), 200);
        let body: Value = r.json().await.unwrap();
        pages += 1;
        items.extend(body["value"].as_array().unwrap().iter().cloned());
        match body["@odata.nextLink"].as_str() {
            Some(n) => url = n.to_string(),
            None => return (items, pages),
        }
    }
}

#[tokio::test]
async fn fake_directory_next_link_keeps_query() {
    let (base, fake) = fake_microsoft().await;
    {
        let mut f = fake.lock().unwrap();
        f.page_size = 2;
        for u in ["a", "b", "c", "d"] {
            f.users.insert(u.into(), true);
        }
        for (id, name) in [
            ("g1", "Medics A"),
            ("g2", "Other"),
            ("g3", "Medics B"),
            ("g4", "Medics C"),
        ] {
            f.groups.insert(
                id.into(),
                FakeGroup {
                    name: name.into(),
                    kind: "security",
                    members: vec![],
                    nested: vec![],
                },
            );
        }
    }
    let token = app_token(&base).await;

    let filter = "id in ('a','b','c','missing','d')";
    let url = reqwest::Url::parse_with_params(
        &format!("{base}/v1.0/users"),
        [("$filter", filter), ("$select", "id,accountEnabled")],
    )
    .unwrap();
    let (items, pages) = all_pages(url.to_string(), &token, false).await;
    let ids: HashSet<&str> = items.iter().map(|u| u["id"].as_str().unwrap()).collect();
    assert_eq!(ids, HashSet::from(["a", "b", "c", "d"]));
    assert_eq!(pages, 2);

    let url = reqwest::Url::parse_with_params(
        &format!("{base}/v1.0/groups"),
        [("$search", "\"displayName:medics\"")],
    )
    .unwrap();
    let (items, pages) = all_pages(url.to_string(), &token, true).await;
    let ids: HashSet<&str> = items.iter().map(|g| g["id"].as_str().unwrap()).collect();
    assert_eq!(ids, HashSet::from(["g1", "g3", "g4"]));
    assert_eq!(pages, 2);
}

use common::TestApp;
use opnrelay_server::graph::{self, GraphError};
use opnrelay_server::microsoft::AppConfig;

fn app_cfg() -> AppConfig {
    AppConfig {
        tenant_id: TENANT.into(),
        client_id: common::fake_ms::CLIENT.into(),
        client_secret: "s3cret".into(),
    }
}

fn fg(name: &str, kind: &'static str, members: &[&str], nested: &[&str]) -> FakeGroup {
    FakeGroup {
        name: name.into(),
        kind,
        members: members.iter().map(|s| s.to_string()).collect(),
        nested: nested.iter().map(|s| s.to_string()).collect(),
    }
}

const GH: &str = "00000000-0000-0000-0000-0000000000a1";
const GG: &str = "00000000-0000-0000-0000-0000000000a2";
const GN: &str = "00000000-0000-0000-0000-0000000000a3";
const GD: &str = "00000000-0000-0000-0000-0000000000a4";
const U4: &str = "00000000-0000-0000-0000-0000000000b4";

#[tokio::test]
async fn graph_reads_directory() {
    let (base, fake) = fake_microsoft().await;
    let app = TestApp::spawn_with(Some(base), None).await;
    let st = app.state();
    let cfg = app_cfg();
    {
        let mut f = fake.lock().unwrap();
        f.groups
            .insert(GH.into(), fg("HR Staff", "security", &["u1"], &[]));
        f.groups.insert(
            GG.into(),
            fg("Medics", "security", &["u1", "u2", "u3"], &[GN]),
        );
        f.groups.insert(GN.into(), fg("Nested", "m365", &[U4], &[]));
        f.groups.insert(GD.into(), fg("Dead", "security", &[], &[]));
        f.deleted_groups.insert(GD.into());
        f.users.insert("u1".into(), true);
    }
    let found = graph::search_groups(&st, &cfg, "hr").await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].display_name, "HR Staff");
    assert_eq!(found[0].kind, "Security group");
    assert!(!found[0].on_prem);
    let m = graph::search_groups(&st, &cfg, "nest").await.unwrap();
    assert_eq!(m[0].kind, "Microsoft 365 group");
    assert_eq!(
        graph::get_group(&st, &cfg, GG).await.unwrap().display_name,
        "Medics"
    );

    let want: HashSet<String> = ["u1", "u2", "u3", U4].map(String::from).into();
    assert_eq!(graph::group_user_ids(&st, &cfg, GG).await.unwrap(), want);
    let groups = graph::user_group_ids(&st, &cfg, U4).await.unwrap();
    assert!(groups.contains(GG) && groups.contains(GN));

    let en = graph::accounts_enabled(&st, &cfg, &["u1".into(), "gone".into()])
        .await
        .unwrap();
    assert_eq!(
        en,
        std::collections::HashMap::from([("u1".to_string(), true)])
    );

    assert!(matches!(
        graph::get_group(&st, &cfg, GD).await,
        Err(GraphError::NotFound)
    ));
    app.cleanup().await;
}

#[tokio::test]
async fn graph_caches_token_and_retries() {
    let (base, fake) = fake_microsoft().await;
    let app = TestApp::spawn_with(Some(base), None).await;
    let st = app.state();
    let cfg = app_cfg();
    fake.lock()
        .unwrap()
        .groups
        .insert(GH.into(), fg("HR Staff", "security", &[], &[]));
    graph::search_groups(&st, &cfg, "hr").await.unwrap();
    graph::search_groups(&st, &cfg, "hr").await.unwrap();
    assert_eq!(fake.lock().unwrap().app_token_requests, 1);

    fake.lock().unwrap().throttle_once = true;
    graph::search_groups(&st, &cfg, "hr").await.unwrap();

    fake.lock().unwrap().graph_fail = true;
    assert!(matches!(
        graph::search_groups(&st, &cfg, "hr").await,
        Err(GraphError::Failed(_))
    ));
    app.cleanup().await;
}

#[test]
fn tenant_check() {
    for t in ["organizations", "common", "consumers"] {
        assert!(!graph::tenant_is_specific(t), "{t}");
    }
    assert!(graph::tenant_is_specific(TENANT));
    assert!(graph::tenant_is_specific("contoso.onmicrosoft.com"));
}

// ---- sync settings, search and linking ----

use common::fake_ms::{CLIENT, browser, microsoft_flow, next_user};
use common::{Session, TestApp as App};
use reqwest::StatusCode;
use serde_json::json;

const HR: &str = "00000000-0000-0000-0000-0000000000c1";

async fn sso(owner: &Session, tenant: &str, sync: bool) -> reqwest::Response {
    owner
        .put(
            "/admin/sso",
            json!({"enabled": true, "tenant_id": tenant, "client_id": CLIENT, "client_secret": "s3cret",
                "jit_provisioning": true, "default_role": "member", "sso_only": false,
                "group_sync_enabled": sync}),
        )
        .await
}

/// Sign in `oid`/`email` through the fake Microsoft; returns the OpnRelay user id.
async fn sign_in(app: &App, fake: &common::fake_ms::S, oid: &str, email: &str) -> String {
    next_user(fake, oid, email);
    let b = browser();
    microsoft_flow(&b, &format!("{}/api/v1/auth/sso/microsoft/start", app.base)).await;
    let me: Value = b
        .get(format!("{}/api/v1/me", app.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    me["id"].as_str().unwrap().to_string()
}

async fn actions(owner: &Session, prefix: &str) -> Vec<String> {
    let v: Value = owner
        .get(&format!("/audit/events?action={prefix}"))
        .await
        .json()
        .await
        .unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn enable_sync_requires_specific_tenant() {
    let (base, _fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    // "organizations" is rejected outright (valid_tenant); a domain is specific enough.
    let r = sso(&owner, "organizations", true).await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(r.text().await.unwrap().contains("specific tenant"));
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    app.cleanup().await;
}

#[tokio::test]
async fn search_and_link() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    // Sync off: search is refused.
    assert_eq!(sso(&owner, TENANT, false).await.status(), 204);
    assert_eq!(
        owner.get("/admin/sso/groups/search?q=hr").await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let s: Value = owner.get("/admin/sso").await.json().await.unwrap();
    assert_eq!(s["group_sync_enabled"], true);
    assert!(s["group_sync_last_at"].is_null());
    assert!(actions(&owner, "idp.sync.settings_changed").await.len() == 1);

    let u1 = sign_in(&app, &fake, "oid-ann@acme.test", "ann@acme.test").await;
    let u2 = sign_in(&app, &fake, "oid-bob@acme.test", "bob@acme.test").await;
    fake.lock().unwrap().groups.insert(
        HR.into(),
        fg(
            "HR Staff",
            "security",
            &["oid-ann@acme.test", "oid-bob@acme.test", "oid-nobody"],
            &[],
        ),
    );

    let found: Value = owner
        .get("/admin/sso/groups/search?q=hr")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(found[0]["display_name"], "HR Staff");
    assert_eq!(found[0]["id"], HR);
    assert_eq!(found[0]["linked"], false);
    assert_eq!(found[0]["kind"], "Security group");

    let r = owner
        .post("/admin/groups/link", json!({"external_id": HR}))
        .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let g: Value = r.json().await.unwrap();
    assert_eq!(g["source"], "idp");
    assert_eq!(g["name"], "HR Staff");
    assert_eq!(g["external_id"], HR);
    let mut members: Vec<String> = g["member_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    members.sort();
    let mut want = vec![u1, u2];
    want.sort();
    assert_eq!(members, want);

    let found: Value = owner
        .get("/admin/sso/groups/search?q=hr")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(found[0]["linked"], true);
    assert_eq!(
        owner
            .post("/admin/groups/link", json!({"external_id": HR}))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let loud = HR.replace('-', "").to_uppercase();
    assert_eq!(
        owner
            .post("/admin/groups/link", json!({"external_id": loud}))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        owner
            .post("/admin/groups/link", json!({"external_id": "not-a-guid"}))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(actions(&owner, "idp.group.linked").await.len(), 1);
    assert_eq!(actions(&owner, "group.member.added").await.len(), 2);
    app.cleanup().await;
}

#[tokio::test]
async fn link_name_clash() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    fake.lock()
        .unwrap()
        .groups
        .insert(HR.into(), fg("HR Staff", "security", &[], &[]));
    let manual: Value = owner
        .post("/admin/groups", json!({"name": "HR Staff"}))
        .await
        .json()
        .await
        .unwrap();
    let r = owner
        .post("/admin/groups/link", json!({"external_id": HR}))
        .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let g: Value = r.json().await.unwrap();
    assert_eq!(g["name"], "HR Staff (Microsoft)");
    let list: Value = owner.get("/admin/groups").await.json().await.unwrap();
    let m = list
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == manual["id"])
        .unwrap();
    assert_eq!(m["name"], "HR Staff");
    assert_eq!(m["source"], "manual");
    app.cleanup().await;
}

#[tokio::test]
async fn idp_members_read_only_and_unlink_audited() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    fake.lock()
        .unwrap()
        .groups
        .insert(HR.into(), fg("HR Staff", "security", &[], &[]));
    let g: Value = owner
        .post("/admin/groups/link", json!({"external_id": HR}))
        .await
        .json()
        .await
        .unwrap();
    let id = g["id"].as_str().unwrap();
    let r = owner
        .put(
            &format!("/admin/groups/{id}/members"),
            json!({"user_ids": []}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(
        r.text()
            .await
            .unwrap()
            .contains("Members of Microsoft groups come from Microsoft")
    );

    assert_eq!(
        owner.delete(&format!("/admin/groups/{id}")).await.status(),
        StatusCode::NO_CONTENT
    );
    let list: Value = owner.get("/admin/groups").await.json().await.unwrap();
    assert!(list.as_array().unwrap().iter().all(|x| x["id"] != id));
    assert_eq!(actions(&owner, "idp.group.unlinked").await.len(), 1);
    assert!(actions(&owner, "group.deleted").await.is_empty());
    app.cleanup().await;
}

#[tokio::test]
async fn failed_first_sync_leaves_no_group() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    fake.lock()
        .unwrap()
        .groups
        .insert(HR.into(), fg("HR Staff", "security", &[], &[]));
    // get_group succeeds (so the group row is inserted), the member listing fails.
    fake.lock().unwrap().members_fail = true;
    let r = owner
        .post("/admin/groups/link", json!({"external_id": HR}))
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(
        fake.lock().unwrap().app_token_requests >= 1,
        "the directory was reached"
    );
    fake.lock().unwrap().members_fail = false;
    let list: Value = owner.get("/admin/groups").await.json().await.unwrap();
    assert!(list.as_array().unwrap().is_empty());
    assert!(actions(&owner, "idp.group.linked").await.is_empty());
    app.cleanup().await;
}

#[tokio::test]
async fn credential_change_drops_cached_token() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    fake.lock()
        .unwrap()
        .groups
        .insert(HR.into(), fg("HR Staff", "security", &[], &[]));
    owner.get("/admin/sso/groups/search?q=hr").await;
    owner.get("/admin/sso/groups/search?q=hr").await;
    assert_eq!(fake.lock().unwrap().app_token_requests, 1);
    // Same credentials: cache kept.
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    owner.get("/admin/sso/groups/search?q=hr").await;
    assert_eq!(fake.lock().unwrap().app_token_requests, 1);
    // Changed secret: cache dropped (the fake accepts any secret).
    let r = owner
        .put(
            "/admin/sso",
            json!({"enabled": true, "tenant_id": TENANT, "client_id": CLIENT, "client_secret": "other",
                "jit_provisioning": true, "default_role": "member", "sso_only": false,
                "group_sync_enabled": true}),
        )
        .await;
    assert_eq!(r.status(), 204);
    owner.get("/admin/sso/groups/search?q=hr").await;
    assert_eq!(fake.lock().unwrap().app_token_requests, 2);
    app.cleanup().await;
}

// ---- full sync ----

use opnrelay_server::group_sync::due;
use time::{Duration, OffsetDateTime};

#[test]
fn due_logic() {
    let t = OffsetDateTime::now_utc();
    assert!(due(None, t));
    assert!(!due(Some(t - Duration::minutes(59)), t));
    assert!(due(Some(t - Duration::minutes(61)), t));
}

async fn sync_now(owner: &Session) -> Value {
    let r = owner.post("/admin/sso/sync", json!({})).await;
    assert_eq!(r.status(), StatusCode::OK);
    r.json().await.unwrap()
}

async fn link(owner: &Session, id: &str) -> String {
    let g: Value = owner
        .post("/admin/groups/link", json!({"external_id": id}))
        .await
        .json()
        .await
        .unwrap();
    g["id"].as_str().unwrap().to_string()
}

async fn member_count(app: &App, gid: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM group_members WHERE group_id = $1::uuid")
        .bind(gid)
        .fetch_one(&app.state().db)
        .await
        .unwrap()
}

#[tokio::test]
async fn removal_takes_effect_next_request() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base.clone()), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);

    next_user(&fake, "oid-ann@acme.test", "ann@acme.test");
    let b = browser();
    microsoft_flow(&b, &format!("{}/api/v1/auth/sso/microsoft/start", app.base)).await;
    fake.lock().unwrap().groups.insert(
        HR.into(),
        fg("HR Staff", "security", &["oid-ann@acme.test"], &[]),
    );
    let gid = link(&owner, HR).await;
    let ws: Value = owner
        .post("/workspaces", json!({"name": "HR"}))
        .await
        .json()
        .await
        .unwrap();
    let ws_id = ws["id"].as_str().unwrap();
    let r = owner
        .put(
            &format!("/workspaces/{ws_id}/members"),
            json!({"members": [{"principal_type": "group", "principal_id": gid, "role": "viewer"}]}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    let guide: Value = owner
        .post("/guides", json!({"workspace_id": ws_id, "title": "Policy"}))
        .await
        .json()
        .await
        .unwrap();
    let guide_id = guide["id"].as_str().unwrap();
    let get = |p: String| {
        let b = b.clone();
        let base = app.base.clone();
        async move { b.get(format!("{base}/api/v1{p}")).send().await.unwrap() }
    };
    let list: Value = get("/workspaces".into()).await.json().await.unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "HR");
    assert_eq!(get(format!("/guides/{guide_id}")).await.status(), 200);

    fake.lock()
        .unwrap()
        .groups
        .get_mut(HR)
        .unwrap()
        .members
        .clear();
    let s = sync_now(&owner).await;
    assert_eq!(s["removed"], 1);
    assert!(s["error"].is_null());

    let list: Value = get("/workspaces".into()).await.json().await.unwrap();
    assert!(list.as_array().unwrap().is_empty());
    assert_eq!(
        get(format!("/guides/{guide_id}")).await.status(),
        StatusCode::NOT_FOUND
    );
    app.cleanup().await;
}

#[tokio::test]
async fn nested_membership_counts() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let u = sign_in(&app, &fake, "oid-nel@acme.test", "nel@acme.test").await;
    {
        let mut f = fake.lock().unwrap();
        f.groups
            .insert(GG.into(), fg("Medics", "security", &[], &[GN]));
        f.groups.insert(
            GN.into(),
            fg("Nested", "security", &["oid-nel@acme.test"], &[]),
        );
    }
    let gid = link(&owner, GG).await;
    let g: Value = owner.get("/admin/groups").await.json().await.unwrap();
    let g = g
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == gid)
        .unwrap();
    assert_eq!(g["member_ids"], json!([u]));
    let s = sync_now(&owner).await;
    assert_eq!(
        (s["added"].as_u64(), s["removed"].as_u64()),
        (Some(0), Some(0))
    );
    assert_eq!(member_count(&app, &gid).await, 1);
    app.cleanup().await;
}

async fn audit_count(app: &App) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(&app.state().db)
        .await
        .unwrap()
}

#[tokio::test]
async fn graph_failure_changes_nothing() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    sign_in(&app, &fake, "oid-ann@acme.test", "ann@acme.test").await;
    fake.lock().unwrap().groups.insert(
        HR.into(),
        fg("HR Staff", "security", &["oid-ann@acme.test"], &[]),
    );
    let gid = link(&owner, HR).await;
    assert_eq!(member_count(&app, &gid).await, 1);

    fake.lock().unwrap().graph_fail = true;
    let s = sync_now(&owner).await;
    assert!(s["error"].is_string());
    assert_eq!(member_count(&app, &gid).await, 1);
    let st: Value = owner.get("/admin/sso").await.json().await.unwrap();
    assert!(st["group_sync_last_error"].is_string());
    assert!(st["group_sync_last_at"].is_string());
    assert!(st["group_sync_last_ok_at"].is_null());
    assert_eq!(actions(&owner, "idp.sync.failed").await.len(), 1);

    let s = sync_now(&owner).await;
    assert!(s["error"].is_string());
    assert_eq!(actions(&owner, "idp.sync.failed").await.len(), 1);

    // Recovery clears the error and records the success.
    fake.lock().unwrap().graph_fail = false;
    let s = sync_now(&owner).await;
    assert!(s["error"].is_null());
    let st: Value = owner.get("/admin/sso").await.json().await.unwrap();
    assert!(st["group_sync_last_error"].is_null());
    assert!(st["group_sync_last_ok_at"].is_string());
    app.cleanup().await;
}

#[tokio::test]
async fn deleted_group_emptied() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    sign_in(&app, &fake, "oid-ann@acme.test", "ann@acme.test").await;
    fake.lock().unwrap().groups.insert(
        HR.into(),
        fg("HR Staff", "security", &["oid-ann@acme.test"], &[]),
    );
    let gid = link(&owner, HR).await;
    fake.lock().unwrap().deleted_groups.insert(HR.into());
    let s = sync_now(&owner).await;
    assert_eq!(s["missing"], 1);
    assert!(s["error"].is_null());
    let missing: bool =
        sqlx::query_scalar("SELECT missing_since IS NOT NULL FROM groups WHERE id = $1::uuid")
            .bind(&gid)
            .fetch_one(&app.state().db)
            .await
            .unwrap();
    assert!(missing);
    assert_eq!(member_count(&app, &gid).await, 0);
    // Audited once only.
    sync_now(&owner).await;
    assert_eq!(actions(&owner, "idp.group.missing").await.len(), 1);

    // It comes back.
    fake.lock().unwrap().deleted_groups.remove(HR);
    let s = sync_now(&owner).await;
    assert_eq!(
        (s["missing"].as_u64(), s["added"].as_u64()),
        (Some(0), Some(1))
    );
    let missing: bool =
        sqlx::query_scalar("SELECT missing_since IS NOT NULL FROM groups WHERE id = $1::uuid")
            .bind(&gid)
            .fetch_one(&app.state().db)
            .await
            .unwrap();
    assert!(!missing);
    app.cleanup().await;
}

#[tokio::test]
async fn noop_sync_writes_no_audit() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    sign_in(&app, &fake, "oid-ann@acme.test", "ann@acme.test").await;
    fake.lock().unwrap().groups.insert(
        HR.into(),
        fg("HR Staff", "security", &["oid-ann@acme.test"], &[]),
    );
    link(&owner, HR).await;
    let before = audit_count(&app).await;
    sync_now(&owner).await;
    sync_now(&owner).await;
    assert_eq!(audit_count(&app).await, before);
    app.cleanup().await;
}

#[tokio::test]
async fn sync_off_freezes() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    sign_in(&app, &fake, "oid-ann@acme.test", "ann@acme.test").await;
    fake.lock().unwrap().groups.insert(
        HR.into(),
        fg("HR Staff", "security", &["oid-ann@acme.test"], &[]),
    );
    let gid = link(&owner, HR).await;
    assert_eq!(sso(&owner, TENANT, false).await.status(), 204);
    fake.lock()
        .unwrap()
        .groups
        .get_mut(HR)
        .unwrap()
        .members
        .clear();
    let r = owner.post("/admin/sso/sync", json!({})).await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(r.text().await.unwrap().contains("Group sync is off"));
    assert_eq!(member_count(&app, &gid).await, 1);
    app.cleanup().await;
}

#[tokio::test]
async fn cancelled_sync_does_not_wedge_the_lock() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    fake.lock()
        .unwrap()
        .groups
        .insert(HR.into(), fg("HR Staff", "security", &[], &[]));
    link(&owner, HR).await;

    // The throttled Graph call makes the run take about a second; cancel the caller meanwhile.
    fake.lock().unwrap().throttle_once = true;
    let st = app.state();
    let h = tokio::spawn(async move {
        opnrelay_server::group_sync::sync_all(
            &st,
            &opnrelay_server::audit::AuditActor::system(),
            false,
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    h.abort();

    // Once the detached run finishes, a new sync works and the lock is free.
    let mut last = Value::Null;
    for _ in 0..40 {
        last = sync_now(&owner).await;
        if last["error"].is_null() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(last["error"].is_null(), "{last}");
    assert_eq!(last["groups"], 1);
    // No session may still hold the sync lock (bigint key 0x6f706e72 -> classid 0, objid = the key).
    let held: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_locks
         WHERE locktype = 'advisory' AND classid = 0 AND objid = 1869639282 AND granted
           AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
    )
    .fetch_one(&app.state().db)
    .await
    .unwrap();
    assert_eq!(held, 0, "the advisory lock leaked");
    app.cleanup().await;
}

const NEW_OID: &str = "00000000-0000-0000-0000-0000000000d1";

#[tokio::test]
async fn first_sign_in_lands_in_workspace() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base.clone()), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    fake.lock()
        .unwrap()
        .groups
        .insert(HR.into(), fg("HR Staff", "security", &[NEW_OID], &[]));
    let gid = link(&owner, HR).await;
    let ws: Value = owner
        .post("/workspaces", json!({"name": "HR"}))
        .await
        .json()
        .await
        .unwrap();
    let ws_id = ws["id"].as_str().unwrap();
    let r = owner
        .put(
            &format!("/workspaces/{ws_id}/members"),
            json!({"members": [{"principal_type": "group", "principal_id": gid, "role": "viewer"}]}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);

    next_user(&fake, NEW_OID, "new@acme.test");
    let b = browser();
    microsoft_flow(&b, &format!("{}/api/v1/auth/sso/microsoft/start", app.base)).await;
    let list: Value = b
        .get(format!("{}/api/v1/workspaces", app.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "HR");
    assert_eq!(member_count(&app, &gid).await, 1);
    assert_eq!(
        actions(&owner, "group.member.added").await,
        vec!["group.member.added"]
    );
    app.cleanup().await;
}

#[tokio::test]
async fn sign_in_survives_graph_outage() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base.clone()), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    fake.lock()
        .unwrap()
        .groups
        .insert(HR.into(), fg("HR Staff", "security", &[NEW_OID], &[]));
    let gid = link(&owner, HR).await;
    next_user(&fake, NEW_OID, "new@acme.test");
    let b = browser();
    microsoft_flow(&b, &format!("{}/api/v1/auth/sso/microsoft/start", app.base)).await;
    assert_eq!(member_count(&app, &gid).await, 1);

    {
        let mut f = fake.lock().unwrap();
        f.groups.get_mut(HR).unwrap().members.clear();
        f.graph_fail = true;
    }
    let b2 = browser();
    let r = microsoft_flow(
        &b2,
        &format!("{}/api/v1/auth/sso/microsoft/start", app.base),
    )
    .await;
    assert!(r.status().is_redirection());
    assert_eq!(r.headers()["location"], "/#/");
    let me = b2
        .get(format!("{}/api/v1/me", app.base))
        .send()
        .await
        .unwrap();
    assert_eq!(me.status(), 200);
    assert_eq!(member_count(&app, &gid).await, 1);
    app.cleanup().await;
}

// ---- roles from groups ----

const IT: &str = "00000000-0000-0000-0000-0000000000e1";
const COMPLIANCE: &str = "00000000-0000-0000-0000-0000000000e2";

async fn admin_user(owner: &Session, id: &str) -> Value {
    let v: Value = owner.get("/admin/users").await.json().await.unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == id)
        .unwrap()
        .clone()
}

async fn grant(owner: &Session, gid: &str, role: Value) -> Value {
    let r = owner
        .patch(
            &format!("/admin/groups/{gid}"),
            json!({"grants_role": role}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    r.json().await.unwrap()
}

fn set_members(fake: &common::fake_ms::S, gid: &str, members: &[&str]) {
    fake.lock().unwrap().groups.get_mut(gid).unwrap().members =
        members.iter().map(|s| s.to_string()).collect();
}

#[tokio::test]
async fn role_granted_and_revoked() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let ann = sign_in(
        &app,
        &fake,
        "00000000-0000-0000-0000-0000000000f1",
        "ann@acme.test",
    )
    .await;
    fake.lock().unwrap().groups.insert(
        IT.into(),
        fg(
            "IT Admins",
            "security",
            &["00000000-0000-0000-0000-0000000000f1"],
            &[],
        ),
    );
    let gid = link(&owner, IT).await;
    let g = grant(&owner, &gid, json!("admin")).await;
    assert_eq!(g["grants_role"], "admin");
    let u = admin_user(&owner, &ann).await;
    assert_eq!(u["org_role"], "admin");
    assert_eq!(u["role_source"], "idp");
    assert_eq!(u["role_from"], "IT Admins");
    assert_eq!(
        actions(&owner, "idp.group.role_changed").await,
        vec!["idp.group.role_changed"]
    );
    assert_eq!(
        actions(&owner, "user.role.changed").await,
        vec!["user.role.changed"]
    );

    set_members(&fake, IT, &[]);
    let s = sync_now(&owner).await;
    assert_eq!(s["roles_changed"], 1);
    let u = admin_user(&owner, &ann).await;
    assert_eq!(u["org_role"], "member");
    assert_eq!(u["role_source"], "idp");
    app.cleanup().await;
}

#[tokio::test]
async fn highest_role_wins() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let ann = sign_in(
        &app,
        &fake,
        "00000000-0000-0000-0000-0000000000f1",
        "ann@acme.test",
    )
    .await;
    {
        let mut f = fake.lock().unwrap();
        f.groups.insert(
            IT.into(),
            fg(
                "IT Admins",
                "security",
                &["00000000-0000-0000-0000-0000000000f1"],
                &[],
            ),
        );
        f.groups.insert(
            COMPLIANCE.into(),
            fg(
                "Compliance",
                "security",
                &["00000000-0000-0000-0000-0000000000f1"],
                &[],
            ),
        );
    }
    let it = link(&owner, IT).await;
    let co = link(&owner, COMPLIANCE).await;
    grant(&owner, &it, json!("admin")).await;
    grant(&owner, &co, json!("auditor")).await;
    assert_eq!(admin_user(&owner, &ann).await["org_role"], "admin");
    set_members(&fake, IT, &[]);
    sync_now(&owner).await;
    let u = admin_user(&owner, &ann).await;
    assert_eq!(u["org_role"], "auditor");
    assert_eq!(u["role_from"], "Compliance");
    app.cleanup().await;
}

#[tokio::test]
async fn owner_untouched() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    sqlx::query(
        "INSERT INTO user_identities (user_id, idp_id, subject)
         SELECT u.id, ip.id, 'oid-owner' FROM users u, identity_providers ip
         WHERE u.email = 'owner@acme.test'",
    )
    .execute(&app.state().db)
    .await
    .unwrap();
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT Admins", "security", &[], &[]));
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("auditor")).await;
    sync_now(&owner).await;
    let users: Value = owner.get("/admin/users").await.json().await.unwrap();
    let o = &users.as_array().unwrap()[0];
    assert_eq!(o["email"], "owner@acme.test");
    assert_eq!(o["org_role"], "owner");
    assert_eq!(o["role_source"], "manual");
    app.cleanup().await;
}

#[tokio::test]
async fn manual_change_refused_while_managed() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let ann = sign_in(
        &app,
        &fake,
        "00000000-0000-0000-0000-0000000000f1",
        "ann@acme.test",
    )
    .await;
    fake.lock().unwrap().groups.insert(
        IT.into(),
        fg(
            "IT Admins",
            "security",
            &["00000000-0000-0000-0000-0000000000f1"],
            &[],
        ),
    );
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;
    let r = owner
        .patch(
            &format!("/admin/users/{ann}"),
            json!({"org_role": "member"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(
        r.text()
            .await
            .unwrap()
            .contains("This person's role is set by a Microsoft group")
    );

    grant(&owner, &gid, Value::Null).await;
    let u = admin_user(&owner, &ann).await;
    assert_eq!(u["role_source"], "manual");
    assert_eq!(u["org_role"], "admin");
    let r = owner
        .patch(
            &format!("/admin/users/{ann}"),
            json!({"org_role": "member"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(admin_user(&owner, &ann).await["org_role"], "member");
    app.cleanup().await;
}

#[tokio::test]
async fn role_preview_counts() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let a = sign_in(
        &app,
        &fake,
        "00000000-0000-0000-0000-0000000000f2",
        "a@acme.test",
    )
    .await;
    let b = sign_in(
        &app,
        &fake,
        "00000000-0000-0000-0000-0000000000f3",
        "b@acme.test",
    )
    .await;
    let c = sign_in(
        &app,
        &fake,
        "00000000-0000-0000-0000-0000000000f4",
        "c@acme.test",
    )
    .await;
    let r = owner
        .patch(&format!("/admin/users/{c}"), json!({"org_role": "admin"}))
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    fake.lock().unwrap().groups.insert(
        IT.into(),
        fg(
            "IT Admins",
            "security",
            &["00000000-0000-0000-0000-0000000000f2"],
            &[],
        ),
    );
    let gid = link(&owner, IT).await;
    let before = audit_count(&app).await;
    let seq = || async {
        sqlx::query_scalar::<_, i64>("SELECT last_value FROM audit_events_seq_seq")
            .fetch_one(&app.state().db)
            .await
            .unwrap()
    };
    let seq_before = seq().await;

    let r = owner
        .get(&format!(
            "/admin/groups/{gid}/role-preview?grants_role=admin"
        ))
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["changes"], 2);

    assert_eq!(audit_count(&app).await, before);
    assert_eq!(seq().await, seq_before);
    for (id, role) in [(&a, "member"), (&b, "member"), (&c, "admin")] {
        let u = admin_user(&owner, id).await;
        assert_eq!(
            (
                u["org_role"].as_str().unwrap(),
                u["role_source"].as_str().unwrap()
            ),
            (role, "manual")
        );
    }
    let groups: Value = owner.get("/admin/groups").await.json().await.unwrap();
    assert!(groups[0]["grants_role"].is_null());
    app.cleanup().await;
}

#[tokio::test]
async fn sign_in_removes_lost_membership() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let ann = sign_in(
        &app,
        &fake,
        "00000000-0000-0000-0000-0000000000f1",
        "ann@acme.test",
    )
    .await;
    fake.lock().unwrap().groups.insert(
        HR.into(),
        fg(
            "HR Staff",
            "security",
            &["00000000-0000-0000-0000-0000000000f1"],
            &[],
        ),
    );
    let gid = link(&owner, HR).await;
    assert_eq!(member_count(&app, &gid).await, 1);

    set_members(&fake, HR, &[]);
    assert_eq!(
        sign_in(
            &app,
            &fake,
            "00000000-0000-0000-0000-0000000000f1",
            "ann@acme.test"
        )
        .await,
        ann
    );
    assert_eq!(member_count(&app, &gid).await, 0);
    assert_eq!(
        actions(&owner, "group.member.removed").await,
        vec!["group.member.removed"]
    );
    app.cleanup().await;
}

#[tokio::test]
async fn owner_recovered_via_bootstrap_is_manual() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let ann = sign_in(
        &app,
        &fake,
        "00000000-0000-0000-0000-0000000000f1",
        "ann@acme.test",
    )
    .await;
    fake.lock().unwrap().groups.insert(
        IT.into(),
        fg(
            "IT Admins",
            "security",
            &["00000000-0000-0000-0000-0000000000f1"],
            &[],
        ),
    );
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;
    assert_eq!(admin_user(&owner, &ann).await["role_source"], "idp");

    opnrelay_server::bootstrap::create_owner(
        &app.state().db,
        &app.base,
        "Acme",
        "ann@acme.test",
        "Ann",
    )
    .await
    .unwrap();
    let u = admin_user(&owner, &ann).await;
    assert_eq!(
        (
            u["org_role"].as_str().unwrap(),
            u["role_source"].as_str().unwrap()
        ),
        ("owner", "manual")
    );
    sync_now(&owner).await;
    assert_eq!(admin_user(&owner, &ann).await["role_source"], "manual");
    app.cleanup().await;
}

#[tokio::test]
async fn jit_user_gets_mapped_role_during_graph_outage() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    let r = owner
        .put(
            "/admin/sso",
            json!({"enabled": true, "tenant_id": TENANT, "client_id": CLIENT, "client_secret": "s3cret",
                "jit_provisioning": true, "default_role": "auditor", "sso_only": false,
                "group_sync_enabled": true}),
        )
        .await;
    assert_eq!(r.status(), 204);
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT Admins", "security", &[], &[]));
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;

    fake.lock().unwrap().graph_fail = true;
    let u = sign_in(
        &app,
        &fake,
        "00000000-0000-0000-0000-0000000000f2",
        "new@acme.test",
    )
    .await;
    let u = admin_user(&owner, &u).await;
    assert_eq!(
        (
            u["org_role"].as_str().unwrap(),
            u["role_source"].as_str().unwrap()
        ),
        ("member", "idp")
    );
    app.cleanup().await;
}

#[tokio::test]
async fn re_enabled_user_loses_stale_role() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let oid = "00000000-0000-0000-0000-0000000000f3";
    let ann = sign_in(&app, &fake, oid, "ann@acme.test").await;
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT Admins", "security", &[oid], &[]));
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;
    assert_eq!(admin_user(&owner, &ann).await["org_role"], "admin");

    let r = owner
        .patch(
            &format!("/admin/users/{ann}"),
            json!({"status": "disabled"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    set_members(&fake, IT, &[]);
    sync_now(&owner).await;
    assert_eq!(admin_user(&owner, &ann).await["org_role"], "admin");

    let r = owner
        .patch(&format!("/admin/users/{ann}"), json!({"status": "active"}))
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(admin_user(&owner, &ann).await["org_role"], "member");
    app.cleanup().await;
}

#[tokio::test]
async fn same_role_patch_on_managed_user_is_allowed() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let oid = "00000000-0000-0000-0000-0000000000f4";
    let ann = sign_in(&app, &fake, oid, "ann@acme.test").await;
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT Admins", "security", &[oid], &[]));
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;
    let r = owner
        .patch(
            &format!("/admin/users/{ann}"),
            json!({"org_role": "admin", "display_name": "Ann A"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    let r = owner
        .patch(
            &format!("/admin/users/{ann}"),
            json!({"org_role": "member"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    app.cleanup().await;
}

#[tokio::test]
async fn user_updated_audit_records_final_role() {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    let oid = "00000000-0000-0000-0000-0000000000f5";
    let ann = sign_in(&app, &fake, oid, "ann@acme.test").await;
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT Admins", "security", &[oid], &[]));
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;
    owner
        .patch(
            &format!("/admin/users/{ann}"),
            json!({"status": "disabled"}),
        )
        .await;
    set_members(&fake, IT, &[]);
    sync_now(&owner).await;
    let r = owner
        .patch(
            &format!("/admin/users/{ann}"),
            json!({"org_role": "admin", "status": "active"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    let ev: Value = owner
        .get("/audit/events?action=user.updated")
        .await
        .json()
        .await
        .unwrap();
    let last = ev
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["metadata"]["status"] == "active")
        .expect("user.updated with status active");
    assert_eq!(last["metadata"]["org_role"], "member");
    app.cleanup().await;
}

// ---- leavers and the safety brake ----

use opnrelay_server::group_sync::brake_trips;

#[test]
fn brake_boundary() {
    assert!(!brake_trips(10, 20));
    assert!(brake_trips(11, 50));
    assert!(!brake_trips(11, 60));
    assert!(!brake_trips(0, 0));
}

/// Sign `oid` in and keep the browser, so the test can use the user's own session.
async fn sign_in_browser(
    app: &App,
    fake: &common::fake_ms::S,
    oid: &str,
    email: &str,
) -> (String, reqwest::Client) {
    next_user(fake, oid, email);
    let b = browser();
    microsoft_flow(&b, &format!("{}/api/v1/auth/sso/microsoft/start", app.base)).await;
    let me: Value = b
        .get(format!("{}/api/v1/me", app.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    (me["id"].as_str().unwrap().to_string(), b)
}

async fn me_status(app: &App, b: &reqwest::Client) -> StatusCode {
    b.get(format!("{}/api/v1/me", app.base))
        .send()
        .await
        .unwrap()
        .status()
}

async fn user_state(app: &App, id: &str) -> (String, Option<String>) {
    sqlx::query_as("SELECT status, disabled_by FROM users WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&app.state().db)
        .await
        .unwrap()
}

fn set_enabled(fake: &common::fake_ms::S, oid: &str, enabled: bool) {
    fake.lock().unwrap().users.insert(oid.into(), enabled);
}

async fn leaver_events(owner: &Session) -> Vec<Value> {
    let v: Value = owner
        .get("/audit/events?action=user.disabled")
        .await
        .json()
        .await
        .unwrap();
    v.as_array().unwrap().clone()
}

const LV: &str = "00000000-0000-0000-0000-0000000000d1";
const LV2: &str = "00000000-0000-0000-0000-0000000000d2";

async fn setup_sso() -> (App, common::fake_ms::S, Session) {
    let (base, fake) = fake_microsoft().await;
    let app = App::spawn_with(Some(base), None).await;
    let owner = app.owner().await;
    assert_eq!(sso(&owner, TENANT, true).await.status(), 204);
    (app, fake, owner)
}

#[tokio::test]
async fn leaver_disabled_and_signed_out() {
    let (app, fake, owner) = setup_sso().await;
    let (id, b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    assert_eq!(me_status(&app, &b).await, StatusCode::OK);

    // A desktop device grant for the same person.
    let csrf: Value = b
        .get(format!("{}/api/v1/me", app.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let csrf = csrf["csrf_token"].as_str().unwrap().to_string();
    let verifier = "v".repeat(50);
    let challenge = {
        use base64::Engine;
        use sha2::Digest;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(&verifier))
    };
    let redirect = "http://127.0.0.1:53682/callback";
    let auth: Value = b
        .post(format!("{}/api/v1/oauth/authorize", app.base))
        .header("x-csrf-token", &csrf)
        .json(
            &json!({"client_id": "opnrelay-desktop", "redirect_uri": redirect,
            "code_challenge": challenge, "code_challenge_method": "S256", "device_name": "Laptop"}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let code = auth["redirect_to"]
        .as_str()
        .unwrap()
        .split("code=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let anon = reqwest::Client::new();
    let tok: Value = anon
        .post(format!("{}/api/v1/oauth/token", app.base))
        .json(&json!({"grant_type": "authorization_code", "code": code,
            "code_verifier": verifier, "redirect_uri": redirect, "client_id": "opnrelay-desktop"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let access = tok["access_token"].as_str().unwrap().to_string();
    let refresh = tok["refresh_token"].as_str().unwrap().to_string();
    let bearer = |t: String| {
        let (c, u) = (anon.clone(), format!("{}/api/v1/me", app.base));
        async move { c.get(u).bearer_auth(t).send().await.unwrap().status() }
    };
    assert_eq!(bearer(access.clone()).await, StatusCode::OK);

    set_enabled(&fake, LV, false);
    let sum = sync_now(&owner).await;
    assert_eq!(sum["disabled"], 1);

    assert_eq!(me_status(&app, &b).await, StatusCode::UNAUTHORIZED);
    assert_eq!(bearer(access).await, StatusCode::UNAUTHORIZED);
    let r = anon
        .post(format!("{}/api/v1/oauth/token", app.base))
        .json(&json!({"grant_type": "refresh_token", "refresh_token": refresh}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        user_state(&app, &id).await,
        ("disabled".into(), Some("microsoft".into()))
    );
    let ev = leaver_events(&owner).await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["metadata"]["source"], "microsoft");
    assert_eq!(ev[0]["metadata"]["reason"], "disabled");

    // A second run has nothing left to do.
    assert_eq!(sync_now(&owner).await["disabled"], 0);
    app.cleanup().await;
}

#[tokio::test]
async fn deleted_user_treated_as_leaver() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    // Someone else still exists, so the directory answer is not empty.
    sign_in_browser(&app, &fake, LV2, "kim@acme.test").await;
    fake.lock().unwrap().users.remove(LV);
    assert_eq!(sync_now(&owner).await["disabled"], 1);
    assert_eq!(user_state(&app, &id).await.0, "disabled");
    let ev = leaver_events(&owner).await;
    assert_eq!(ev[0]["metadata"]["reason"], "deleted");
    app.cleanup().await;
}

#[tokio::test]
async fn leaver_leaves_linked_groups() {
    let (app, fake, owner) = setup_sso().await;
    let (_id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT", "security", &[LV], &[]));
    let gid = link(&owner, IT).await;
    assert_eq!(member_count(&app, &gid).await, 1);
    set_enabled(&fake, LV, false);
    assert_eq!(sync_now(&owner).await["disabled"], 1);
    assert_eq!(member_count(&app, &gid).await, 0);
    // Still a member in Microsoft, but a later run must not put them back.
    sync_now(&owner).await;
    assert_eq!(member_count(&app, &gid).await, 0);
    // Back in Microsoft: signing in restores the account and the membership.
    set_enabled(&fake, LV, true);
    next_user(&fake, LV, "lee@acme.test");
    let b = browser();
    microsoft_flow(&b, &format!("{}/api/v1/auth/sso/microsoft/start", app.base)).await;
    assert_eq!(member_count(&app, &gid).await, 1);
    app.cleanup().await;
}

#[tokio::test]
async fn reenable_on_sign_in_only_if_microsoft_disabled() {
    let (app, fake, owner) = setup_sso().await;
    let oid2 = "00000000-0000-0000-0000-0000000000d2";
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    let (id2, _b2) = sign_in_browser(&app, &fake, oid2, "kim@acme.test").await;

    set_enabled(&fake, LV, false);
    assert_eq!(sync_now(&owner).await["disabled"], 1);
    assert_eq!(user_state(&app, &id).await.0, "disabled");
    // Disabled by hand: Microsoft sign-in must not undo it.
    let r = owner
        .patch(
            &format!("/admin/users/{id2}"),
            json!({"status": "disabled"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        user_state(&app, &id2).await,
        ("disabled".into(), Some("admin".into()))
    );

    set_enabled(&fake, LV, true);
    next_user(&fake, LV, "lee@acme.test");
    let b = browser();
    microsoft_flow(&b, &format!("{}/api/v1/auth/sso/microsoft/start", app.base)).await;
    assert_eq!(me_status(&app, &b).await, StatusCode::OK);
    assert_eq!(user_state(&app, &id).await, ("active".into(), None));
    assert_eq!(actions(&owner, "user.enabled").await, vec!["user.enabled"]);

    next_user(&fake, oid2, "kim@acme.test");
    let b2 = browser();
    microsoft_flow(
        &b2,
        &format!("{}/api/v1/auth/sso/microsoft/start", app.base),
    )
    .await;
    assert_eq!(me_status(&app, &b2).await, StatusCode::UNAUTHORIZED);
    assert_eq!(user_state(&app, &id2).await.0, "disabled");

    // Admin re-enable clears the marker.
    let r = owner
        .patch(&format!("/admin/users/{id2}"), json!({"status": "active"}))
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(user_state(&app, &id2).await, ("active".into(), None));
    app.cleanup().await;
}

#[tokio::test]
async fn owner_never_disabled() {
    let (app, fake, owner) = setup_sso().await;
    sqlx::query(
        "INSERT INTO user_identities (user_id, idp_id, subject)
         SELECT u.id, ip.id, 'oid-owner' FROM users u, identity_providers ip
         WHERE u.email = 'owner@acme.test'",
    )
    .execute(&app.state().db)
    .await
    .unwrap();
    set_enabled(&fake, "oid-owner", false);
    let sum = sync_now(&owner).await;
    assert_eq!(sum["disabled"], 0);
    assert!(sum["error"].is_null(), "{sum}");
    assert!(sum["warnings"][0].as_str().unwrap().starts_with("Owner "));
    let (err, warn, ok): (Option<String>, Option<String>, Option<time::OffsetDateTime>) =
        sqlx::query_as(
            "SELECT group_sync_last_error, group_sync_last_warning, group_sync_last_ok_at
             FROM identity_providers LIMIT 1",
        )
        .fetch_one(&app.state().db)
        .await
        .unwrap();
    assert!(err.is_none() && ok.is_some());
    assert!(warn.unwrap().contains("is disabled in Microsoft"));
    let st: Value = owner.get("/admin/sso").await.json().await.unwrap();
    assert!(
        st["group_sync_last_warning"]
            .as_str()
            .unwrap()
            .contains("Owner ")
    );
    assert!(actions(&owner, "idp.sync.failed").await.is_empty());
    let st: String = sqlx::query_scalar("SELECT status FROM users WHERE email = 'owner@acme.test'")
        .fetch_one(&app.state().db)
        .await
        .unwrap();
    assert_eq!(st, "active");
    app.cleanup().await;
}

async fn pending(app: &App) -> Option<i32> {
    sqlx::query_scalar("SELECT group_sync_brake_pending FROM identity_providers LIMIT 1")
        .fetch_one(&app.state().db)
        .await
        .unwrap()
}

#[tokio::test]
async fn brake_blocks_mass_disable() {
    let (app, fake, owner) = setup_sso().await;
    let db = app.state().db;
    for i in 0..50 {
        let oid = format!("00000000-0000-0000-0000-{i:012}");
        sqlx::query(
            "WITH u AS (INSERT INTO users (id, org_id, email, display_name, org_role, status)
                        SELECT gen_random_uuid(), id, $1, 'x', 'member', 'active'
                        FROM organizations LIMIT 1 RETURNING id)
             INSERT INTO user_identities (user_id, idp_id, subject)
             SELECT u.id, ip.id, $2 FROM u, identity_providers ip",
        )
        .bind(format!("p{i}@acme.test"))
        .bind(&oid)
        .execute(&db)
        .await
        .unwrap();
        set_enabled(&fake, &oid, i >= 11);
    }
    let sum = sync_now(&owner).await;
    assert_eq!(sum["disabled"], 0);
    assert_eq!(sum["brake"], 11);
    assert!(
        sum["error"]
            .as_str()
            .unwrap()
            .starts_with("Sync would have disabled 11 people; nothing was changed")
    );
    assert_eq!(pending(&app).await, Some(11));
    let off: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE status = 'disabled'")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(off, 0);
    let ev: Value = owner
        .get("/audit/events?action=idp.sync.failed")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(ev[0]["metadata"]["reason"], "brake");
    assert_eq!(ev[0]["metadata"]["count"], 11);

    // Confirming applies to that one run.
    let r = owner
        .post("/admin/sso/sync", json!({"confirm_brake": true}))
        .await;
    let sum: Value = r.json().await.unwrap();
    assert_eq!(sum["disabled"], 11, "{sum}");
    assert_eq!(pending(&app).await, None);
    app.cleanup().await;
}

#[tokio::test]
async fn graph_error_disables_no_one() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    set_enabled(&fake, LV, false);
    fake.lock().unwrap().users_fail = true;
    let sum = sync_now(&owner).await;
    assert_eq!(sum["disabled"], 0);
    assert!(sum["error"].is_string());
    assert_eq!(user_state(&app, &id).await.0, "active");
    app.cleanup().await;
}

#[tokio::test]
async fn failed_group_skips_leaver_step() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT", "security", &[LV], &[]));
    link(&owner, IT).await;
    set_enabled(&fake, LV, false);
    fake.lock().unwrap().members_fail = true;
    let sum = sync_now(&owner).await;
    assert_eq!(sum["disabled"], 0);
    assert!(sum["error"].is_string());
    assert_eq!(user_state(&app, &id).await.0, "active");
    app.cleanup().await;
}

#[tokio::test]
async fn one_enabled_identity_keeps_the_person() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    // An older Entra object for the same person, since deleted.
    let old = "00000000-0000-0000-0000-0000000000d9";
    sqlx::query(
        "INSERT INTO user_identities (user_id, idp_id, subject)
         SELECT $1::uuid, id, $2 FROM identity_providers",
    )
    .bind(&id)
    .bind(old)
    .execute(&app.state().db)
    .await
    .unwrap();
    assert_eq!(sync_now(&owner).await["disabled"], 0);
    assert_eq!(user_state(&app, &id).await.0, "active");
    // Both gone or off: now a leaver.
    set_enabled(&fake, LV, false);
    assert_eq!(sync_now(&owner).await["disabled"], 1);
    app.cleanup().await;
}

#[tokio::test]
async fn upper_case_oids_match() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    sqlx::query("UPDATE user_identities SET subject = upper(subject) WHERE user_id = $1::uuid")
        .bind(&id)
        .execute(&app.state().db)
        .await
        .unwrap();
    assert_eq!(sync_now(&owner).await["disabled"], 0);
    assert_eq!(user_state(&app, &id).await.0, "active");
    app.cleanup().await;
}

#[tokio::test]
async fn email_match_with_new_oid_stays_refused() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    set_enabled(&fake, LV, false);
    assert_eq!(sync_now(&owner).await["disabled"], 1);
    let new = "00000000-0000-0000-0000-0000000000d8";
    next_user(&fake, new, "lee@acme.test");
    let b = browser();
    microsoft_flow(&b, &format!("{}/api/v1/auth/sso/microsoft/start", app.base)).await;
    assert_eq!(me_status(&app, &b).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        user_state(&app, &id).await,
        ("disabled".into(), Some("microsoft".into()))
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM user_identities WHERE subject = $1")
        .bind(new)
        .fetch_one(&app.state().db)
        .await
        .unwrap();
    assert_eq!(n, 0);
    app.cleanup().await;
}

#[tokio::test]
async fn confirmation_cannot_exceed_what_was_shown() {
    let (app, fake, owner) = setup_sso().await;
    let db = app.state().db;
    for i in 0..50 {
        let oid = format!("00000000-0000-0000-0000-{i:012}");
        sqlx::query(
            "WITH u AS (INSERT INTO users (id, org_id, email, display_name, org_role, status)
                        SELECT gen_random_uuid(), id, $1, 'x', 'member', 'active'
                        FROM organizations LIMIT 1 RETURNING id)
             INSERT INTO user_identities (user_id, idp_id, subject)
             SELECT u.id, ip.id, $2 FROM u, identity_providers ip",
        )
        .bind(format!("p{i}@acme.test"))
        .bind(&oid)
        .execute(&db)
        .await
        .unwrap();
        set_enabled(&fake, &oid, i >= 11);
    }
    assert_eq!(sync_now(&owner).await["brake"], 11);
    // One more person leaves before the admin confirms.
    set_enabled(&fake, "00000000-0000-0000-0000-000000000040", false);
    let sum: Value = owner
        .post("/admin/sso/sync", json!({"confirm_brake": true}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(sum["disabled"], 0, "{sum}");
    assert_eq!(sum["brake"], 12);
    assert_eq!(pending(&app).await, Some(12));
    let sum: Value = owner
        .post("/admin/sso/sync", json!({"confirm_brake": true}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(sum["disabled"], 12, "{sum}");
    app.cleanup().await;
}

// ---- final review fixes ----

async fn events(owner: &Session, action: &str) -> Vec<Value> {
    let v: Value = owner
        .get(&format!("/audit/events?action={action}"))
        .await
        .json()
        .await
        .unwrap();
    v.as_array().unwrap().clone()
}

async fn missing(app: &App, gid: &str) -> bool {
    sqlx::query_scalar("SELECT missing_since IS NOT NULL FROM groups WHERE id = $1::uuid")
        .bind(gid)
        .fetch_one(&app.state().db)
        .await
        .unwrap()
}

#[tokio::test]
async fn reenable_at_sign_in_recomputes_role_with_sync_off() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT Admins", "security", &[LV], &[]));
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;
    assert_eq!(admin_user(&owner, &id).await["org_role"], "admin");

    set_enabled(&fake, LV, false);
    assert_eq!(sync_now(&owner).await["disabled"], 1);
    set_members(&fake, IT, &[]);
    sync_now(&owner).await;
    assert_eq!(sso(&owner, TENANT, false).await.status(), 204);

    set_enabled(&fake, LV, true);
    let (_, b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    assert_eq!(me_status(&app, &b).await, StatusCode::OK);
    let u = admin_user(&owner, &id).await;
    assert_eq!(u["status"], "active");
    assert_eq!(u["org_role"], "member");
    app.cleanup().await;
}

#[tokio::test]
async fn empty_directory_answer_disables_no_one() {
    let (app, fake, owner) = setup_sso().await;
    let (a, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    let (b, _b2) = sign_in_browser(&app, &fake, LV2, "kim@acme.test").await;
    // Wrong tenant / wrong directory: none of the linked accounts exist there.
    fake.lock().unwrap().users.clear();
    let sum = sync_now(&owner).await;
    assert_eq!(sum["disabled"], 0);
    let err = sum["error"].as_str().unwrap();
    assert!(err.contains("check the tenant"), "{err}");
    assert_eq!(user_state(&app, &a).await.0, "active");
    assert_eq!(user_state(&app, &b).await.0, "active");
    app.cleanup().await;
}

#[tokio::test]
async fn every_group_missing_is_a_failure() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    {
        let mut f = fake.lock().unwrap();
        f.groups
            .insert(HR.into(), fg("HR Staff", "security", &[LV], &[]));
        f.groups.insert(IT.into(), fg("IT", "security", &[LV], &[]));
    }
    let hr = link(&owner, HR).await;
    let it = link(&owner, IT).await;
    {
        let mut f = fake.lock().unwrap();
        f.deleted_groups.insert(HR.into());
        f.deleted_groups.insert(IT.into());
    }
    set_enabled(&fake, LV, false);
    let sum = sync_now(&owner).await;
    assert!(sum["error"].is_string(), "{sum}");
    assert_eq!(sum["disabled"], 0);
    for g in [&hr, &it] {
        assert_eq!(member_count(&app, g).await, 1);
        assert!(!missing(&app, g).await);
    }
    assert_eq!(user_state(&app, &id).await.0, "active");
    assert!(actions(&owner, "idp.group.missing").await.is_empty());
    app.cleanup().await;
}

#[tokio::test]
async fn members_404_is_a_failure_not_a_deletion() {
    let (app, fake, owner) = setup_sso().await;
    sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    fake.lock()
        .unwrap()
        .groups
        .insert(HR.into(), fg("HR Staff", "security", &[LV], &[]));
    let gid = link(&owner, HR).await;
    fake.lock().unwrap().members_missing = true;
    let sum = sync_now(&owner).await;
    assert!(sum["error"].is_string(), "{sum}");
    assert_eq!(sum["missing"], 0);
    assert_eq!(member_count(&app, &gid).await, 1);
    assert!(!missing(&app, &gid).await);
    app.cleanup().await;
}

#[tokio::test]
async fn sign_in_sync_audited_as_microsoft_sync() {
    let (app, fake, owner) = setup_sso().await;
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT Admins", "security", &[LV], &[]));
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    for action in ["group.member.added", "user.role.changed"] {
        let ev = events(&owner, action).await;
        assert_eq!(ev.len(), 1, "{action}");
        assert_eq!(ev[0]["actor_label"], "Microsoft sync", "{action}");
        assert!(ev[0]["actor_id"].is_null(), "{action}");
    }
    assert_eq!(
        events(&owner, "group.member.added").await[0]["metadata"]["user_id"],
        id
    );
    assert_eq!(
        events(&owner, "user.role.changed").await[0]["target_id"],
        id
    );

    // The Graph-outage path (role recompute only) too.
    set_members(&fake, IT, &[]);
    sync_now(&owner).await;
    set_members(&fake, IT, &[LV]);
    fake.lock().unwrap().graph_fail = true;
    // Put the membership back by hand so the recompute has something to do.
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1::uuid, $2::uuid)")
        .bind(&gid)
        .bind(&id)
        .execute(&app.state().db)
        .await
        .unwrap();
    sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    let ev = events(&owner, "user.role.changed").await;
    // Newest first: the sign-in recompute, not the owner's "Sync now".
    assert_eq!(ev.len(), 3);
    assert_eq!(ev[0]["metadata"]["to"], "admin");
    assert_eq!(ev[0]["actor_label"], "Microsoft sync");
    assert!(ev[0]["actor_id"].is_null());
    app.cleanup().await;
}

async fn xmin(app: &App, id: &str) -> String {
    sqlx::query_scalar("SELECT xmin::text FROM users WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&app.state().db)
        .await
        .unwrap()
}

#[tokio::test]
async fn role_recompute_leaves_unchanged_users_alone() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT Admins", "security", &[LV], &[]));
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;
    let before = xmin(&app, &id).await;
    sync_now(&owner).await;
    assert_eq!(xmin(&app, &id).await, before);
    app.cleanup().await;
}

#[tokio::test]
async fn rename_in_microsoft_is_audited() {
    let (app, fake, owner) = setup_sso().await;
    fake.lock()
        .unwrap()
        .groups
        .insert(HR.into(), fg("HR Staff", "security", &[], &[]));
    let gid = link(&owner, HR).await;
    fake.lock().unwrap().groups.get_mut(HR).unwrap().name = "People".into();
    sync_now(&owner).await;
    sync_now(&owner).await;
    let ev = events(&owner, "group.renamed").await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["target_id"], gid);
    assert_eq!(
        ev[0]["metadata"],
        json!({"from": "HR Staff", "to": "People", "source": "microsoft"})
    );
    app.cleanup().await;
}

#[tokio::test]
async fn slow_microsoft_does_not_hold_up_sign_in() {
    let (app, fake, owner) = setup_sso().await;
    let (id, _b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    fake.lock()
        .unwrap()
        .groups
        .insert(IT.into(), fg("IT Admins", "security", &[LV], &[]));
    let gid = link(&owner, IT).await;
    grant(&owner, &gid, json!("admin")).await;
    // Membership lost in OpnRelay only, so the role recompute alone changes the role.
    sqlx::query("DELETE FROM group_members WHERE group_id = $1::uuid")
        .bind(&gid)
        .execute(&app.state().db)
        .await
        .unwrap();
    fake.lock().unwrap().member_of_delay = Some(std::time::Duration::from_secs(9));
    let t = std::time::Instant::now();
    let (_, b) = sign_in_browser(&app, &fake, LV, "lee@acme.test").await;
    let took = t.elapsed();
    assert_eq!(me_status(&app, &b).await, StatusCode::OK);
    assert!(
        took < std::time::Duration::from_secs(8),
        "sign-in took {took:?}"
    );
    assert_eq!(admin_user(&owner, &id).await["org_role"], "member");
    app.cleanup().await;
}
