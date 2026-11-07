-- Phase 2 schema. add last fetch time/last failure cause to portals

ALTER TABLE portals ADD COLUMN last_fetch_attempt_at TEXT;
ALTER TABLE portals ADD COLUMN last_failure_cause TEXT;