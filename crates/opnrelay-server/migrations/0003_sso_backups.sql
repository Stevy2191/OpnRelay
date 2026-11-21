-- SPDX-License-Identifier: AGPL-3.0-only
-- Microsoft Entra ID sign-in and scheduled backups (local + OneDrive).

ALTER TABLE sessions ADD COLUMN auth_method text NOT NULL DEFAULT 'password'
    CHECK (auth_method IN ('password', 'sso'));
ALTER TABLE org_policies ADD COLUMN sso_only boolean NOT NULL DEFAULT false;
CREATE TABLE identity_providers (
    id                  uuid PRIMARY KEY,
    org_id              uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    kind                text NOT NULL CHECK (kind IN ('oidc', 'saml')),
    name                text NOT NULL,
    config_enc          bytea NOT NULL,
    enabled             boolean NOT NULL DEFAULT false,
    jit_provisioning    boolean NOT NULL DEFAULT true,
    default_role        text NOT NULL DEFAULT 'member'
                        CHECK (default_role IN ('admin', 'member', 'auditor')),
    group_mapping       jsonb NOT NULL DEFAULT '{}',
    created_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);

CREATE TABLE user_identities (
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    idp_id      uuid NOT NULL REFERENCES identity_providers(id) ON DELETE CASCADE,
    subject     text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (idp_id, subject)
);
CREATE INDEX user_identities_user ON user_identities(user_id);

-- In-flight OAuth redirects to Microsoft (login and OneDrive connection).
CREATE TABLE sso_states (
    state_hash  bytea PRIMARY KEY,
    purpose     text NOT NULL CHECK (purpose IN ('login', 'onedrive')),
    nonce       text NOT NULL,
    verifier    text NOT NULL,
    return_to   text NOT NULL DEFAULT '/',
    user_id     uuid REFERENCES users(id) ON DELETE CASCADE,
    expires_at  timestamptz NOT NULL
);

CREATE TABLE backup_settings (
    org_id              uuid PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    enabled             boolean NOT NULL DEFAULT true,
    schedule_hour       integer NOT NULL DEFAULT 2 CHECK (schedule_hour BETWEEN 0 AND 23),
    keep_local          integer NOT NULL DEFAULT 7 CHECK (keep_local BETWEEN 1 AND 365),
    onedrive_enabled    boolean NOT NULL DEFAULT false,
    onedrive_folder     text NOT NULL DEFAULT 'OpnRelay Backups',
    keep_remote         integer NOT NULL DEFAULT 30 CHECK (keep_remote BETWEEN 1 AND 3650),
    onedrive_token_enc  bytea,
    onedrive_account    text,
    last_run_at         timestamptz,
    last_status         text,
    last_error          text,
    last_file           text,
    last_size           bigint,
    last_remote_at      timestamptz
);
INSERT INTO backup_settings (org_id) SELECT id FROM organizations;
