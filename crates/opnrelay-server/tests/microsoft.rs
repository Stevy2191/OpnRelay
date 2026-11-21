// SPDX-License-Identifier: AGPL-3.0-only
//! Microsoft sign-in, OneDrive backups and restore, against the shared fake
//! Microsoft identity platform + Graph in `common/fake_ms.rs`.

mod common;

use common::fake_ms::{
    CLIENT, TENANT, browser, fake_microsoft, location, microsoft_flow, next_user,
};
use common::{TestApp, png};
use reqwest::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn microsoft_sign_in_onedrive_backup_and_restore() {
    let (ms, fake) = fake_microsoft().await;
    let app = TestApp::spawn_with(Some(ms.clone()), None).await;
    let owner = app.owner().await;
    let api = format!("{}/api/v1", app.base);

    // Configure the Entra app.
    let bad = owner
        .put("/admin/sso", json!({"enabled": true, "tenant_id": "common", "client_id": CLIENT, "client_secret": "s3cret",
            "jit_provisioning": true, "default_role": "member", "sso_only": false}))
        .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let ok = owner
        .put("/admin/sso", json!({"enabled": true, "tenant_id": TENANT, "client_id": CLIENT, "client_secret": "s3cret",
            "jit_provisioning": true, "default_role": "member", "sso_only": false}))
        .await;
    assert_eq!(ok.status(), StatusCode::NO_CONTENT);
    let settings: Value = owner.get("/admin/sso").await.json().await.unwrap();
    assert_eq!(settings["has_secret"], true);
    assert!(
        settings.get("client_secret").is_none(),
        "the secret is never returned"
    );
    let providers: Value = reqwest::get(format!("{api}/auth/providers"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(providers, json!({"microsoft": true, "password": true}));

    // A new employee signs in with Microsoft and is created automatically.
    next_user(&fake, "oid-morgan@acme.test", "morgan@acme.test");
    let b = browser();
    let r = microsoft_flow(
        &b,
        &format!("{api}/auth/sso/microsoft/start?return_to=/connect%3Fx%3D1"),
    )
    .await;
    assert_eq!(location(&r), "/#/connect?x=1");
    let me: Value = b
        .get(format!("{api}/me"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(me["email"], "morgan@acme.test");
    assert_eq!(me["org_role"], "member");

    // A Microsoft account from another tenant is refused.
    fake.lock().unwrap().next_user = (
        "oid-eve@other.test".into(),
        "eve@other.test".into(),
        "33333333-3333-3333-3333-333333333333".into(),
    );
    let r = microsoft_flow(&browser(), &format!("{api}/auth/sso/microsoft/start")).await;
    assert!(
        location(&r).starts_with("/#/login?error="),
        "{}",
        location(&r)
    );

    // The callback cannot be completed in a different browser (login CSRF).
    next_user(&fake, "oid-morgan@acme.test", "morgan@acme.test");
    let b1 = browser();
    let r = b1
        .get(format!("{api}/auth/sso/microsoft/start"))
        .send()
        .await
        .unwrap();
    let r = b1.get(location(&r)).send().await.unwrap();
    let r = browser().get(location(&r)).send().await.unwrap();
    assert!(
        location(&r).contains("another%20browser"),
        "{}",
        location(&r)
    );

    // Microsoft-only mode: members can no longer use passwords; owners can.
    owner
        .put(
            "/admin/sso",
            json!({"enabled": true, "tenant_id": TENANT, "client_id": CLIENT, "client_secret": "",
            "jit_provisioning": true, "default_role": "member", "sso_only": true}),
        )
        .await;
    let providers: Value = reqwest::get(format!("{api}/auth/providers"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(providers["password"], false);
    let (member, _) = {
        // invite_accept still works but password login is refused for members
        let r = owner
            .post(
                "/admin/users",
                json!({"email": "pw@acme.test", "display_name": "Pw"}),
            )
            .await;
        let v: Value = r.json().await.unwrap();
        let token = v["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        let s = common::Session::new(&app.base);
        s.accept_invite(&token, "member password 1").await;
        (s, ())
    };
    let r = member
        .post(
            "/auth/login",
            json!({"email": "pw@acme.test", "password": "member password 1"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let r = owner
        .post(
            "/auth/login",
            json!({"email": "owner@acme.test", "password": "owner password 123"}),
        )
        .await;
    assert_eq!(
        r.status(),
        StatusCode::OK,
        "owners keep break-glass password access"
    );
    owner.refresh_csrf().await;

    // Content to back up: a guide with a screenshot.
    let ws: Value = owner
        .post("/workspaces", json!({"name": "EMS"}))
        .await
        .json()
        .await
        .unwrap();
    let g: Value = owner
        .post(
            "/guides",
            json!({"workspace_id": ws["id"], "title": "Restock"}),
        )
        .await
        .json()
        .await
        .unwrap();
    let gid = g["id"].as_str().unwrap().to_string();
    let img = png(40, 30);
    let m: Value = owner
        .upload_png(&format!("/media?guide_id={gid}"), img.clone())
        .await
        .json()
        .await
        .unwrap();
    let mid = m["id"].as_str().unwrap().to_string();
    owner
        .put(
            &format!("/guides/{gid}/steps"),
            json!({"steps": [{"kind": "action", "title": "Open", "media_id": mid}]}),
        )
        .await;
    let original_media = owner
        .get(&format!("/media/{mid}"))
        .await
        .bytes()
        .await
        .unwrap();

    // Connect OneDrive through Microsoft consent (as the owner, in their browser).
    let ob = owner.client.clone();
    let r = ob
        .get(format!("{api}/admin/backups/onedrive/connect"))
        .send()
        .await
        .unwrap();
    // The owner's client follows redirects; it ends on the app root after the callback.
    assert!(r.status().is_success(), "{}", r.status());
    let status: Value = owner.get("/admin/backups").await.json().await.unwrap();
    assert_eq!(status["settings"]["onedrive_account"], "backup@acme.test");
    assert_eq!(status["settings"]["onedrive_enabled"], true);

    // Keep two remote copies; run three backups.
    let mut s = status["settings"].clone();
    s["keep_remote"] = json!(2);
    assert_eq!(
        owner.put("/admin/backups/settings", s).await.status(),
        StatusCode::OK
    );
    let mut last = String::new();
    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await; // distinct timestamps in names
        assert_eq!(
            owner.post("/admin/backups/run", json!({})).await.status(),
            StatusCode::ACCEPTED
        );
        for _ in 0..100 {
            let st: Value = owner.get("/admin/backups").await.json().await.unwrap();
            if st["settings"]["last_file"]
                .as_str()
                .is_some_and(|f| f != last)
                && st["settings"]["last_status"] == "ok"
            {
                last = st["settings"]["last_file"].as_str().unwrap().to_string();
                break;
            }
            assert_ne!(
                st["settings"]["last_status"], "failed",
                "{}",
                st["settings"]["last_error"]
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    let files = fake.lock().unwrap().files.clone();
    assert_eq!(files.len(), 2, "older OneDrive backups are pruned");
    assert!(files.iter().any(|(_, n, _)| n == &last));
    let uploaded = files.iter().find(|(_, n, _)| n == &last).unwrap().2.clone();
    assert!(
        uploaded.starts_with(b"ORBKUP01"),
        "OneDrive only ever sees encrypted data"
    );
    assert!(!uploaded.windows(7).any(|w| w == b"Restock"));
    let local = std::fs::read(app.storage.join("backups").join(&last)).unwrap();
    assert_eq!(local, uploaded);
    let r = owner.get(&format!("/admin/backups/files/{last}")).await;
    assert_eq!(r.status(), StatusCode::OK);

    // Restore into a brand-new installation that has the same master key.
    let backup_file = app.storage.join("backups").join(&last);
    let fresh = TestApp::spawn_with(None, Some(app.master_key.clone())).await;
    let manifest = opnrelay_server::backup::restore(
        &fresh.db,
        &fresh.storage,
        &fresh.master_key,
        &backup_file,
    )
    .await
    .unwrap();
    assert!(
        manifest
            .tables
            .iter()
            .any(|t| t.name == "guides" && t.rows >= 1)
    );
    // Restoring twice is refused.
    assert!(
        opnrelay_server::backup::restore(
            &fresh.db,
            &fresh.storage,
            &fresh.master_key,
            &backup_file
        )
        .await
        .is_err()
    );
    // The owner signs in on the restored copy and sees the same guide and screenshot.
    let o2 = common::Session::new(&fresh.base);
    o2.login("owner@acme.test", "owner password 123").await;
    let g2: Value = o2
        .get(&format!("/guides/{gid}"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(g2["title"], "Restock");
    let restored_media = o2
        .get(&format!("/media/{mid}"))
        .await
        .bytes()
        .await
        .unwrap();
    assert_eq!(restored_media, original_media);
    let verify: Value = o2.get("/audit/verify").await.json().await.unwrap();
    assert_eq!(
        verify["ok"], true,
        "audit chain survives backup and restore: {verify}"
    );

    // A wrong master key cannot read the backup.
    let other = TestApp::spawn().await;
    assert!(
        opnrelay_server::backup::restore(
            &other.db,
            &other.storage,
            &other.master_key,
            &backup_file
        )
        .await
        .is_err()
    );

    fresh.cleanup().await;
    other.cleanup().await;
    app.cleanup().await;
}
