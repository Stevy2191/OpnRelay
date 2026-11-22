-- Needs-review flag: per-workspace review period (NULL = off) and the time a
-- guide was last confirmed accurate (publish or "Still accurate").
ALTER TABLE workspaces
    ADD COLUMN review_period_days integer DEFAULT 90
    CHECK (review_period_days BETWEEN 1 AND 3650);

ALTER TABLE guides ADD COLUMN reviewed_at timestamptz;

-- Existing published guides start their review clock at publication.
UPDATE guides SET reviewed_at = published_at WHERE published_at IS NOT NULL;
