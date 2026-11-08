-- Phase 3.2: the triage provenance on each posting.
--
-- `queue_state` and `queue_reason` (migration 0005) are what the Pipeline
-- reads. These columns are the record of what the sorter said: which decision,
-- with which reason, from which model, with which prompt, and when. Same
-- discipline as `evaluation_reports.prompt_version`; a prompt change must not
-- rewrite history.
--
-- Every column is nullable: a row that has never been triaged keeps NULL, and
-- there is no backfill.
ALTER TABLE job_descriptions ADD COLUMN triage_decision TEXT;
ALTER TABLE job_descriptions ADD COLUMN triage_reason TEXT;
ALTER TABLE job_descriptions ADD COLUMN triage_model TEXT;
ALTER TABLE job_descriptions ADD COLUMN triage_prompt_hash TEXT;
ALTER TABLE job_descriptions ADD COLUMN triaged_at TEXT;
