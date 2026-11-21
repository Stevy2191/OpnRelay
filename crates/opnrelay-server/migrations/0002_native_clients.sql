-- SPDX-License-Identifier: AGPL-3.0-only
-- OAuth 2.0 (authorization code + PKCE) for the desktop recorder and browser
-- extension. Tokens are opaque; only SHA-256 hashes are stored.

CREATE TABLE oauth_codes (
    code_hash       bytea PRIMARY KEY,
    user_id         uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id       text NOT NULL,
    redirect_uri    text NOT NULL,
    code_challenge  text NOT NULL,
    device_name     text NOT NULL DEFAULT '',
    expires_at      timestamptz NOT NULL,
    used_at         timestamptz
);

CREATE TABLE device_grants (
    id                  uuid PRIMARY KEY,
    user_id             uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id           text NOT NULL CHECK (client_id IN ('opnrelay-desktop','opnrelay-extension')),
    device_name         text NOT NULL DEFAULT '',
    access_hash         bytea NOT NULL UNIQUE,
    access_expires_at   timestamptz NOT NULL,
    refresh_hash        bytea NOT NULL UNIQUE,
    refresh_expires_at  timestamptz NOT NULL,
    ip                  inet,
    created_at          timestamptz NOT NULL DEFAULT now(),
    last_used_at        timestamptz NOT NULL DEFAULT now(),
    revoked_at          timestamptz
);
CREATE INDEX device_grants_user ON device_grants(user_id);

-- Previously issued refresh tokens. Presenting one again means the token was
-- copied, so the whole grant is revoked (refresh token reuse detection).
CREATE TABLE device_refresh_history (
    refresh_hash    bytea PRIMARY KEY,
    grant_id        uuid NOT NULL REFERENCES device_grants(id) ON DELETE CASCADE
);
