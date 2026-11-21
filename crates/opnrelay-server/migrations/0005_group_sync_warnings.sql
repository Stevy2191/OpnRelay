-- Non-fatal notes from the last group sync (e.g. an owner disabled in Microsoft).
ALTER TABLE identity_providers
    ADD COLUMN group_sync_last_warning text;
