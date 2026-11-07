-- Phase 3: evaluation provenance and a uniqueness guard on prompt versions.
--
-- The structured fields are columns, not JSON inside `content`, so history and
-- sorting do not have to re-parse a markdown report. `truncated` records that
-- the prompt received a cut job text, which is the difference between "the
-- model was weak" and "the model saw a card teaser".
ALTER TABLE evaluation_reports ADD COLUMN match_score REAL;
ALTER TABLE evaluation_reports ADD COLUMN recommendation TEXT;
ALTER TABLE evaluation_reports ADD COLUMN input_chars INTEGER;
ALTER TABLE evaluation_reports ADD COLUMN source_chars INTEGER;
ALTER TABLE evaluation_reports ADD COLUMN truncated INTEGER NOT NULL DEFAULT 0;

-- Cover letters record the same input size, for the same reason.
ALTER TABLE cover_letters ADD COLUMN input_chars INTEGER;

-- Two rows may share a name and version only when their owners differ: a user
-- copy of `evaluate` v2 and a shipped system `evaluate` v2 are different
-- prompts. Same-owner duplicates cannot arise from the scoped `create_version`
-- (`crates/core/tests/prompts_repo.rs` proves the seeded set and a user copy
-- satisfy this), so the index can be created without a repair step.
CREATE UNIQUE INDEX ux_prompts_name_owner_version ON prompts(name, is_system, version);
