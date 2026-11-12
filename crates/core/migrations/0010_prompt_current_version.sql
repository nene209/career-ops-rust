-- Phase 3.4: a current-version pointer and retirement.
--
-- Prompt content is immutable: a version row is never rewritten. Activation is
-- a separate flag, so "revert" moves the pointer instead of appending a copy of
-- an old version's content (which duplicated the content hash and grew the
-- table). `retired_at` hides a version from the picker without breaking
-- `get_by_hash`, which is how an evaluation, cover letter, or triage row
-- resolves the text that produced it.
ALTER TABLE prompts ADD COLUMN is_current INTEGER NOT NULL DEFAULT 0;
ALTER TABLE prompts ADD COLUMN retired_at TEXT;

-- Before this migration the newest version was the one in use. Preserve that
-- exactly: one current row per (name, is_system).
UPDATE prompts
SET is_current = 1
WHERE id IN (
    SELECT p.id FROM prompts p
    WHERE p.version = (SELECT MAX(q.version) FROM prompts q
                       WHERE q.name = p.name AND q.is_system = p.is_system)
);

-- The invariant is a database rule, not a convention: a thread has exactly one
-- current version. The partial index only constrains current rows.
CREATE UNIQUE INDEX ux_prompts_current_per_thread
    ON prompts(name, is_system) WHERE is_current = 1;
