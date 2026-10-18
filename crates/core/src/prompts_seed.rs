//! System prompt seeding.
//!
//! The original `career-ops` project stores its AI logic in Markdown files
//! under `modes/`. This module embeds those files at compile time and seeds
//! them into the `prompts` table on first launch, or inserts them as new
//! versions when the application ships a changed prompt.
//!
//! ## Design
//!
//! - Prompts are embedded with `include_str!` from `crates/core/prompts/`.
//!   A missing file is a **compile error**, not a runtime surprise.
//! - Seeding is idempotent: running it twice with the same binary is a no-op.
//! - When a release ships a changed system prompt, seeding inserts a new
//!   version with `supersedes_id` pointing to the previous one. Historical
//!   evaluation reports that reference the old content hash remain
//!   reproducible.
//! - User prompts (`is_system = false`) are never touched by seeding, even
//!   if they share a name with a system prompt. See `active_system` in
//!   `repo::prompts`.
//!
//! ## Keeping the seed list in sync
//!
//! `SYSTEM_PROMPTS` references files that must exist at compile time. If you
//! add a new prompt to the original repo, add a corresponding `.md` file to
//! `crates/core/prompts/` and a row to `SYSTEM_PROMPTS`. If you choose not to
//! ship a prompt, **comment out its row** — do not leave a missing file
//! referenced.
//!
//! ## Files to copy from the original repo
//!
//! Copy from `github.com/career-ops-hq/career-ops/modes/*.md` into
//! `crates/core/prompts/`. The required-for-Phase-3 subset is:
//!
//!   _shared.md, oferta.md, scan.md, cover.md, pdf.md
//!
//! The remaining prompts are optional for v1 but present in the repo. Comment
//! out any row whose file you did not copy.

use sqlx::SqlitePool;
use tracing::{debug, info, warn};

use crate::error::{CoreError, Result};
use crate::repo::content_hash;
use crate::repo::prompts;

/// A single embedded system prompt.
///
/// The `name` is the file stem of the original `.md` file (e.g. `"oferta"`
/// for `modes/oferta.md`). The `category` maps the prompt to a Phase 3
/// workstream (`evaluate`, `cover_letter`, `scan`, `render`) or marks it as
/// `utility` for prompts that are not yet used in a specific workstream.
pub struct SeedPrompt {
    pub name: &'static str,
    pub category: &'static str,
    pub content: &'static str,
}

/// Embedded system prompts.
///
/// ## Required for Phase 3
///
/// `_shared`, `oferta`, `scan`, `cover`, `pdf`.
///
/// ## Optional
///
/// The remaining entries are present in the original repo and useful in
/// later phases. Comment out any row whose `.md` file you did not copy
/// into `crates/core/prompts/`.
pub const SYSTEM_PROMPTS: &[SeedPrompt] = &[
    // ------------------------------------------------------------------
    // Required for Phase 3 workstreams.
    // ------------------------------------------------------------------
    SeedPrompt {
        name: "_shared",
        category: "shared",
        content: include_str!("../prompts/_shared.md"),
    },
    SeedPrompt {
        name: "oferta",
        category: "evaluate",
        content: include_str!("../prompts/oferta.md"),
    },
    SeedPrompt {
        name: "scan",
        category: "scan",
        content: include_str!("../prompts/scan.md"),
    },
    SeedPrompt {
        name: "cover",
        category: "cover_letter",
        content: include_str!("../prompts/cover.md"),
    },
    SeedPrompt {
        name: "pdf",
        category: "render",
        content: include_str!("../prompts/pdf.md"),
    },

    // ------------------------------------------------------------------
    // Optional — present in the original repo, useful in later phases.
    // Comment out any row whose file you did not copy.
    // ------------------------------------------------------------------
    SeedPrompt {
        name: "ofertas",
        category: "evaluate",
        content: include_str!("../prompts/ofertas.md"),
    },
    SeedPrompt {
        name: "auto-pipeline",
        category: "evaluate",
        content: include_str!("../prompts/auto-pipeline.md"),
    },
    SeedPrompt {
        name: "batch",
        category: "scan",
        content: include_str!("../prompts/batch.md"),
    },
    SeedPrompt {
        name: "pipeline",
        category: "utility",
        content: include_str!("../prompts/pipeline.md"),
    },
    SeedPrompt {
        name: "tracker",
        category: "utility",
        content: include_str!("../prompts/tracker.md"),
    },
    SeedPrompt {
        name: "apply",
        category: "utility",
        content: include_str!("../prompts/apply.md"),
    },
    SeedPrompt {
        name: "email",
        category: "utility",
        content: include_str!("../prompts/email.md"),
    },
    SeedPrompt {
        name: "contacto",
        category: "utility",
        content: include_str!("../prompts/contacto.md"),
    },
    SeedPrompt {
        name: "deep",
        category: "utility",
        content: include_str!("../prompts/deep.md"),
    },
];

/// The outcome of seeding a single prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedOutcome {
    /// No prior system prompt existed; version 1 was created.
    Inserted { version: i64 },
    /// A prior system prompt existed with different content; a new version
    /// was created and the old one was retained.
    Updated { old_version: i64, new_version: i64 },
    /// A prior system prompt existed with identical content; nothing to do.
    Unchanged { version: i64 },
}

/// Public entry point. Seeds `SYSTEM_PROMPTS` into the database.
pub async fn seed_or_update_system_prompts(
    pool: &SqlitePool,
) -> Result<Vec<(&'static str, SeedOutcome)>> {
    seed_or_update(pool, SYSTEM_PROMPTS).await
}

/// Testable entry point. Seeds an arbitrary set of prompts.
///
/// In production only `seed_or_update_system_prompts` is called.
/// Exposed so tests can exercise versioning and category-change behaviour
/// without mutating a `const`.
pub async fn seed_or_update(
    pool: &SqlitePool,
    seeds: &[SeedPrompt],
) -> Result<Vec<(&'static str, SeedOutcome)>> {
    let mut outcomes = Vec::with_capacity(seeds.len());

    for seed in seeds {
        let outcome = seed_one(pool, seed).await?;
        debug!(
            name = seed.name,
            category = seed.category,
            outcome = ?outcome,
            "seeded prompt"
        );
        outcomes.push((seed.name, outcome));
    }

    let inserted = outcomes
        .iter()
        .filter(|(_, o)| matches!(o, SeedOutcome::Inserted { .. }))
        .count();
    let updated = outcomes
        .iter()
        .filter(|(_, o)| matches!(o, SeedOutcome::Updated { .. }))
        .count();
    let unchanged = outcomes
        .iter()
        .filter(|(_, o)| matches!(o, SeedOutcome::Unchanged { .. }))
        .count();

    info!(
        inserted,
        updated,
        unchanged,
        total = outcomes.len(),
        "prompt seeding complete"
    );

    Ok(outcomes)
}

async fn seed_one(pool: &SqlitePool, seed: &SeedPrompt) -> Result<SeedOutcome> {
    let hash = content_hash(seed.content);

    match prompts::active_system(pool, seed.name).await? {
        None => {
            let prompt = prompts::insert_system_prompt(
                pool,
                seed.name,
                seed.category,
                seed.content,
                &hash,
                1,
            )
            .await?;
            Ok(SeedOutcome::Inserted {
                version: prompt.version,
            })
        }
        Some(existing) if existing.content_hash == hash => Ok(SeedOutcome::Unchanged {
            version: existing.version,
        }),
        Some(existing) => {
            if existing.category != seed.category {
                warn!(
                    name = seed.name,
                    old_category = existing.category,
                    new_category = seed.category,
                    "system prompt category changed between releases"
                );
            }
            let prompt = prompts::insert_system_prompt(
                pool,
                seed.name,
                seed.category,
                seed.content,
                &hash,
                existing.version + 1,
            )
            .await?;
            Ok(SeedOutcome::Updated {
                old_version: existing.version,
                new_version: prompt.version,
            })
        }
    }
}

/// Verify that every embedded prompt has valid content.
///
/// Called from tests, not from the runtime path. A failure here means a
/// `.md` file was corrupted or truncated when copied into the repo — for
/// example, a partial copy that left the file empty or a UTF-8 corruption
/// from a text editor that converted line endings non-idiomatically.
pub fn validate_embedded_prompts() -> Result<()> {
    for seed in SYSTEM_PROMPTS {
        if seed.content.trim().is_empty() {
            return Err(CoreError::Internal(
                "embedded prompt is empty — check crates/core/prompts/",
            ));
        }
        if seed.name.is_empty() || seed.category.is_empty() {
            return Err(CoreError::Internal(
                "embedded prompt has empty name or category",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn embedded_prompts_are_valid() {
        validate_embedded_prompts().expect("embedded prompts are malformed");
    }

    #[test]
    fn prompt_names_are_unique() {
        let mut seen = HashSet::new();
        for seed in SYSTEM_PROMPTS {
            assert!(
                seen.insert(seed.name),
                "duplicate prompt name in SYSTEM_PROMPTS: {}",
                seed.name
            );
        }
    }

    #[test]
    fn all_prompts_have_a_known_category() {
        let known = [
            "shared",
            "evaluate",
            "scan",
            "render",
            "cover_letter",
            "utility",
        ];
        for seed in SYSTEM_PROMPTS {
            assert!(
                known.contains(&seed.category),
                "unknown category for prompt {}: {}",
                seed.name,
                seed.category
            );
        }
    }

    #[test]
    fn required_phase3_prompts_are_present() {
        let required = ["_shared", "oferta", "scan", "cover", "pdf"];
        let names: HashSet<&str> = SYSTEM_PROMPTS.iter().map(|s| s.name).collect();
        for name in required {
            assert!(
                names.contains(name),
                "required Phase 3 prompt missing: {}",
                name
            );
        }
    }
}