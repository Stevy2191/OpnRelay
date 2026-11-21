-- SPDX-License-Identifier: AGPL-3.0-only
-- OpnRelay initial schema.

CREATE EXTENSION IF NOT EXISTS citext;

-- ---------------------------------------------------------------------------
-- Organisation & settings
-- ---------------------------------------------------------------------------
CREATE TABLE organizations (
    id          uuid PRIMARY KEY,
    name        text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE org_policies (
    org_id                  uuid PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    -- off | admins | all
    mfa_requirement         text NOT NULL DEFAULT 'off'
                            CHECK (mfa_requirement IN ('off','admins','all')),
    session_idle_minutes    integer NOT NULL DEFAULT 480 CHECK (session_idle_minutes BETWEEN 5 AND 43200),
    session_absolute_hours  integer NOT NULL DEFAULT 168 CHECK (session_absolute_hours BETWEEN 1 AND 8760),
    public_links_enabled    boolean NOT NULL DEFAULT true,
    default_capture_mode    text NOT NULL DEFAULT 'region'
                            CHECK (default_capture_mode IN ('full','window','region')),
    auto_redaction_default  boolean NOT NULL DEFAULT true,
    redaction_patterns      jsonb NOT NULL DEFAULT '[]',
    retention               jsonb NOT NULL DEFAULT '{}',
    updated_at              timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE branding (
    org_id              uuid PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    logo_media_id       uuid,
    primary_color       text NOT NULL DEFAULT '#1f4fd1',
    accent_color        text NOT NULL DEFAULT '#f59e0b',
    font_family         text NOT NULL DEFAULT 'Inter',
    header_text         text NOT NULL DEFAULT '{company}',
    footer_text         text NOT NULL DEFAULT '{guide_title} · Page {page} of {pages}',
    cover_page          boolean NOT NULL DEFAULT true,
    show_step_numbers   boolean NOT NULL DEFAULT true,
    watermark_exports   boolean NOT NULL DEFAULT false,
    updated_at          timestamptz NOT NULL DEFAULT now()
);

-- ---------------------------------------------------------------------------
-- Identity
-- ---------------------------------------------------------------------------
CREATE TABLE users (
    id              uuid PRIMARY KEY,
    org_id          uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email           citext NOT NULL UNIQUE,
    display_name    text NOT NULL,
    password_hash   text,
    org_role        text NOT NULL DEFAULT 'member'
                    CHECK (org_role IN ('owner','admin','member','auditor')),
    status          text NOT NULL DEFAULT 'active'
                    CHECK (status IN ('invited','active','disabled')),
    failed_logins   integer NOT NULL DEFAULT 0,
    locked_until    timestamptz,
    last_login_at   timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);

CREATE TABLE user_preferences (
    user_id             uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    capture_mode        text CHECK (capture_mode IN ('full','window','region')),
    auto_redaction      boolean,
    include_typed_text  boolean NOT NULL DEFAULT false,
    region_size         integer
);

CREATE TABLE mfa_factors (
    id              uuid PRIMARY KEY,
    user_id         uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind            text NOT NULL CHECK (kind IN ('totp','webauthn')),
    label           text NOT NULL DEFAULT '',
    secret_enc      bytea,
    credential      jsonb,
    confirmed_at    timestamptz,
    last_used_step  bigint,          -- TOTP replay protection
    created_at      timestamptz NOT NULL DEFAULT now(),
    last_used_at    timestamptz
);
CREATE INDEX mfa_factors_user ON mfa_factors(user_id);

CREATE TABLE recovery_codes (
    id          uuid PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash   bytea NOT NULL,
    used_at     timestamptz
);
CREATE INDEX recovery_codes_user ON recovery_codes(user_id);

CREATE TABLE sessions (
    id              uuid PRIMARY KEY,
    user_id         uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      bytea NOT NULL UNIQUE,
    csrf_token      text NOT NULL,
    mfa_pending     boolean NOT NULL DEFAULT false,
    mfa_at          timestamptz,
    ip              inet,
    user_agent      text,
    created_at      timestamptz NOT NULL DEFAULT now(),
    last_seen_at    timestamptz NOT NULL DEFAULT now(),
    expires_at      timestamptz NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);

CREATE TABLE invitations (
    id          uuid PRIMARY KEY,
    org_id      uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  bytea NOT NULL UNIQUE,
    purpose     text NOT NULL DEFAULT 'invite' CHECK (purpose IN ('invite','password_reset')),
    created_by  uuid REFERENCES users(id) ON DELETE SET NULL,
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE groups (
    id          uuid PRIMARY KEY,
    org_id      uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        text NOT NULL,
    source      text NOT NULL DEFAULT 'manual' CHECK (source IN ('manual','idp')),
    external_id text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);

CREATE TABLE group_members (
    group_id    uuid NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);

-- ---------------------------------------------------------------------------
-- Content
-- ---------------------------------------------------------------------------
CREATE TABLE workspaces (
    id          uuid PRIMARY KEY,
    org_id      uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        text NOT NULL,
    description text NOT NULL DEFAULT '',
    legal_hold  boolean NOT NULL DEFAULT false,
    created_by  uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    deleted_at  timestamptz
);

CREATE TABLE workspace_members (
    workspace_id    uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    principal_type  text NOT NULL CHECK (principal_type IN ('user','group')),
    principal_id    uuid NOT NULL,
    role            text NOT NULL CHECK (role IN ('manager','editor','viewer')),
    PRIMARY KEY (workspace_id, principal_type, principal_id)
);
CREATE INDEX workspace_members_principal ON workspace_members(principal_type, principal_id);

CREATE TABLE folders (
    id              uuid PRIMARY KEY,
    workspace_id    uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id       uuid REFERENCES folders(id) ON DELETE CASCADE,
    name            text NOT NULL,
    position        integer NOT NULL DEFAULT 0,
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX folders_workspace ON folders(workspace_id);

CREATE TABLE media (
    id              uuid PRIMARY KEY,
    org_id          uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    kind            text NOT NULL CHECK (kind IN ('image','video','gif','logo','export')),
    mime            text NOT NULL,
    storage_key     text NOT NULL,
    sha256          bytea NOT NULL,
    bytes           bigint NOT NULL,
    width           integer,
    height          integer,
    duration_ms     bigint,
    dek_wrapped     bytea NOT NULL,
    kek_id          text NOT NULL,
    guide_id        uuid,
    filename        text,
    expires_at      timestamptz,
    created_by      uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX media_guide ON media(guide_id);

CREATE TABLE guides (
    id                  uuid PRIMARY KEY,
    workspace_id        uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    folder_id           uuid REFERENCES folders(id) ON DELETE SET NULL,
    title               text NOT NULL,
    description         text NOT NULL DEFAULT '',
    status              text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','published','archived')),
    classification      text NOT NULL DEFAULT 'internal'
                        CHECK (classification IN ('internal','confidential','restricted')),
    source              text NOT NULL DEFAULT 'manual' CHECK (source IN ('desktop','extension','manual')),
    recording_media_id  uuid REFERENCES media(id) ON DELETE SET NULL,
    revision            integer NOT NULL DEFAULT 1,
    current_version     integer NOT NULL DEFAULT 0,
    tags                text[] NOT NULL DEFAULT '{}',
    search              tsvector,
    legal_hold          boolean NOT NULL DEFAULT false,
    created_by          uuid REFERENCES users(id) ON DELETE SET NULL,
    updated_by          uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    published_at        timestamptz,
    deleted_at          timestamptz
);
CREATE INDEX guides_workspace ON guides(workspace_id) WHERE deleted_at IS NULL;
CREATE INDEX guides_search ON guides USING gin(search);

ALTER TABLE media ADD CONSTRAINT media_guide_fk FOREIGN KEY (guide_id) REFERENCES guides(id) ON DELETE SET NULL;

CREATE TABLE steps (
    id          uuid PRIMARY KEY,
    guide_id    uuid NOT NULL REFERENCES guides(id) ON DELETE CASCADE,
    position    integer NOT NULL,
    kind        text NOT NULL CHECK (kind IN ('action','text','heading','tip','alert')),
    title       text NOT NULL DEFAULT '',
    body_md     text NOT NULL DEFAULT '',
    action      jsonb,
    media_id    uuid REFERENCES media(id) ON DELETE SET NULL,
    annotations jsonb NOT NULL DEFAULT '{}',
    UNIQUE (guide_id, position) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE guide_versions (
    guide_id    uuid NOT NULL REFERENCES guides(id) ON DELETE CASCADE,
    version     integer NOT NULL,
    snapshot    jsonb NOT NULL,
    note        text NOT NULL DEFAULT '',
    created_by  uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (guide_id, version)
);

CREATE TABLE share_links (
    id              uuid PRIMARY KEY,
    guide_id        uuid NOT NULL REFERENCES guides(id) ON DELETE CASCADE,
    scope           text NOT NULL CHECK (scope IN ('org','public')),
    token_hash      bytea NOT NULL UNIQUE,
    password_hash   text,
    expires_at      timestamptz,
    allow_download  boolean NOT NULL DEFAULT false,
    view_count      bigint NOT NULL DEFAULT 0,
    created_by      uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    revoked_at      timestamptz
);
CREATE INDEX share_links_guide ON share_links(guide_id);

-- ---------------------------------------------------------------------------
-- Jobs & exports
-- ---------------------------------------------------------------------------
CREATE TABLE jobs (
    id          bigserial PRIMARY KEY,
    kind        text NOT NULL,
    payload     jsonb NOT NULL,
    status      text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','running','done','failed')),
    attempts    integer NOT NULL DEFAULT 0,
    run_after   timestamptz NOT NULL DEFAULT now(),
    locked_at   timestamptz,
    error       text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz
);
CREATE INDEX jobs_ready ON jobs(run_after) WHERE status = 'queued';

CREATE TABLE exports (
    id              uuid PRIMARY KEY,
    guide_id        uuid REFERENCES guides(id) ON DELETE CASCADE,
    requested_by    uuid REFERENCES users(id) ON DELETE SET NULL,
    format          text NOT NULL,
    options         jsonb NOT NULL DEFAULT '{}',
    status          text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','running','done','failed')),
    result_media_id uuid REFERENCES media(id) ON DELETE SET NULL,
    error           text,
    created_at      timestamptz NOT NULL DEFAULT now(),
    finished_at     timestamptz
);

-- ---------------------------------------------------------------------------
-- Audit log (append-only, hash-chained)
-- ---------------------------------------------------------------------------
CREATE TABLE audit_events (
    seq         bigserial PRIMARY KEY,
    ts          timestamptz NOT NULL,
    actor_id    uuid,
    actor_label text NOT NULL DEFAULT '',
    actor_ip    inet,
    action      text NOT NULL,
    target_type text NOT NULL DEFAULT '',
    target_id   uuid,
    metadata    jsonb NOT NULL DEFAULT '{}',
    prev_hash   bytea NOT NULL,
    hash        bytea NOT NULL
);
CREATE INDEX audit_events_ts ON audit_events(ts);
CREATE INDEX audit_events_actor ON audit_events(actor_id);
CREATE INDEX audit_events_target ON audit_events(target_id);

-- Block modification of audit events at the database level, regardless of role.
CREATE FUNCTION audit_events_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit_events is append-only';
END;
$$;
CREATE TRIGGER audit_events_no_update BEFORE UPDATE OR DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_events_immutable();
CREATE TRIGGER audit_events_no_truncate BEFORE TRUNCATE ON audit_events
    FOR EACH STATEMENT EXECUTE FUNCTION audit_events_immutable();
