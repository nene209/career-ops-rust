-- Phase 2.9: the pre-evaluation queue.
--
-- `applications.status` is the application lifecycle (discovered → applied …).
-- This is a different axis: what the user wants to do with a posting *before*
-- any evaluation exists. A row can be `new` and have no application at all.
--
-- `new` is correct for every existing row, so there is no backfill. The index
-- makes the queue filter cheap inside the 500-row Pipeline page.
ALTER TABLE job_descriptions ADD COLUMN queue_state TEXT NOT NULL DEFAULT 'new';
ALTER TABLE job_descriptions ADD COLUMN queue_reason TEXT;
ALTER TABLE job_descriptions ADD COLUMN queued_at TEXT;

CREATE INDEX ix_job_descriptions_queue_state ON job_descriptions(queue_state);
