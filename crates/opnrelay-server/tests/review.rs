// SPDX-License-Identifier: AGPL-3.0-only
//! Needs-review flag, "Still accurate", and the workspace review period.

mod common;

use common::{Session, TestApp};
use reqwest::StatusCode;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

async fn json(r: reqwest::Response) -> Value {
    let status = r.status();
    let text = r.text().await.unwrap();
    serde_json::from_str(&text).unwrap_or_else(|_| panic!("status {status}, body {text}"))
}

fn ts(v: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(v.as_str().unwrap(), &Rfc3339).unwrap()
}

struct Fx {
    app: TestApp,
    owner: Session,
    editor: Session,
    viewer: Session,
    ws: Uuid,
}

async fn setup() -> Fx {
    let app = TestApp::spawn().await;
    let owner = app.owner().await;
    let (editor, eid) = app.user(&owner, "ed@acme.test", "member").await;
    let (viewer, vid) = app.user(&owner, "vi@acme.test", "member").await;
    let ws = json(owner.post("/workspaces", json!({"name": "Ops"})).await).await;
    let ws: Uuid = ws["id"].as_str().unwrap().parse().unwrap();
    let r = owner
        .put(
            &format!("/workspaces/{ws}/members"),
            json!({"members": [
                {"principal_type": "user", "principal_id": owner_id(&owner).await, "role": "manager"},
                {"principal_type": "user", "principal_id": eid, "role": "editor"},
                {"principal_type": "user", "principal_id": vid, "role": "viewer"}
            ]}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    Fx {
        app,
        owner,
        editor,
        viewer,
        ws,
    }
}

async fn owner_id(owner: &Session) -> Uuid {
    let me = json(owner.get("/me").await).await;
    me["id"].as_str().unwrap().parse().unwrap()
}

async fn new_guide(s: &Session, ws: Uuid, publish: bool) -> Uuid {
    let g = json(
        s.post(
            "/guides",
            json!({"workspace_id": ws, "title": "Reset password"}),
        )
        .await,
    )
    .await;
    let id: Uuid = g["id"].as_str().unwrap().parse().unwrap();
    if publish {
        let r = s.post(&format!("/guides/{id}/publish"), json!({})).await;
        assert_eq!(r.status(), StatusCode::OK);
    }
    id
}

async fn age(app: &TestApp, id: Uuid, days: i32) {
    sqlx::query("UPDATE guides SET reviewed_at = now() - make_interval(days => $2) WHERE id = $1")
        .bind(id)
        .bind(days)
        .execute(&app.db)
        .await
        .unwrap();
}

async fn get_guide(s: &Session, id: Uuid) -> Value {
    json(s.get(&format!("/guides/{id}")).await).await
}

async fn flagged_ids(s: &Session) -> Vec<String> {
    json(s.get("/guides?needs_review=true").await)
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap().to_string())
        .collect()
}

async fn count_audit(app: &TestApp, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = $1")
        .bind(action)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn publish_sets_reviewed_and_flag_appears_after_period() {
    let f = setup().await;
    let id = new_guide(&f.editor, f.ws, true).await;
    let g = get_guide(&f.editor, id).await;
    assert!(!g["reviewed_at"].is_null());
    assert_eq!(g["needs_review"], false);
    let due = ts(&g["review_due_at"]) - ts(&g["reviewed_at"]);
    assert_eq!(due.whole_days(), 90);
    assert!(flagged_ids(&f.editor).await.is_empty());

    age(&f.app, id, 91).await;
    let g = get_guide(&f.editor, id).await;
    assert_eq!(g["needs_review"], true);
    assert_eq!(flagged_ids(&f.editor).await, vec![id.to_string()]);
    // Visibility is still enforced for the filter.
    let (outsider, _) = f.app.user(&f.owner, "out@acme.test", "member").await;
    assert!(flagged_ids(&outsider).await.is_empty());
    assert_eq!(flagged_ids(&f.owner).await, vec![id.to_string()]);
    f.app.cleanup().await;
}

#[tokio::test]
async fn drafts_and_archived_never_flagged() {
    let f = setup().await;
    let draft = new_guide(&f.editor, f.ws, false).await;
    sqlx::query("UPDATE guides SET reviewed_at = now() - interval '400 days' WHERE id = $1")
        .bind(draft)
        .execute(&f.app.db)
        .await
        .unwrap();
    let g = get_guide(&f.editor, draft).await;
    assert_eq!(g["needs_review"], false);
    assert!(g["review_due_at"].is_null());

    let arch = new_guide(&f.editor, f.ws, true).await;
    let r = f
        .editor
        .patch(&format!("/guides/{arch}"), json!({"status": "archived"}))
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    age(&f.app, arch, 400).await;
    assert_eq!(get_guide(&f.editor, arch).await["needs_review"], false);
    assert!(flagged_ids(&f.editor).await.is_empty());
    f.app.cleanup().await;
}

#[tokio::test]
async fn still_accurate_clears_without_new_version() {
    let f = setup().await;
    let id = new_guide(&f.editor, f.ws, true).await;
    age(&f.app, id, 100).await;
    let before = get_guide(&f.editor, id).await;
    assert_eq!(before["needs_review"], true);
    let versions = json(f.editor.get(&format!("/guides/{id}/versions")).await).await;

    let r = f
        .editor
        .post(&format!("/guides/{id}/reviewed"), json!({}))
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    let after = json(r).await;
    assert_eq!(after["needs_review"], false);
    assert_eq!(after["id"], before["id"]);
    assert!(!after["steps"].is_null());
    assert_eq!(after["current_version"], before["current_version"]);
    assert_eq!(after["revision"], before["revision"]);
    assert_eq!(
        json(f.editor.get(&format!("/guides/{id}/versions")).await)
            .await
            .as_array()
            .unwrap()
            .len(),
        versions.as_array().unwrap().len()
    );
    assert_eq!(count_audit(&f.app, "guide.reviewed").await, 1);
    f.app.cleanup().await;
}

#[tokio::test]
async fn viewer_cannot_mark_reviewed() {
    let f = setup().await;
    let id = new_guide(&f.editor, f.ws, true).await;
    age(&f.app, id, 100).await;
    let r = f
        .viewer
        .post(&format!("/guides/{id}/reviewed"), json!({}))
        .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(get_guide(&f.editor, id).await["needs_review"], true);
    assert_eq!(count_audit(&f.app, "guide.reviewed").await, 0);
    f.app.cleanup().await;
}

#[tokio::test]
async fn draft_cannot_be_marked_reviewed() {
    let f = setup().await;
    let id = new_guide(&f.editor, f.ws, false).await;
    let r = f
        .editor
        .post(&format!("/guides/{id}/reviewed"), json!({}))
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(count_audit(&f.app, "guide.reviewed").await, 0);
    f.app.cleanup().await;
}

#[tokio::test]
async fn period_off_and_shorter() {
    let f = setup().await;
    let id = new_guide(&f.editor, f.ws, true).await;
    age(&f.app, id, 91).await;
    assert_eq!(get_guide(&f.editor, id).await["needs_review"], true);

    // Off.
    let r = f
        .owner
        .patch(
            &format!("/workspaces/{}", f.ws),
            json!({"review_period_days": null}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(json(r).await["review_period_days"].is_null());
    let g = get_guide(&f.editor, id).await;
    assert_eq!(g["needs_review"], false);
    assert!(g["review_due_at"].is_null());
    assert!(flagged_ids(&f.editor).await.is_empty());

    // Absent key leaves the (now off) period unchanged.
    let r = f
        .owner
        .patch(
            &format!("/workspaces/{}", f.ws),
            json!({"description": "x"}),
        )
        .await;
    assert!(json(r).await["review_period_days"].is_null());

    // Shorter.
    age(&f.app, id, 45).await;
    let r = f
        .owner
        .patch(
            &format!("/workspaces/{}", f.ws),
            json!({"review_period_days": 30}),
        )
        .await;
    assert_eq!(json(r).await["review_period_days"], 30);
    assert_eq!(get_guide(&f.editor, id).await["needs_review"], true);

    for bad in [0, 4000] {
        let r = f
            .owner
            .patch(
                &format!("/workspaces/{}", f.ws),
                json!({"review_period_days": bad}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{bad}");
    }
    let r = f
        .editor
        .patch(
            &format!("/workspaces/{}", f.ws),
            json!({"review_period_days": 10}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);

    let meta: Value = sqlx::query_scalar(
        "SELECT metadata FROM audit_events WHERE action = 'workspace.updated' ORDER BY seq LIMIT 1",
    )
    .fetch_one(&f.app.db)
    .await
    .unwrap();
    assert!(meta["review_period_days"].is_null() && meta.get("review_period_days").is_some());
    f.app.cleanup().await;
}

#[tokio::test]
async fn new_workspace_defaults_to_90() {
    let f = setup().await;
    let ws = json(f.owner.get(&format!("/workspaces/{}", f.ws)).await).await;
    assert_eq!(ws["review_period_days"], 90);
    f.app.cleanup().await;
}
