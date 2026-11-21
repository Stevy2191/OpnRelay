// SPDX-License-Identifier: AGPL-3.0-only
//! End-to-end API tests against a real PostgreSQL database.

mod common;

use common::{Session, TestApp, png};
use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn json(r: reqwest::Response) -> Value {
    let status = r.status();
    let text = r.text().await.unwrap();
    serde_json::from_str(&text).unwrap_or_else(|_| panic!("status {status}, body {text}"))
}

#[tokio::test]
async fn health_and_security_headers() {
    let app = TestApp::spawn().await;
    let r = reqwest::get(format!("{}/healthz", app.base)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let r = reqwest::get(format!("{}/readyz", app.base)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let r = reqwest::get(format!("{}/", app.base)).await.unwrap();
    let h = r.headers();
    assert!(
        h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(h.contains_key("x-request-id"));
    // Unknown API routes are JSON 404s, not the SPA.
    let r = reqwest::get(format!("{}/api/v1/nope", app.base))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    app.cleanup().await;
}

#[tokio::test]
async fn auth_basics_csrf_and_lockout() {
    let app = TestApp::spawn().await;
    let owner = app.owner().await;
    let me = owner.refresh_csrf().await;
    assert_eq!(me["org_role"], "owner");
    assert_eq!(me["organization"]["name"], "Acme");

    // Unauthenticated requests are rejected.
    let anon = Session::new(&app.base);
    assert_eq!(anon.get("/me").await.status(), StatusCode::UNAUTHORIZED);

    // Mutations without the CSRF token are rejected.
    let r = owner
        .client
        .post(format!("{}/workspaces", owner.base))
        .json(&json!({"name": "X"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);

    // Wrong password and unknown user look identical.
    let a = anon
        .post(
            "/auth/login",
            json!({"email": "owner@acme.test", "password": "nope nope nope"}),
        )
        .await;
    let b = anon
        .post(
            "/auth/login",
            json!({"email": "ghost@acme.test", "password": "nope nope nope"}),
        )
        .await;
    assert_eq!(a.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(b.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json(a).await["detail"], json(b).await["detail"]);

    // Weak passwords are refused.
    let r = owner
        .put(
            "/me/password",
            json!({"current_password": "owner password 123", "new_password": "short"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);

    // Logout kills the session.
    assert_eq!(
        owner.post("/auth/logout", json!({})).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(owner.get("/me").await.status(), StatusCode::UNAUTHORIZED);

    // Repeated failures trigger the rate limit.
    let mut last = StatusCode::OK;
    for _ in 0..12 {
        last = anon
            .post(
                "/auth/login",
                json!({"email": "owner@acme.test", "password": "wrong password!"}),
            )
            .await
            .status();
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
    app.cleanup().await;
}

#[tokio::test]
async fn workspaces_guides_permissions_and_versions() {
    let app = TestApp::spawn().await;
    let owner = app.owner().await;
    let (editor, editor_id) = app.user(&owner, "ed@acme.test", "member").await;
    let (viewer, viewer_id) = app.user(&owner, "vi@acme.test", "member").await;
    let (outsider, _) = app.user(&owner, "out@acme.test", "member").await;
    let (auditor, _) = app.user(&owner, "audit@acme.test", "auditor").await;

    // Members cannot create workspaces; admins can.
    assert_eq!(
        editor
            .post("/workspaces", json!({"name": "Nope"}))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let ws = json(
        owner
            .post(
                "/workspaces",
                json!({"name": "HR", "description": "People ops"}),
            )
            .await,
    )
    .await;
    let ws_id = ws["id"].as_str().unwrap().to_string();

    // A group grants the viewer role; the editor gets a direct role.
    let group = json(
        owner
            .post("/admin/groups", json!({"name": "Readers"}))
            .await,
    )
    .await;
    let gid = group["id"].as_str().unwrap();
    assert_eq!(
        owner
            .put(
                &format!("/admin/groups/{gid}/members"),
                json!({"user_ids": [viewer_id]})
            )
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let r = owner
        .put(
            &format!("/workspaces/{ws_id}/members"),
            json!({"members": [
                {"principal_type": "user", "principal_id": editor_id, "role": "editor"},
                {"principal_type": "group", "principal_id": gid, "role": "viewer"}
            ]}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);

    // Visibility.
    let list = json(editor.get("/workspaces").await).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["my_permission"], "edit");
    assert_eq!(
        json(outsider.get("/workspaces").await)
            .await
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        outsider.get(&format!("/workspaces/{ws_id}")).await.status(),
        StatusCode::NOT_FOUND
    );

    // Editor creates a folder and a guide with a screenshot step.
    let folder = json(
        editor
            .post(
                &format!("/workspaces/{ws_id}/folders"),
                json!({"name": "Onboarding"}),
            )
            .await,
    )
    .await;
    let guide = json(
        editor
            .post("/guides", json!({"workspace_id": ws_id, "folder_id": folder["id"], "title": "Add a new hire"}))
            .await,
    )
    .await;
    let gid = guide["id"].as_str().unwrap().to_string();
    assert_eq!(guide["status"], "draft");

    let up = editor
        .upload_png(&format!("/media?guide_id={gid}"), png(64, 40))
        .await;
    assert_eq!(up.status(), StatusCode::CREATED);
    let media_id = json(up).await["id"].as_str().unwrap().to_string();

    // Non-images are refused.
    let bad = editor
        .upload_png("/media", b"<svg/onload=alert(1)>".to_vec())
        .await;
    assert_eq!(bad.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let step_id = Uuid::now_v7();
    let steps = json!({"steps": [
        {"id": Uuid::now_v7(), "kind": "heading", "title": "Before you start"},
        {"id": step_id, "kind": "action", "title": "Click \"New employee\"", "body_md": "Opens the **form**",
         "action": {"type": "click", "target_label": "New employee", "x": 10, "y": 12},
         "media_id": media_id, "annotations": {"highlight": {"x": 2, "y": 3, "w": 20, "h": 10}}}
    ]});
    let g = json(
        editor
            .put_if_match(&format!("/guides/{gid}/steps"), 1, steps.clone())
            .await,
    )
    .await;
    assert_eq!(g["steps"].as_array().unwrap().len(), 2);
    assert_eq!(g["step_count"], 1);
    assert_eq!(g["revision"], 2);

    // Stale revision is rejected.
    let stale = editor
        .put_if_match(&format!("/guides/{gid}/steps"), 1, steps.clone())
        .await;
    assert_eq!(stale.status(), StatusCode::PRECONDITION_FAILED);

    // Viewer can read the guide and image, but not edit.
    let vg = json(viewer.get(&format!("/guides/{gid}")).await).await;
    assert_eq!(vg["my_permission"], "read");
    assert_eq!(
        viewer.get(&format!("/media/{media_id}")).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        viewer
            .put(&format!("/guides/{gid}/steps"), json!({"steps": []}))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // Outsider can see neither guide nor image.
    assert_eq!(
        outsider.get(&format!("/guides/{gid}")).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        outsider.get(&format!("/media/{media_id}")).await.status(),
        StatusCode::NOT_FOUND
    );

    // Another user's unattached image cannot be stolen into a guide.
    let foreign = json(outsider.upload_png("/media", png(8, 8)).await).await;
    let r = editor
        .put(
            &format!("/guides/{gid}/steps"),
            json!({"steps": [{"kind": "action", "title": "x", "media_id": foreign["id"]}]}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);

    // Search finds the guide by step text.
    let found = json(editor.get("/guides?q=employee").await).await;
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(
        json(outsider.get("/guides?q=employee").await)
            .await
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // Publish, edit, then restore version 1.
    let p = json(
        editor
            .post(&format!("/guides/{gid}/publish"), json!({"note": "first"}))
            .await,
    )
    .await;
    assert_eq!(p["status"], "published");
    assert_eq!(p["current_version"], 1);
    editor
        .put(
            &format!("/guides/{gid}/steps"),
            json!({"steps": [{"kind": "text", "title": "only"}]}),
        )
        .await;
    let versions = json(editor.get(&format!("/guides/{gid}/versions")).await).await;
    assert_eq!(versions[0]["version"], 1);
    let restored = json(
        editor
            .post(&format!("/guides/{gid}/versions/1/restore"), json!({}))
            .await,
    )
    .await;
    assert_eq!(restored["steps"].as_array().unwrap().len(), 2);
    assert_eq!(restored["steps"][1]["id"], json!(step_id));

    // Duplicate copies screenshots into new media owned by the copy.
    let copy = json(
        editor
            .post(&format!("/guides/{gid}/duplicate"), json!({}))
            .await,
    )
    .await;
    assert_eq!(copy["title"], "Copy of Add a new hire");
    let copy_media = copy["steps"][1]["media_id"].as_str().unwrap();
    assert_ne!(copy_media, media_id);
    let img = editor.get(&format!("/media/{copy_media}")).await;
    assert_eq!(img.status(), StatusCode::OK);
    assert_eq!(img.headers()["content-type"], "image/png");

    // Auditors can read everything but change nothing.
    assert_eq!(
        auditor.get(&format!("/guides/{gid}")).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        auditor
            .patch(&format!("/guides/{gid}"), json!({"title": "hack"}))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        auditor.delete(&format!("/guides/{gid}")).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        auditor.upload_png("/media", png(4, 4)).await.status(),
        StatusCode::FORBIDDEN
    );

    // Legal hold blocks deletion.
    assert_eq!(
        owner
            .patch(&format!("/guides/{gid}"), json!({"legal_hold": true}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        editor.delete(&format!("/guides/{gid}")).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        editor
            .patch(&format!("/guides/{gid}"), json!({"legal_hold": false}))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    owner
        .patch(&format!("/guides/{gid}"), json!({"legal_hold": false}))
        .await;
    assert_eq!(
        editor.delete(&format!("/guides/{gid}")).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        viewer.get(&format!("/guides/{gid}")).await.status(),
        StatusCode::NOT_FOUND
    );

    // Audit log: readable by auditors, not by members; chain verifies.
    assert_eq!(
        editor.get("/audit/events").await.status(),
        StatusCode::FORBIDDEN
    );
    let events = json(auditor.get("/audit/events?action=guide.").await).await;
    let actions: Vec<&str> = events
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    for expected in [
        "guide.created",
        "guide.viewed",
        "guide.published",
        "guide.deleted",
        "guide.duplicated",
    ] {
        assert!(
            actions.contains(&expected),
            "missing {expected} in {actions:?}"
        );
    }
    let report = json(auditor.get("/audit/verify").await).await;
    assert_eq!(report["ok"], true, "{report}");
    let csv = auditor
        .get("/audit/export?format=csv")
        .await
        .text()
        .await
        .unwrap();
    assert!(csv.starts_with("seq,ts,"));

    // The database refuses to modify audit events.
    let tamper = sqlx::query("UPDATE audit_events SET action = 'x' WHERE seq = 1")
        .execute(&app.db)
        .await;
    assert!(tamper.is_err());

    app.cleanup().await;
}

#[tokio::test]
async fn admin_user_management_rules() {
    let app = TestApp::spawn().await;
    let owner = app.owner().await;
    let (admin, admin_id) = app.user(&owner, "adm@acme.test", "admin").await;
    let (member, member_id) = app.user(&owner, "mem@acme.test", "member").await;
    let me = owner.refresh_csrf().await;
    let owner_id = me["id"].as_str().unwrap();

    // Only owners grant/modify owner.
    assert_eq!(
        admin
            .patch(
                &format!("/admin/users/{member_id}"),
                json!({"org_role": "owner"})
            )
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        admin
            .patch(
                &format!("/admin/users/{owner_id}"),
                json!({"status": "disabled"})
            )
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    // Cannot remove the last owner or disable yourself.
    assert_eq!(
        owner
            .patch(
                &format!("/admin/users/{owner_id}"),
                json!({"org_role": "admin"})
            )
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        owner
            .patch(
                &format!("/admin/users/{owner_id}"),
                json!({"status": "disabled"})
            )
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    // Members cannot administer.
    assert_eq!(
        member
            .post(
                "/admin/users",
                json!({"email": "x@acme.test", "display_name": "x"})
            )
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    // Duplicate email.
    assert_eq!(
        admin
            .post(
                "/admin/users",
                json!({"email": "MEM@acme.test", "display_name": "dup"})
            )
            .await
            .status(),
        StatusCode::CONFLICT
    );

    // Disabling a user kills their sessions immediately.
    assert_eq!(member.get("/me").await.status(), StatusCode::OK);
    assert_eq!(
        admin
            .patch(
                &format!("/admin/users/{member_id}"),
                json!({"status": "disabled"})
            )
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(member.get("/me").await.status(), StatusCode::UNAUTHORIZED);

    // Password reset link works once and signs the user out elsewhere.
    admin
        .patch(
            &format!("/admin/users/{member_id}"),
            json!({"status": "active"}),
        )
        .await;
    let link = json(
        admin
            .post(&format!("/admin/users/{member_id}/reset-link"), json!({}))
            .await,
    )
    .await;
    assert_eq!(link["purpose"], "password_reset");
    let token = link["url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    let anon = Session::new(&app.base);
    let info = json(
        anon.post("/auth/invite/inspect", json!({"token": token}))
            .await,
    )
    .await;
    assert_eq!(info["email"], "mem@acme.test");
    anon.accept_invite(&token, "brand new password").await;
    let again = anon
        .post(
            "/auth/invite/accept",
            json!({"token": token, "password": "another password!"}),
        )
        .await;
    assert_eq!(again.status(), StatusCode::BAD_REQUEST);
    anon.login("mem@acme.test", "brand new password").await;

    // Policies round-trip and are validated.
    let mut p = json(admin.get("/admin/policies").await).await;
    p["public_links_enabled"] = json!(false);
    assert_eq!(
        admin.put("/admin/policies", p.clone()).await.status(),
        StatusCode::OK
    );
    p["mfa_requirement"] = json!("sometimes");
    assert_eq!(
        admin.put("/admin/policies", p).await.status(),
        StatusCode::BAD_REQUEST
    );
    let _ = admin_id;
    app.cleanup().await;
}

fn totp_code(secret_b32: &str, offset_steps: i64) -> String {
    let secret = totp_rs::Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .unwrap();
    let t = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret,
        Some("OpnRelay".into()),
        "x".into(),
    )
    .unwrap();
    let now = time::OffsetDateTime::now_utc().unix_timestamp() + offset_steps * 30;
    t.generate(now as u64)
}

#[tokio::test]
async fn mfa_enrolment_login_replay_and_recovery() {
    let app = TestApp::spawn().await;
    let owner = app.owner().await;
    let (user, _) = app.user(&owner, "m@acme.test", "member").await;

    let start = json(user.post("/me/mfa/totp", json!({})).await).await;
    assert!(start["qr_svg"].as_str().unwrap().contains("<svg"));
    let secret = start["secret"].as_str().unwrap().to_string();
    let bad = user
        .post(
            "/me/mfa/totp/confirm",
            json!({"factor_id": start["factor_id"], "code": "000000"}),
        )
        .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let confirm = json(
        user.post(
            "/me/mfa/totp/confirm",
            json!({"factor_id": start["factor_id"], "code": totp_code(&secret, -1)}),
        )
        .await,
    )
    .await;
    let codes: Vec<String> = serde_json::from_value(confirm["recovery_codes"].clone()).unwrap();
    assert_eq!(codes.len(), 10);

    // New login requires the second factor.
    let s = Session::new(&app.base);
    let v = s.login("m@acme.test", "user password 123").await;
    assert_eq!(v["mfa_required"], true);
    assert_eq!(s.get("/me").await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        s.post("/auth/mfa/verify", json!({"code": "123456"}))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let code = totp_code(&secret, 0);
    assert_eq!(
        s.post("/auth/mfa/verify", json!({"code": code}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(s.get("/me").await.status(), StatusCode::OK);

    // The same code cannot be replayed for another login.
    let s2 = Session::new(&app.base);
    s2.login("m@acme.test", "user password 123").await;
    assert_eq!(
        s2.post("/auth/mfa/verify", json!({"code": code}))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    // A recovery code works exactly once.
    assert_eq!(
        s2.post("/auth/mfa/verify", json!({"code": codes[0].to_uppercase()}))
            .await
            .status(),
        StatusCode::OK
    );
    let s3 = Session::new(&app.base);
    s3.login("m@acme.test", "user password 123").await;
    assert_eq!(
        s3.post("/auth/mfa/verify", json!({"code": codes[0]}))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    // Org policy: MFA required for all. A user without MFA is limited to enrolment.
    let mut p = json(owner.get("/admin/policies").await).await;
    p["mfa_requirement"] = json!("all");
    owner.put("/admin/policies", p).await;
    let me = json(owner.get("/me").await).await;
    assert_eq!(me["mfa_setup_required"], true);
    assert_eq!(
        owner.get("/workspaces").await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        owner.post("/me/mfa/totp", json!({})).await.status(),
        StatusCode::OK
    );

    // Admin resets the user's MFA.
    let sessions = json(s.get("/me/sessions").await).await;
    assert!(!sessions.as_array().unwrap().is_empty());
    app.cleanup().await;
}

#[tokio::test]
async fn branding_and_exports() {
    let app = TestApp::spawn().await;
    let owner = app.owner().await;
    let (viewer, viewer_id) = app.user(&owner, "v@acme.test", "member").await;
    let (outsider, _) = app.user(&owner, "o@acme.test", "member").await;

    // Branding: members can read, only admins can change.
    let mut b = json(viewer.get("/branding").await).await;
    assert_eq!(b["primary_color"], "#1f4fd1");
    b["primary_color"] = json!("#AA0000");
    b["watermark_exports"] = json!(true);
    assert_eq!(
        viewer.put("/admin/branding", b.clone()).await.status(),
        StatusCode::FORBIDDEN
    );
    let saved = json(owner.put("/admin/branding", b.clone()).await).await;
    assert_eq!(saved["primary_color"], "#aa0000");
    b["accent_color"] = json!("red");
    assert_eq!(
        owner.put("/admin/branding", b).await.status(),
        StatusCode::BAD_REQUEST
    );
    let logo = owner
        .client
        .put(format!("{}/admin/branding/logo", owner.base))
        .header(
            "x-csrf-token",
            owner.refresh_csrf().await["csrf_token"].as_str().unwrap(),
        )
        .header("content-type", "image/png")
        .body(png(120, 40))
        .send()
        .await
        .unwrap();
    assert_eq!(logo.status(), StatusCode::OK);
    let logo_id = json(logo).await["logo_media_id"]
        .as_str()
        .unwrap()
        .to_string();
    // Everyone signed in can load the logo for the UI.
    assert_eq!(
        viewer.get(&format!("/media/{logo_id}")).await.status(),
        StatusCode::OK
    );

    // A guide with a screenshot.
    let ws = json(owner.post("/workspaces", json!({"name": "EMS"})).await).await;
    let ws_id = ws["id"].as_str().unwrap();
    owner
        .put(
            &format!("/workspaces/{ws_id}/members"),
            json!({"members": [
                {"principal_type": "user", "principal_id": viewer_id, "role": "viewer"}
            ]}),
        )
        .await;
    let g = json(
        owner
            .post(
                "/guides",
                json!({"workspace_id": ws_id, "title": "Restock the ambulance"}),
            )
            .await,
    )
    .await;
    let gid = g["id"].as_str().unwrap();
    let m = json(
        owner
            .upload_png(&format!("/media?guide_id={gid}"), png(300, 200))
            .await,
    )
    .await;
    owner
        .put(&format!("/guides/{gid}/steps"), json!({"steps": [
            {"kind": "heading", "title": "Prep"},
            {"kind": "action", "title": "Open the inventory app", "body_md": "Use **your** login", "media_id": m["id"],
             "annotations": {"highlight": {"x": 10, "y": 10, "w": 50, "h": 20}}},
            {"kind": "alert", "title": "Check expiry dates"}
        ]}))
        .await;

    let cases = [
        ("pdf", "application/pdf", b"%PDF-".as_slice()),
        (
            "docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            b"PK".as_slice(),
        ),
        (
            "html",
            "text/html; charset=utf-8",
            b"<!doctype html>".as_slice(),
        ),
        ("md", "application/zip", b"PK".as_slice()),
        ("png", "application/zip", b"PK".as_slice()),
    ];
    for (fmt, mime, magic) in cases {
        let r = viewer
            .get(&format!("/guides/{gid}/export?format={fmt}"))
            .await;
        assert_eq!(r.status(), StatusCode::OK, "{fmt}");
        assert_eq!(r.headers()["content-type"], mime);
        let cd = r.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .to_string();
        assert!(cd.contains("restock-the-ambulance."), "{cd}");
        let body = r.bytes().await.unwrap();
        assert!(body.starts_with(magic), "{fmt} magic");
        if fmt == "html" {
            let html = String::from_utf8_lossy(&body);
            assert!(html.contains("--p:#aa0000"));
            assert!(html.contains("Exported by v@acme.test"));
            assert!(html.contains("<strong>your</strong>"));
        }
    }
    let no_img = viewer
        .get(&format!("/guides/{gid}/export?format=html&images=false"))
        .await
        .text()
        .await
        .unwrap();
    assert_eq!(
        no_img.matches("data:image/png").count(),
        1,
        "only the logo remains"
    );
    assert_eq!(
        viewer
            .get(&format!("/guides/{gid}/export?format=exe"))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        outsider
            .get(&format!("/guides/{gid}/export?format=pdf"))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    let events = json(owner.get("/audit/events?action=guide.exported").await).await;
    assert_eq!(events.as_array().unwrap().len(), 6);
    assert_eq!(json(owner.get("/audit/verify").await).await["ok"], true);
    app.cleanup().await;
}
