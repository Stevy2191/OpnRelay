// SPDX-License-Identifier: AGPL-3.0-only
//! Test harness: a fresh database and a running server per test.
//!
//! Requires PostgreSQL. Set `OPNRELAY_TEST_DATABASE_URL` to a URL for a role
//! that may create databases (default: postgres://opnrelay:opnrelay@localhost/postgres).

#![allow(dead_code)]

pub mod fake_ms;

use std::net::SocketAddr;

use opnrelay_crypto::SecretKey;
use opnrelay_server::config::Config;
use opnrelay_server::state::AppState;
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub struct TestApp {
    pub base: String,
    pub db: PgPool,
    pub db_name: String,
    admin_url: String,
    pub storage: std::path::PathBuf,
    pub master_key: SecretKey,
    pub db_cfg: Config,
}

pub fn admin_url() -> String {
    std::env::var("OPNRELAY_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://opnrelay:opnrelay@localhost/postgres".into())
}

fn with_db(url: &str, db: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("db url has a path");
    format!("{base}/{db}")
}

impl TestApp {
    pub async fn spawn() -> TestApp {
        Self::spawn_with(None, None).await
    }

    /// `ms_url`: base URL of a fake Microsoft service; `key`: reuse a master key.
    pub async fn spawn_with(ms_url: Option<String>, key: Option<SecretKey>) -> TestApp {
        let ms = ms_url.unwrap_or_else(|| "http://127.0.0.1:9".into());
        let master_key = key.unwrap_or_else(SecretKey::generate);
        let admin = admin_url();
        let db_name = format!("opnrelay_test_{}", Uuid::now_v7().simple());
        let admin_pool = PgPool::connect(&admin).await.expect("connect admin db");
        sqlx::query(&format!("CREATE DATABASE {db_name}"))
            .execute(&admin_pool)
            .await
            .unwrap();
        admin_pool.close().await;
        let url = with_db(&admin, &db_name);
        let db = opnrelay_server::connect(&url).await.unwrap();
        opnrelay_server::MIGRATOR.run(&db).await.unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let storage = std::env::temp_dir().join(format!("opnrelay-it-{}", Uuid::now_v7()));
        let cfg = Config {
            bind: addr,
            public_url: base.clone(),
            database_url: url,
            master_key: master_key.clone(),
            storage_dir: storage.clone(),
            cookie_secure: false,
            trust_proxy: false,
            max_upload_bytes: 20 * 1024 * 1024,
            downloads_dir: Some(storage.join("downloads")),
            backup_dir: storage.join("backups"),
            ms_login_url: ms.clone(),
            ms_graph_url: format!("{ms}/v1.0"),
        };
        let state = AppState::new(db.clone(), cfg.clone());
        state.storage.ensure_root().await.unwrap();
        let app = opnrelay_server::app(state);
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        TestApp {
            base,
            db,
            db_name,
            admin_url: admin,
            storage,
            master_key,
            db_cfg: cfg,
        }
    }

    /// An `AppState` with the same config the spawned server uses.
    pub fn state(&self) -> AppState {
        AppState::new(self.db.clone(), self.db_cfg.clone())
    }

    pub async fn cleanup(self) {
        self.db.close().await;
        let admin_pool = PgPool::connect(&self.admin_url).await.unwrap();
        let _ = sqlx::query(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.db_name
        ))
        .execute(&admin_pool)
        .await;
        let _ = tokio::fs::remove_dir_all(&self.storage).await;
    }

    /// Bootstrap the owner and return a signed-in client.
    pub async fn owner(&self) -> Session {
        let url = opnrelay_server::bootstrap::create_owner(
            &self.db,
            &self.base,
            "Acme",
            "owner@acme.test",
            "Olive Owner",
        )
        .await
        .unwrap();
        let token = url.rsplit('/').next().unwrap().to_string();
        let s = Session::new(&self.base);
        s.accept_invite(&token, "owner password 123").await;
        s.login("owner@acme.test", "owner password 123").await;
        s
    }

    /// Invite a user as `admin` and return a signed-in session for them.
    pub async fn user(&self, admin: &Session, email: &str, role: &str) -> (Session, Uuid) {
        let r = admin
            .post(
                "/admin/users",
                json!({"email": email, "display_name": email, "org_role": role}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::CREATED);
        let v: Value = r.json().await.unwrap();
        let token = v["invite_url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        let id: Uuid = serde_json::from_value(v["user"]["id"].clone()).unwrap();
        let s = Session::new(&self.base);
        s.accept_invite(&token, "user password 123").await;
        s.login(email, "user password 123").await;
        (s, id)
    }
}

pub struct Session {
    pub base: String,
    pub client: Client,
    pub csrf: std::sync::Mutex<String>,
}

impl Session {
    pub fn new(base: &str) -> Self {
        Session {
            base: format!("{base}/api/v1"),
            client: Client::builder().cookie_store(true).build().unwrap(),
            csrf: std::sync::Mutex::new(String::new()),
        }
    }

    fn csrf(&self) -> String {
        self.csrf.lock().unwrap().clone()
    }

    pub async fn refresh_csrf(&self) -> Value {
        let me: Value = self.get("/me").await.json().await.unwrap();
        *self.csrf.lock().unwrap() = me["csrf_token"].as_str().unwrap_or_default().to_string();
        me
    }

    pub async fn accept_invite(&self, token: &str, password: &str) {
        let r = self
            .post(
                "/auth/invite/accept",
                json!({"token": token, "password": password}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::OK, "{}", r.text().await.unwrap());
    }

    pub async fn login(&self, email: &str, password: &str) -> Value {
        let r = self
            .post("/auth/login", json!({"email": email, "password": password}))
            .await;
        assert_eq!(r.status(), StatusCode::OK, "login failed");
        let v: Value = r.json().await.unwrap();
        if v["mfa_required"] == json!(false) {
            self.refresh_csrf().await;
        }
        v
    }

    pub async fn get(&self, path: &str) -> Response {
        self.client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
    }

    pub async fn post(&self, path: &str, body: Value) -> Response {
        self.client
            .post(format!("{}{path}", self.base))
            .header("x-csrf-token", self.csrf())
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    pub async fn put(&self, path: &str, body: Value) -> Response {
        self.client
            .put(format!("{}{path}", self.base))
            .header("x-csrf-token", self.csrf())
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    pub async fn put_if_match(&self, path: &str, rev: i64, body: Value) -> Response {
        self.client
            .put(format!("{}{path}", self.base))
            .header("x-csrf-token", self.csrf())
            .header("if-match", format!("\"{rev}\""))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    pub async fn patch(&self, path: &str, body: Value) -> Response {
        self.client
            .patch(format!("{}{path}", self.base))
            .header("x-csrf-token", self.csrf())
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    pub async fn delete(&self, path: &str) -> Response {
        self.client
            .delete(format!("{}{path}", self.base))
            .header("x-csrf-token", self.csrf())
            .send()
            .await
            .unwrap()
    }

    pub async fn upload_png(&self, path: &str, bytes: Vec<u8>) -> Response {
        self.client
            .post(format!("{}{path}", self.base))
            .header("x-csrf-token", self.csrf())
            .header("content-type", "image/png")
            .body(bytes)
            .send()
            .await
            .unwrap()
    }
}

pub fn png(w: u32, h: u32) -> Vec<u8> {
    use image::{ImageFormat, Rgba, RgbaImage};
    let img = RgbaImage::from_fn(w, h, |x, y| {
        Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}
