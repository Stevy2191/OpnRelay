// SPDX-License-Identifier: AGPL-3.0-only
use std::sync::Arc;

use opnrelay_crypto::SecretKey;
use sqlx::PgPool;

use crate::config::Config;
use crate::ratelimit::RateLimiter;
use crate::storage::Storage;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cfg: Arc<Config>,
    pub storage: Storage,
    pub limiter: Arc<RateLimiter>,
    /// Key for encrypting secrets stored in the database (TOTP seeds...).
    pub secrets_key: Arc<SecretKey>,
}

impl AppState {
    pub fn new(db: PgPool, cfg: Config) -> Self {
        let storage = Storage::new(cfg.storage_dir.clone(), cfg.master_key.clone());
        let secrets_key = Arc::new(cfg.master_key.derive("db-secrets"));
        Self {
            db,
            storage,
            limiter: Arc::new(RateLimiter::default()),
            secrets_key,
            cfg: Arc::new(cfg),
        }
    }
}
