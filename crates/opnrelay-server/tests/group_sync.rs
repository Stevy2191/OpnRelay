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

#[tokio::test]
async fn graph_reads_directory() {
    let (base, fake) = fake_microsoft().await;
    let app = TestApp::spawn_with(Some(base), None).await;
    let st = app.state();
    let cfg = app_cfg();
    {
        let mut f = fake.lock().unwrap();
        f.groups
            .insert("H".into(), fg("HR Staff", "security", &["u1"], &[]));
        f.groups.insert(
            "G".into(),
            fg("Medics", "security", &["u1", "u2", "u3"], &["N"]),
        );
        f.groups
            .insert("N".into(), fg("Nested", "m365", &["u4"], &[]));
        f.groups
            .insert("D".into(), fg("Dead", "security", &[], &[]));
        f.deleted_groups.insert("D".into());
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
        graph::get_group(&st, &cfg, "G").await.unwrap().display_name,
        "Medics"
    );

    let want: HashSet<String> = ["u1", "u2", "u3", "u4"].map(String::from).into();
    assert_eq!(graph::group_user_ids(&st, &cfg, "G").await.unwrap(), want);
    let groups = graph::user_group_ids(&st, &cfg, "u4").await.unwrap();
    assert!(groups.contains("G") && groups.contains("N"));

    let en = graph::accounts_enabled(&st, &cfg, &["u1".into(), "gone".into()])
        .await
        .unwrap();
    assert_eq!(
        en,
        std::collections::HashMap::from([("u1".to_string(), true)])
    );

    assert!(matches!(
        graph::get_group(&st, &cfg, "D").await,
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
        .insert("H".into(), fg("HR Staff", "security", &[], &[]));
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
