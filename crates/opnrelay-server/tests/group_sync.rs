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
