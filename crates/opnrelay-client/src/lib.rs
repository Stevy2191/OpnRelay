// SPDX-License-Identifier: AGPL-3.0-only
//! Blocking HTTP client for the OpnRelay API, used by the desktop recorder.
//!
//! Sign-in uses OAuth 2.0 authorisation code + PKCE with a loopback redirect
//! (RFC 8252): a one-shot HTTP listener on 127.0.0.1 receives the code after
//! the user approves in their normal browser.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use opnrelay_core::Step;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const CLIENT_ID: &str = "opnrelay-desktop";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("{0}")]
    Api(String),
    #[error("not signed in")]
    NotSignedIn,
    #[error("network error: {0}")]
    Network(String),
    #[error("sign-in failed: {0}")]
    SignIn(String),
}

pub type Result<T> = std::result::Result<T, ClientError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Me {
    pub email: String,
    pub display_name: String,
    pub organization: Org,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Org {
    pub name: String,
    pub default_capture_mode: String,
    pub auto_redaction_default: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub my_permission: String,
}

impl Workspace {
    pub fn can_edit(&self) -> bool {
        self.my_permission == "edit" || self.my_permission == "manage"
    }
}

/// Normalise a server address typed by a user to `scheme://host[:port]`.
pub fn normalize_server(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return None;
    }
    let with_scheme = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("https://{s}")
    };
    let (scheme, rest) = with_scheme.split_once("://")?;
    let host_port = rest.split('/').next()?;
    if host_port.is_empty() || host_port.contains(['@', ' ']) {
        return None;
    }
    let host = host_port.split(':').next()?;
    if scheme == "http" && host != "localhost" && host != "127.0.0.1" {
        return None;
    }
    Some(format!("{scheme}://{host_port}"))
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn random_b64(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b64(&b)
}

/// PKCE verifier and S256 challenge.
pub fn pkce() -> (String, String) {
    let verifier = random_b64(32);
    let challenge = b64(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn api_error(status: u16, body: &str) -> ClientError {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["detail"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("request failed ({status})"));
    ClientError::Api(detail)
}

/// A pending browser sign-in.
pub struct SignIn {
    listener: TcpListener,
    verifier: String,
    state: String,
    redirect: String,
    pub url: String,
}

impl SignIn {
    /// Prepare a sign-in: binds a loopback port and builds the browser URL.
    pub fn start(server: &str, device_name: &str) -> Result<SignIn> {
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|e| ClientError::SignIn(e.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|e| ClientError::SignIn(e.to_string()))?
            .port();
        let redirect = format!("http://127.0.0.1:{port}/callback");
        let (verifier, challenge) = pkce();
        let state = random_b64(16);
        let q = [
            ("client_id", CLIENT_ID),
            ("redirect_uri", &redirect),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            ("device_name", device_name),
        ]
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect::<Vec<_>>()
        .join("&");
        Ok(SignIn {
            url: format!("{server}/#/connect?{q}"),
            listener,
            verifier,
            state,
            redirect,
        })
    }

    /// Wait for the browser to hit the loopback redirect, then exchange the
    /// code for tokens.
    pub fn finish(self, server: &str, timeout: Duration) -> Result<Tokens> {
        self.listener
            .set_nonblocking(true)
            .map_err(|e| ClientError::SignIn(e.to_string()))?;
        let deadline = Instant::now() + timeout;
        let (mut stream, _) = loop {
            match self.listener.accept() {
                Ok(s) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return Err(ClientError::SignIn(
                            "timed out waiting for the browser".into(),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(ClientError::SignIn(e.to_string())),
            }
        };
        stream.set_nonblocking(false).ok();
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut line = String::new();
        BufReader::new(&stream)
            .read_line(&mut line)
            .map_err(|e| ClientError::SignIn(e.to_string()))?;
        let path = line.split_whitespace().nth(1).unwrap_or("");
        let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let param = |name: &str| {
            query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(k, _)| *k == name)
                .map(|(_, v)| decode(v))
        };
        let ok = param("state").as_deref() == Some(self.state.as_str()) && param("code").is_some();
        let body = if ok {
            "<h2>OpnRelay is connected.</h2><p>You can close this tab and return to the recorder.</p>"
        } else {
            "<h2>Sign-in failed.</h2><p>Please try again from the recorder.</p>"
        };
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        if !ok {
            return Err(ClientError::SignIn(
                "the browser returned an invalid response".into(),
            ));
        }
        exchange_code(
            server,
            &param("code").unwrap_or_default(),
            &self.verifier,
            &self.redirect,
        )
    }
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"),
                    16,
                ) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn token_request(server: &str, body: Value) -> Result<Tokens> {
    let mut res = agent()
        .post(&format!("{server}/api/v1/oauth/token"))
        .send_json(&body)
        .map_err(|e| ClientError::Network(e.to_string()))?;
    let status = res.status().as_u16();
    let text = res.body_mut().read_to_string().unwrap_or_default();
    if status != 200 {
        return Err(api_error(status, &text));
    }
    serde_json::from_str(&text).map_err(|e| ClientError::Api(e.to_string()))
}

pub fn exchange_code(server: &str, code: &str, verifier: &str, redirect: &str) -> Result<Tokens> {
    token_request(
        server,
        json!({"grant_type": "authorization_code", "code": code, "code_verifier": verifier,
               "redirect_uri": redirect, "client_id": CLIENT_ID}),
    )
}

/// An authenticated API session. Refreshes its access token automatically;
/// `on_tokens` is called whenever new tokens should be persisted.
pub struct Client {
    pub server: String,
    tokens: Tokens,
    expires_at: Instant,
    agent: ureq::Agent,
    on_tokens: Box<dyn Fn(&Tokens) + Send>,
}

impl Client {
    pub fn new(server: &str, tokens: Tokens, on_tokens: Box<dyn Fn(&Tokens) + Send>) -> Self {
        let expires_at = Instant::now() + Duration::from_secs(tokens.expires_in.saturating_sub(30));
        Client {
            server: server.to_string(),
            tokens,
            expires_at,
            agent: agent(),
            on_tokens,
        }
    }

    /// Resume from a stored refresh token.
    pub fn from_refresh(
        server: &str,
        refresh_token: &str,
        on_tokens: Box<dyn Fn(&Tokens) + Send>,
    ) -> Result<Self> {
        let t = token_request(
            server,
            json!({"grant_type": "refresh_token", "refresh_token": refresh_token}),
        )
        .map_err(|_| ClientError::NotSignedIn)?;
        on_tokens(&t);
        Ok(Client::new(server, t, on_tokens))
    }

    fn access(&mut self) -> Result<String> {
        if Instant::now() >= self.expires_at {
            let t = token_request(
                &self.server,
                json!({"grant_type": "refresh_token", "refresh_token": self.tokens.refresh_token}),
            )
            .map_err(|_| ClientError::NotSignedIn)?;
            (self.on_tokens)(&t);
            self.expires_at = Instant::now() + Duration::from_secs(t.expires_in.saturating_sub(30));
            self.tokens = t;
        }
        Ok(self.tokens.access_token.clone())
    }

    fn handle(&self, res: Result<ureq::http::Response<ureq::Body>>) -> Result<Value> {
        let mut res = res?;
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().unwrap_or_default();
        match status {
            200..=299 => Ok(if text.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&text).unwrap_or(Value::Null)
            }),
            401 => Err(ClientError::NotSignedIn),
            _ => Err(api_error(status, &text)),
        }
    }

    fn get(&mut self, path: &str) -> Result<Value> {
        let token = self.access()?;
        let r = self
            .agent
            .get(&format!("{}/api/v1{path}", self.server))
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| ClientError::Network(e.to_string()));
        self.handle(r)
    }

    fn send_json(&mut self, method: &str, path: &str, body: &Value) -> Result<Value> {
        let token = self.access()?;
        let url = format!("{}/api/v1{path}", self.server);
        let auth = format!("Bearer {token}");
        let r = match method {
            "PUT" => self
                .agent
                .put(&url)
                .header("Authorization", &auth)
                .send_json(body),
            _ => self
                .agent
                .post(&url)
                .header("Authorization", &auth)
                .send_json(body),
        }
        .map_err(|e| ClientError::Network(e.to_string()));
        self.handle(r)
    }

    pub fn me(&mut self) -> Result<Me> {
        serde_json::from_value(self.get("/me")?).map_err(|e| ClientError::Api(e.to_string()))
    }

    pub fn editable_workspaces(&mut self) -> Result<Vec<Workspace>> {
        let all: Vec<Workspace> = serde_json::from_value(self.get("/workspaces")?)
            .map_err(|e| ClientError::Api(e.to_string()))?;
        Ok(all.into_iter().filter(Workspace::can_edit).collect())
    }

    pub fn create_guide(&mut self, workspace_id: &str, title: &str) -> Result<String> {
        let v = self.send_json(
            "POST",
            "/guides",
            &json!({"workspace_id": workspace_id, "title": title, "source": "desktop"}),
        )?;
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ClientError::Api("no guide id".into()))
    }

    pub fn upload_png(&mut self, guide_id: &str, png: &[u8]) -> Result<String> {
        let token = self.access()?;
        let r = self
            .agent
            .post(&format!("{}/api/v1/media?guide_id={guide_id}", self.server))
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", "image/png")
            .send(png)
            .map_err(|e| ClientError::Network(e.to_string()));
        let v = self.handle(r)?;
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ClientError::Api("no media id".into()))
    }

    pub fn put_steps(&mut self, guide_id: &str, steps: &[Step]) -> Result<()> {
        self.send_json(
            "PUT",
            &format!("/guides/{guide_id}/steps"),
            &json!({ "steps": steps }),
        )?;
        Ok(())
    }

    pub fn revoke(&mut self) {
        let _ = self
            .agent
            .post(&format!("{}/api/v1/oauth/revoke", self.server))
            .send_json(json!({"token": self.tokens.refresh_token}));
    }

    pub fn guide_edit_url(&self, guide_id: &str) -> String {
        format!("{}/#/guides/{guide_id}/edit", self.server)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_normalisation() {
        assert_eq!(
            normalize_server("guides.acme.org/x").as_deref(),
            Some("https://guides.acme.org")
        );
        assert_eq!(
            normalize_server("http://localhost:8080/").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(normalize_server("http://evil.org"), None);
        assert_eq!(normalize_server("https://a@b.org"), None);
        assert_eq!(normalize_server("  "), None);
    }

    #[test]
    fn pkce_and_encoding() {
        let (v, c) = pkce();
        assert_eq!(v.len(), 43);
        assert_eq!(c, b64(&Sha256::digest(v.as_bytes())));
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
        assert_eq!(decode("a%20b%2Fc+d"), "a b/c d");
        assert_eq!(decode("bad%zz%"), "bad%zz%");
    }

    #[test]
    fn loopback_rejects_wrong_state() {
        let s = SignIn::start("http://127.0.0.1:9", "test").unwrap();
        assert!(s.url.contains("client_id=opnrelay-desktop"));
        let addr = s.listener.local_addr().unwrap();
        let h = std::thread::spawn(move || s.finish("http://127.0.0.1:9", Duration::from_secs(5)));
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        write!(
            c,
            "GET /callback?code=x&state=wrong HTTP/1.1\r\nHost: x\r\n\r\n"
        )
        .unwrap();
        assert!(matches!(h.join().unwrap(), Err(ClientError::SignIn(_))));
    }
}
