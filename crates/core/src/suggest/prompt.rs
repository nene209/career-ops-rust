//! The suggestion prompt: the known-site block and strict assembly.
//!
//! The prompt is `crates/core/prompts/suggest_portals.md`, seeded as `suggest`.
//! This module fills its three variables: the CV summary, the acquisition
//! preferences, and the embedded known sites reduced to `name | engine | slug`.
//! The engine and URL rules live in the prompt text; the slug is the only
//! company-specific value the model may copy from here.

use crate::acquisition::config::ScraperConfig;
use crate::acquisition::known_sites::{self, SiteEngine};
use crate::preferences::Preferences;
use crate::prompt_render::{render, RenderError, Variables};
use crate::triage::render_preferences;

use super::output::SuggestEngine;

/// The seeded prompt's name and category.
pub const PROMPT_NAME: &str = "suggest";
pub const PROMPT_CATEGORY: &str = "suggest";

/// The embedded prompt, so its ceiling can be tested without a database.
#[cfg(test)]
const EMBEDDED_PROMPT: &str = include_str!("../../prompts/suggest_portals.md");

/// Render the embedded registry as one `name | engine | slug` line per board.
///
/// Only boards the suggestion contract can rebuild a URL for appear: an ATS
/// entry whose adapter is one of the five engines, and Eightfold, whose slug is
/// the domain in its config. A selector site needs a hand-written config, so it
/// is not something the model can propose.
pub fn render_known_sites() -> String {
    let mut lines: Vec<String> = known_sites::all()
        .iter()
        .filter_map(|site| {
            let engine = SuggestEngine::from_site_engine(site.engine)?;
            let slug = match site.engine {
                SiteEngine::Ats(_) => site.path_prefix?.trim_start_matches('/').to_string(),
                SiteEngine::Provider(_) => {
                    let parsed = ScraperConfig::parse(site.config).ok()?;
                    parsed.eightfold()?.domain.clone()
                }
            };
            Some(format!(
                "- {} | {} | slug: {slug}",
                site.name,
                engine.as_str()
            ))
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

/// Fill the suggestion template with the candidate, the preferences, and the
/// known-site block.
pub fn assemble(
    template: &str,
    cv_summary: &str,
    preferences: &Preferences,
) -> Result<String, RenderError> {
    let known_sites = render_known_sites();
    let preferences = render_preferences(preferences);

    render(
        template,
        &Variables {
            cv_summary,
            preferences: &preferences,
            known_sites: &known_sites,
            ..Variables::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt_render::estimated_tokens;

    fn preferences() -> Preferences {
        Preferences {
            preferred_companies: vec!["Figma".into()],
            keywords: vec!["Rust".into(), "compiler".into()],
            ..Default::default()
        }
    }

    #[test]
    fn the_known_site_block_is_sorted_and_uses_engine_slugs() {
        let block = render_known_sites();
        let lines: Vec<&str> = block.lines().collect();

        assert!(
            lines.windows(2).all(|pair| pair[0] <= pair[1]),
            "the block is sorted so a diff is stable: {block}"
        );
        assert!(
            block.contains("- Figma | greenhouse | slug: figma"),
            "{block}"
        );
        assert!(
            block.contains("- Spotify | lever | slug: spotify"),
            "{block}"
        );
        assert!(block.contains("- Linear | ashby | slug: linear"), "{block}");
        assert!(
            block.contains("- Ubisoft | smartrecruiters | slug: Ubisoft2"),
            "{block}"
        );
        assert!(
            block.contains("- NVIDIA | eightfold | slug: nvidia.com"),
            "{block}"
        );
        assert!(
            !block.contains("Waymo"),
            "a selector site needs a hand-written config"
        );
    }

    #[test]
    fn assemble_substitutes_the_three_variables() {
        let rendered =
            assemble(EMBEDDED_PROMPT, "Ada, six years of Rust.", &preferences()).expect("render");

        assert!(rendered.contains("Ada, six years of Rust."));
        assert!(rendered.contains("Keywords: Rust, compiler"));
        assert!(rendered.contains("slug: figma"));
        assert!(!rendered.contains("{{"), "every placeholder was filled");
    }

    #[test]
    fn an_unknown_variable_fails_instead_of_rendering_empty() {
        let error = assemble(
            "{{cv_summary}} {{preferences}} {{known_sites}} {{not_a_variable}}",
            "Ada",
            &Preferences::default(),
        )
        .expect_err("unknown variable");

        assert_eq!(
            error,
            RenderError::Unknown {
                name: "not_a_variable".into()
            }
        );
    }

    #[test]
    fn the_prompt_is_under_its_ceiling_and_states_the_contract() {
        let chars = EMBEDDED_PROMPT.chars().count();
        assert!(
            estimated_tokens(chars) <= 800,
            "suggest_portals.md is about {} tokens; the ceiling is 800",
            estimated_tokens(chars)
        );

        for variable in ["{{cv_summary}}", "{{preferences}}", "{{known_sites}}"] {
            assert!(
                EMBEDDED_PROMPT.contains(variable),
                "the prompt must use {variable}"
            );
        }
        for engine in SuggestEngine::ALL {
            assert!(
                EMBEDDED_PROMPT.contains(engine.as_str()),
                "the prompt must name the {} engine",
                engine.as_str()
            );
        }
        assert!(
            EMBEDDED_PROMPT.contains("never write a URL"),
            "the prompt must forbid inventing URLs"
        );
        assert!(
            EMBEDDED_PROMPT.contains("untrusted"),
            "the prompt must state the untrusted-data rule"
        );
        assert!(
            EMBEDDED_PROMPT.contains("\"suggestions\""),
            "the prompt must describe the output contract"
        );
    }
}
