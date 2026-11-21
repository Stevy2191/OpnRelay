// SPDX-License-Identifier: AGPL-3.0-only
//! Tamper-evident audit log.
//!
//! Each event stores `hash = SHA-256(prev_hash || canonical_json(event))`.
//! Writers serialise on a transaction-scoped advisory lock so the chain is
//! strictly linear. The table itself rejects UPDATE/DELETE/TRUNCATE.

use std::net::IpAddr;

use serde::Serialize;
use serde_json::{Value, json};
use sqlx::types::ipnetwork::IpNetwork;
use sqlx::{PgConnection, PgPool};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

const AUDIT_LOCK_KEY: i64 = 0x4f70_6e52_656c_6179; // "OpnRelay"
pub const GENESIS: [u8; 32] = [0u8; 32];

/// Who performed an action.
#[derive(Debug, Clone, Default)]
pub struct AuditActor {
    pub id: Option<Uuid>,
    pub label: String,
    pub ip: Option<IpAddr>,
}

impl AuditActor {
    pub fn system() -> Self {
        Self {
            id: None,
            label: "system".into(),
            ip: None,
        }
    }

    pub fn anonymous(ip: Option<IpAddr>) -> Self {
        Self {
            id: None,
            label: "anonymous".into(),
            ip,
        }
    }
}

/// An event to append.
#[derive(Debug, Clone)]
pub struct Event<'a> {
    pub action: &'a str,
    pub target_type: &'a str,
    pub target_id: Option<Uuid>,
    pub metadata: Value,
}

impl<'a> Event<'a> {
    pub fn new(action: &'a str) -> Self {
        Self {
            action,
            target_type: "",
            target_id: None,
            metadata: json!({}),
        }
    }

    pub fn target(mut self, target_type: &'a str, id: Uuid) -> Self {
        self.target_type = target_type;
        self.target_id = Some(id);
        self
    }

    pub fn meta(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}

fn now_micros() -> OffsetDateTime {
    let now = OffsetDateTime::now_utc();
    let micros = now.nanosecond() / 1_000;
    now.replace_nanosecond(micros * 1_000)
        .expect("valid nanosecond")
}

#[allow(clippy::too_many_arguments)]
fn canonical(
    ts: OffsetDateTime,
    actor_id: Option<Uuid>,
    actor_label: &str,
    actor_ip: Option<IpAddr>,
    action: &str,
    target_type: &str,
    target_id: Option<Uuid>,
    metadata: &Value,
) -> Vec<u8> {
    // serde_json's Map is a BTreeMap (no preserve_order), so keys are sorted
    // and the encoding is deterministic.
    let v = json!({
        "ts": ts.format(&Rfc3339).expect("formattable"),
        "actor_id": actor_id,
        "actor_label": actor_label,
        "actor_ip": actor_ip.map(|i| i.to_string()),
        "action": action,
        "target_type": target_type,
        "target_id": target_id,
        "metadata": metadata,
    });
    serde_json::to_vec(&v).expect("json encodes")
}

fn chain_hash(prev: &[u8], canonical: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(prev.len() + canonical.len());
    buf.extend_from_slice(prev);
    buf.extend_from_slice(canonical);
    opnrelay_crypto::sha256(&buf).to_vec()
}

/// Append an event. **Must** be called inside a transaction (the advisory
/// lock is transaction scoped).
/// Take the audit chain lock (held to the end of the transaction). Code that
/// both locks rows it will audit about and records audit events takes this
/// first, so every path acquires locks in the same order: audit chain, then rows.
pub async fn lock(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(AUDIT_LOCK_KEY)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn record(
    conn: &mut PgConnection,
    actor: &AuditActor,
    ev: Event<'_>,
) -> sqlx::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(AUDIT_LOCK_KEY)
        .execute(&mut *conn)
        .await?;
    let prev: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT hash FROM audit_events ORDER BY seq DESC LIMIT 1")
            .fetch_optional(&mut *conn)
            .await?;
    let prev = prev.unwrap_or_else(|| GENESIS.to_vec());
    let ts = now_micros();
    let c = canonical(
        ts,
        actor.id,
        &actor.label,
        actor.ip,
        ev.action,
        ev.target_type,
        ev.target_id,
        &ev.metadata,
    );
    let hash = chain_hash(&prev, &c);
    sqlx::query(
        "INSERT INTO audit_events (ts, actor_id, actor_label, actor_ip, action, target_type, target_id, metadata, prev_hash, hash)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(ts)
    .bind(actor.id)
    .bind(&actor.label)
    .bind(actor.ip.map(IpNetwork::from))
    .bind(ev.action)
    .bind(ev.target_type)
    .bind(ev.target_id)
    .bind(&ev.metadata)
    .bind(&prev)
    .bind(&hash)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Append an event in its own transaction.
pub async fn record_now(pool: &PgPool, actor: &AuditActor, ev: Event<'_>) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    record(&mut tx, actor, ev).await?;
    tx.commit().await
}

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct AuditRow {
    pub seq: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    pub actor_id: Option<Uuid>,
    pub actor_label: String,
    #[serde(serialize_with = "ser_ip")]
    pub actor_ip: Option<IpNetwork>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub metadata: Value,
    #[serde(serialize_with = "ser_hex")]
    pub prev_hash: Vec<u8>,
    #[serde(serialize_with = "ser_hex")]
    pub hash: Vec<u8>,
}

fn ser_ip<S: serde::Serializer>(ip: &Option<IpNetwork>, s: S) -> Result<S::Ok, S::Error> {
    match ip {
        Some(ip) => s.serialize_str(&ip.ip().to_string()),
        None => s.serialize_none(),
    }
}

fn ser_hex<S: serde::Serializer>(b: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex::encode(b))
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct VerifyReport {
    pub ok: bool,
    pub events_checked: u64,
    pub first_bad_seq: Option<i64>,
    pub head_hash: Option<String>,
}

/// Recompute the whole chain and report the first broken link, if any.
pub async fn verify(pool: &PgPool) -> sqlx::Result<VerifyReport> {
    use futures_util::TryStreamExt;
    let mut rows = sqlx::query_as::<_, AuditRow>(
        "SELECT seq, ts, actor_id, actor_label, actor_ip, action, target_type, target_id, metadata, prev_hash, hash
         FROM audit_events ORDER BY seq",
    )
    .fetch(pool);
    let mut prev = GENESIS.to_vec();
    let mut n = 0u64;
    while let Some(r) = rows.try_next().await? {
        n += 1;
        let c = canonical(
            r.ts,
            r.actor_id,
            &r.actor_label,
            r.actor_ip.map(|i| i.ip()),
            &r.action,
            &r.target_type,
            r.target_id,
            &r.metadata,
        );
        let expected = chain_hash(&prev, &c);
        if r.prev_hash != prev || r.hash != expected {
            return Ok(VerifyReport {
                ok: false,
                events_checked: n,
                first_bad_seq: Some(r.seq),
                head_hash: None,
            });
        }
        prev = r.hash;
    }
    Ok(VerifyReport {
        ok: true,
        events_checked: n,
        first_bad_seq: None,
        head_hash: (n > 0).then(|| hex::encode(&prev)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_is_key_sorted_and_stable() {
        let ts = OffsetDateTime::UNIX_EPOCH;
        let m = json!({"z": 1, "a": "x"});
        let c1 = canonical(ts, None, "sys", None, "x.y", "", None, &m);
        let s = String::from_utf8(c1.clone()).unwrap();
        assert!(s.starts_with("{\"action\""));
        assert!(s.contains("{\"a\":\"x\",\"z\":1}"));
        let c2 = canonical(ts, None, "sys", None, "x.y", "", None, &m);
        assert_eq!(chain_hash(&GENESIS, &c1), chain_hash(&GENESIS, &c2));
    }
}
