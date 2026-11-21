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
    // Let get_group pass, fail the member listing.
    fake.lock().unwrap().users_fail = false;
    fake.lock().unwrap().graph_fail = true;
    let r = owner
        .post("/admin/groups/link", json!({"external_id": HR}))
        .await;
    assert!(r.status().is_client_error() || r.status().is_server_error());
    fake.lock().unwrap().graph_fail = false;
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
