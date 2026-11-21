// SPDX-License-Identifier: AGPL-3.0-only
//! Microsoft identity platform (Entra ID) helpers: app configuration,
//! authorisation URLs, token exchange and ID token checks.
//!
//! ID tokens are received directly from the token endpoint over TLS, so per
//! OpenID Connect Core §3.1.3.7 the TLS connection authenticates the issuer;
//! we still verify issuer, audience, tenant, expiry and nonce.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const PROVIDER_NAME: &str = "microsoft";
pub const LOGIN_SCOPE: &str = "openid profile email";
pub const ONEDRIVE_SCOPE: &str = "offline_access User.Read Files.ReadWrite";

/// Secrets stored encrypted in `identity_providers.config_enc`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub tenant_id: String,
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub id: Uuid,
    pub enabled: bool,
    pub jit_provisioning: bool,
    pub default_role: String,
    pub group_sync_enabled: bool,
    pub app: AppConfig,
}

#[derive(sqlx::FromRow)]
struct ProviderRow {
    id: Uuid,
    enabled: bool,
    jit_provisioning: bool,
    default_role: String,
    group_sync_enabled: bool,
    config_enc: Vec<u8>,
}

pub fn seal_config(st: &AppState, id: Uuid, app: &AppConfig) -> ApiResult<Vec<u8>> {
    let json = serde_json::to_vec(app).map_err(anyhow::Error::from)?;
    Ok(
        opnrelay_crypto::seal(&st.secrets_key, &json, id.as_bytes())
            .map_err(anyhow::Error::from)?,
    )
}

/// The organisation's Microsoft app configuration, if any.
pub async fn load(st: &AppState, db: &PgPool, org: Uuid) -> ApiResult<Option<Provider>> {
    let row: Option<ProviderRow> = sqlx::query_as(
        "SELECT id, enabled, jit_provisioning, default_role, group_sync_enabled, config_enc FROM identity_providers
         WHERE org_id = $1 AND name = $2",
    )
    .bind(org)
    .bind(PROVIDER_NAME)
    .fetch_optional(db)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let json = opnrelay_crypto::open(&st.secrets_key, &r.config_enc, r.id.as_bytes())
        .map_err(anyhow::Error::from)?;
    Ok(Some(Provider {
        id: r.id,
        enabled: r.enabled,
        jit_provisioning: r.jit_provisioning,
        default_role: r.default_role,
        group_sync_enabled: r.group_sync_enabled,
        app: serde_json::from_slice(&json).map_err(anyhow::Error::from)?,
    }))
}

pub fn valid_tenant(t: &str) -> bool {
    let t = t.trim();
    let generic = ["common", "organizations", "consumers"].contains(&t.to_lowercase().as_str());
    !generic
        && !t.is_empty()
        && t.len() <= 100
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

pub fn is_guid(s: &str) -> bool {
    Uuid::parse_str(s.trim()).is_ok()
}

pub fn redirect_uri(st: &AppState) -> String {
    format!("{}/api/v1/auth/sso/microsoft/callback", st.cfg.public_url)
}

fn enc(s: &str) -> String {
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

pub fn authorize_url(
    st: &AppState,
    app: &AppConfig,
    scope: &str,
    state: &str,
    nonce: &str,
    challenge: &str,
    login_hint: Option<&str>,
) -> String {
    let mut url = format!(
        "{}/{}/oauth2/v2.0/authorize?client_id={}&response_type=code&response_mode=query&redirect_uri={}&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        st.cfg.ms_login_url,
        enc(&app.tenant_id),
        enc(&app.client_id),
        enc(&redirect_uri(st)),
        enc(scope),
        enc(state),
        enc(nonce),
        enc(challenge),
    );
    if let Some(h) = login_hint {
        url.push_str(&format!("&login_hint={}", enc(h)));
    }
    url
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: Option<String>,
    pub id_token: Option<String>,
    pub refresh_token: Option<String>,
}

async fn token_request(
    st: &AppState,
    app: &AppConfig,
    form: &[(&str, &str)],
) -> ApiResult<TokenResponse> {
    let mut params = vec![
        ("client_id", app.client_id.as_str()),
        ("client_secret", app.client_secret.as_str()),
    ];
    params.extend_from_slice(form);
    let res = reqwest::Client::new()
        .post(format!(
            "{}/{}/oauth2/v2.0/token",
            st.cfg.ms_login_url, app.tenant_id
        ))
        .form(&params)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| ApiError::bad(format!("could not reach Microsoft: {e}")))?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = body["error_description"]
            .as_str()
            .or(body["error"].as_str())
            .unwrap_or("token request failed")
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        return Err(ApiError::bad(format!(
            "Microsoft rejected the sign-in: {msg}"
        )));
    }
    serde_json::from_value(body).map_err(|e| ApiError::Internal(e.into()))
}

pub async fn exchange_code(
    st: &AppState,
    app: &AppConfig,
    code: &str,
    verifier: &str,
    scope: &str,
) -> ApiResult<TokenResponse> {
    let redirect = redirect_uri(st);
    token_request(
        st,
        app,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &redirect),
            ("code_verifier", verifier),
            ("scope", scope),
        ],
    )
    .await
}

pub async fn refresh(
    st: &AppState,
    app: &AppConfig,
    refresh_token: &str,
    scope: &str,
) -> ApiResult<TokenResponse> {
    token_request(
        st,
        app,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", scope),
        ],
    )
    .await
}

/// Verified identity from an ID token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub subject: String,
    pub tenant: String,
    pub email: String,
    pub name: String,
}

/// Check an ID token's claims. `now` is unix seconds.
pub fn verify_id_token(
    token: &str,
    login_url: &str,
    app: &AppConfig,
    nonce: &str,
    now: i64,
) -> Result<Identity, String> {
    let payload = token.split('.').nth(1).ok_or("malformed ID token")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| "malformed ID token")?;
    let c: Value = serde_json::from_slice(&bytes).map_err(|_| "malformed ID token")?;
    let tid = c["tid"].as_str().ok_or("ID token has no tenant")?;
    if c["iss"].as_str() != Some(format!("{login_url}/{tid}/v2.0").as_str()) {
        return Err("ID token issuer is not Microsoft".into());
    }
    if is_guid(&app.tenant_id) && !tid.eq_ignore_ascii_case(app.tenant_id.trim()) {
        return Err("this Microsoft account belongs to a different organisation".into());
    }
    let aud_ok = match &c["aud"] {
        Value::String(a) => a == &app.client_id,
        Value::Array(a) => a.iter().any(|v| v.as_str() == Some(app.client_id.as_str())),
        _ => false,
    };
    if !aud_ok {
        return Err("ID token was issued for another application".into());
    }
    if c["exp"].as_i64().is_none_or(|exp| exp < now - 60) {
        return Err("ID token has expired".into());
    }
    if c["nonce"].as_str() != Some(nonce) {
        return Err("ID token nonce mismatch".into());
    }
    let subject = c["oid"]
        .as_str()
        .or(c["sub"].as_str())
        .ok_or("ID token has no user id")?;
    let email = c["email"]
        .as_str()
        .or(c["preferred_username"].as_str())
        .or(c["upn"].as_str())
        .ok_or("the Microsoft account has no email address")?
        .trim()
        .to_lowercase();
    let name = c["name"].as_str().unwrap_or(&email).trim().to_string();
    Ok(Identity {
        subject: subject.to_string(),
        tenant: tid.to_string(),
        email,
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tok(claims: Value) -> String {
        format!(
            "e30.{}.sig",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    fn app() -> AppConfig {
        AppConfig {
            tenant_id: "11111111-1111-1111-1111-111111111111".into(),
            client_id: "22222222-2222-2222-2222-222222222222".into(),
            client_secret: "s".into(),
        }
    }

    fn good() -> Value {
        json!({"iss": "https://login.microsoftonline.com/11111111-1111-1111-1111-111111111111/v2.0",
               "tid": "11111111-1111-1111-1111-111111111111", "aud": "22222222-2222-2222-2222-222222222222",
               "exp": 2_000_000_000i64, "nonce": "n1", "oid": "abc", "preferred_username": "Jo@Acme.com", "name": "Jo"})
    }

    #[test]
    fn accepts_valid_and_rejects_tampered_tokens() {
        let base = "https://login.microsoftonline.com";
        let id = verify_id_token(&tok(good()), base, &app(), "n1", 1_900_000_000).unwrap();
        assert_eq!(id.email, "jo@acme.com");
        assert_eq!(id.subject, "abc");
        for (k, v) in [
            ("aud", json!("other")),
            ("nonce", json!("n2")),
            ("exp", json!(1)),
            ("tid", json!("33333333-3333-3333-3333-333333333333")),
            ("iss", json!("https://evil.example/x/v2.0")),
        ] {
            let mut c = good();
            c[k] = v;
            assert!(
                verify_id_token(&tok(c), base, &app(), "n1", 1_900_000_000).is_err(),
                "{k} must be checked"
            );
        }
        assert!(verify_id_token("garbage", base, &app(), "n1", 0).is_err());
    }

    #[test]
    fn tenant_validation() {
        assert!(valid_tenant("contoso.onmicrosoft.com"));
        assert!(valid_tenant("11111111-1111-1111-1111-111111111111"));
        assert!(!valid_tenant("common"));
        assert!(!valid_tenant("organizations"));
        assert!(!valid_tenant("a/b"));
    }
}
