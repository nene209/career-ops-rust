-- Phase 2.5 identity: a canonical URL for deduplication.
--
-- The indexes are deliberately NOT unique. A database written before this
-- migration can already contain two rows that canonicalize to the same
-- identity; making the index unique would fail the migration instead of
-- leaving the user's duplicate for them to delete. `job_descriptions::upsert`
-- enforces one row per identity on write; these indexes only make that lookup
-- cheap.
ALTER TABLE job_descriptions ADD COLUMN canonical_url TEXT;

CREATE INDEX ix_jd_canonical_external
    ON job_descriptions(canonical_url, external_id);

CREATE INDEX ix_jd_canonical_hash
    ON job_descriptions(canonical_url, content_hash);
