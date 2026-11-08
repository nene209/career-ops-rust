-- Phase 3.1: one row per provider call.
--
-- The row is a ledger entry, not a child of the job: `application_id` and
-- `job_description_id` are references without a foreign key, so deleting a job
-- cannot delete the record of the tokens it cost.
--
-- `input_tokens`, `output_tokens`, and the rest are NULL when the provider did
-- not report them. `source` says whether the counters came from the provider
-- (`reported`) or from the four-characters-per-token estimate (`estimated`).
-- `cost_usd` is NULL when no price is known; it is never a stand-in zero.
--
-- `task`, `source`, and `outcome` are free text so a later build can add a
-- value without a migration; an older build reads an unknown value as its
-- weaker variant instead of failing.
CREATE TABLE llm_usage (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    provider               TEXT    NOT NULL,
    model                  TEXT    NOT NULL,
    task                   TEXT    NOT NULL,
    application_id         INTEGER,
    job_description_id     INTEGER,
    input_tokens           INTEGER,
    output_tokens          INTEGER,
    total_tokens           INTEGER,
    cached_input_tokens    INTEGER,
    reasoning_tokens       INTEGER,
    source                 TEXT    NOT NULL,
    cost_usd               REAL,
    -- 1 when `cost_usd` is the whole charge, 0 when it is a lower bound,
    -- NULL when there is no cost at all.
    cost_complete          INTEGER,
    outcome                TEXT    NOT NULL,
    error_kind             TEXT,
    duration_ms            INTEGER NOT NULL,
    created_at             TEXT    NOT NULL
);

CREATE INDEX idx_llm_usage_created_at ON llm_usage(created_at);
CREATE INDEX idx_llm_usage_provider_model ON llm_usage(provider, model);
CREATE INDEX idx_llm_usage_task ON llm_usage(task);
