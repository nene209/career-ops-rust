-- Phase 3.3: the Pipeline orders by the discovery time, newest first.
--
-- `fetched_at` alone leaves ties: one fetch inserts many rows in the same
-- second, and two loads could order them differently. `id DESC` is the
-- tie-break, so this index matches the exact `ORDER BY` the Pipeline uses
-- instead of sorting the table on every load.
CREATE INDEX ix_job_descriptions_fetched_at
    ON job_descriptions(fetched_at DESC, id DESC);
