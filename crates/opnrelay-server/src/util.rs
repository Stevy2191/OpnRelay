// SPDX-License-Identifier: AGPL-3.0-only
//! Small shared helpers.

use axum::http::HeaderMap;
use sqlx::types::ipnetwork::IpNetwork;

use crate::error::{ApiError, ApiResult};

pub fn ser_ip<S: serde::Serializer>(ip: &Option<IpNetwork>, s: S) -> Result<S::Ok, S::Error> {
    match ip {
        Some(ip) => s.serialize_str(&ip.ip().to_string()),
        None => s.serialize_none(),
    }
}

/// Parse an optional `If-Match: "<revision>"` header.
pub fn if_match(headers: &HeaderMap) -> ApiResult<Option<i32>> {
    let Some(v) = headers.get(axum::http::header::IF_MATCH) else {
        return Ok(None);
    };
    let s = v.to_str().map_err(|_| ApiError::bad("invalid If-Match"))?;
    let s = s.trim().trim_start_matches("W/").trim_matches('"');
    s.parse()
        .map(Some)
        .map_err(|_| ApiError::bad("If-Match must be a revision number"))
}

/// Trim and bound a user-provided name.
pub fn name(field: &'static str, v: &str, max: usize) -> ApiResult<String> {
    let t = v.trim();
    if t.is_empty() {
        return Err(ApiError::bad(format!("{field} must not be empty")));
    }
    if t.chars().count() > max {
        return Err(ApiError::bad(format!("{field} is too long (max {max})")));
    }
    Ok(t.to_string())
}

pub fn valid_email(e: &str) -> bool {
    let e = e.trim();
    let Some((local, domain)) = e.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && e.len() <= 254
        && !e.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn if_match_parsing() {
        let mut h = HeaderMap::new();
        assert_eq!(if_match(&h).unwrap(), None);
        h.insert("if-match", HeaderValue::from_static("\"7\""));
        assert_eq!(if_match(&h).unwrap(), Some(7));
        h.insert("if-match", HeaderValue::from_static("W/\"8\""));
        assert_eq!(if_match(&h).unwrap(), Some(8));
        h.insert("if-match", HeaderValue::from_static("abc"));
        assert!(if_match(&h).is_err());
    }

    #[test]
    fn emails() {
        assert!(valid_email("a@b.co"));
        assert!(!valid_email("a@b"));
        assert!(!valid_email("ab.co"));
        assert!(!valid_email("a b@c.de"));
    }
}
