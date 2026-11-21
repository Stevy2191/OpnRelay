-- Entra group sync: per-provider sync state, group-granted roles, role/disable provenance.
ALTER TABLE identity_providers
    ADD COLUMN group_sync_enabled boolean NOT NULL DEFAULT false,
    ADD COLUMN group_sync_last_at timestamptz,
    ADD COLUMN group_sync_last_ok_at timestamptz,
    ADD COLUMN group_sync_last_error text,
    ADD COLUMN group_sync_failed_logged_at timestamptz,
    ADD COLUMN group_sync_brake_pending integer;

ALTER TABLE groups
    ADD COLUMN grants_role text CHECK (grants_role IN ('admin', 'auditor')),
    ADD COLUMN missing_since timestamptz,
    ADD COLUMN last_synced_at timestamptz;
CREATE UNIQUE INDEX groups_idp_external ON groups (org_id, external_id) WHERE source = 'idp';

ALTER TABLE users
    ADD COLUMN role_source text NOT NULL DEFAULT 'manual' CHECK (role_source IN ('manual', 'idp')),
    ADD COLUMN disabled_by text CHECK (disabled_by IN ('admin', 'microsoft'));
