-- Phase 1 schema. Every table from §1 of the main plan.
-- Migrations are append-only once released.

CREATE TABLE profile (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    name        TEXT NOT NULL DEFAULT '',
    email       TEXT NOT NULL DEFAULT '',
    phone       TEXT,
    location    TEXT,
    summary     TEXT,
    archetypes  TEXT,
    preferences TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE cv_entries (
    id            INTEGER PRIMARY KEY,
    profile_id    INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL,
    title         TEXT NOT NULL,
    organization  TEXT,
    start_date    TEXT,
    end_date      TEXT,
    description   TEXT,
    metadata      TEXT,
    sort_order    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE job_descriptions (
    id            INTEGER PRIMARY KEY,
    title         TEXT NOT NULL,
    company       TEXT NOT NULL,
    source_url    TEXT NOT NULL,
    raw_text      TEXT NOT NULL,
    fetched_at    TEXT NOT NULL,
    fetch_tier    TEXT NOT NULL,
    external_id   TEXT,
    content_hash  TEXT
);
CREATE UNIQUE INDEX ux_jd_source_external
    ON job_descriptions(source_url, external_id)
    WHERE external_id IS NOT NULL;
CREATE UNIQUE INDEX ux_jd_source_content_hash
    ON job_descriptions(source_url, content_hash)
    WHERE content_hash IS NOT NULL;

CREATE TABLE applications (
    id                    INTEGER PRIMARY KEY,
    job_description_id    INTEGER NOT NULL REFERENCES job_descriptions(id) ON DELETE CASCADE,
    status                TEXT NOT NULL DEFAULT 'discovered',
    applied_at            TEXT,
    notes                 TEXT,
    match_score           REAL,
    evaluation_report_id  INTEGER,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);
CREATE INDEX ix_applications_status ON applications(status);
CREATE INDEX ix_applications_jd ON applications(job_description_id);

CREATE TABLE evaluation_reports (
    id              INTEGER PRIMARY KEY,
    application_id  INTEGER NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    content         TEXT NOT NULL,
    generated_at    TEXT NOT NULL,
    model_used      TEXT NOT NULL,
    prompt_version  INTEGER,
    prompt_hash     TEXT
);

CREATE TABLE cover_letters (
    id              INTEGER PRIMARY KEY,
    application_id  INTEGER NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    content         TEXT NOT NULL,
    generated_at    TEXT NOT NULL,
    model_used      TEXT NOT NULL,
    prompt_version  INTEGER,
    prompt_hash     TEXT
);

CREATE TABLE templates (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('cv', 'cover_letter')),
    content       TEXT NOT NULL,
    version       INTEGER NOT NULL DEFAULT 1,
    content_hash  TEXT NOT NULL,
    is_system     INTEGER NOT NULL DEFAULT 0,
    supersedes_id INTEGER REFERENCES templates(id),
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE document_renders (
    id              INTEGER PRIMARY KEY,
    application_id  INTEGER REFERENCES applications(id) ON DELETE SET NULL,
    kind            TEXT NOT NULL,
    backend_used    TEXT NOT NULL,
    template_id     INTEGER REFERENCES templates(id),
    file_path       TEXT NOT NULL,
    rendered_at     TEXT NOT NULL
);

CREATE TABLE portals (
    id                INTEGER PRIMARY KEY,
    company_name      TEXT NOT NULL,
    url               TEXT NOT NULL,
    render_mode       TEXT NOT NULL CHECK (render_mode IN ('static', 'browser')),
    scraper_config    TEXT,
    last_verified_at  TEXT
);

CREATE TABLE prompts (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL,
    category      TEXT NOT NULL,
    content       TEXT NOT NULL,
    is_system     INTEGER NOT NULL DEFAULT 0,
    version       INTEGER NOT NULL DEFAULT 1,
    content_hash  TEXT NOT NULL,
    supersedes_id INTEGER REFERENCES prompts(id),
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX ix_prompts_category ON prompts(category);
CREATE INDEX ix_prompts_name_version ON prompts(name, version DESC);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);