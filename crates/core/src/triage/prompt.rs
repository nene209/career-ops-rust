//! Triage prompt: the batch input rendering and strict assembly.
//!
//! The prompt itself is `crates/core/prompts/triage_batch.md`, seeded as the
//! system prompt named `triage`. The imported `triage.md` is the original mode
//! and stays as reference, like `oferta.md` beside `evaluate.md`: it addresses an
//! agent with file access and is never sent as a single-shot prompt. This module
//! prepares the two inputs the template needs beyond the CV summary: a short
//! preferences block and one line per posting.

use crate::preferences::Preferences;
use crate::prompt_render::{render, RenderError, Variables};

/// The seeded prompt's name and category.
pub const PROMPT_NAME: &str = "triage";
pub const PROMPT_CATEGORY: &str = "triage";

/// The embedded prompt, so its ceiling can be tested without a database.
#[cfg(test)]
const EMBEDDED_PROMPT: &str = include_str!("../../prompts/triage_batch.md");

/// The brief's own budget, in characters.
///
/// A brief is a sorting aid, not the posting. Bounding it keeps a batch of 50
/// postings inside a modest input estimate; a `worth_details` decision is what
/// fetches the full text.
pub const BRIEF_CHARS: usize = 400;

/// One posting as the prompt sees it.
///
/// The brief is prepared by the caller: the service normalizes the stored text
/// with `text::plain_text` and bounds it, so a 200 KB posting cannot dominate
/// the batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriageJob<'a> {
    pub id: i64,
    pub title: &'a str,
    pub company: &'a str,
    pub brief: &'a str,
}

/// Render the batch as one `id | title | company | brief` line per posting.
///
/// Every field is collapsed to a single line and a literal `|` becomes `/`, so
/// a hostile title cannot invent a fifth column or a second posting.
pub fn render_jobs(jobs: &[TriageJob<'_>]) -> String {
    jobs.iter()
        .map(|job| {
            format!(
                "{} | {} | {} | {}",
                job.id,
                one_line(job.title),
                one_line(job.company),
                one_line(job.brief)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render the saved preferences as a short advisory block.
///
/// Only configured lists appear; an empty configuration says so, so the model
/// can tell "the user has no preferences" from "the block was lost".
pub fn render_preferences(preferences: &Preferences) -> String {
    let mut lines = Vec::new();

    let companies = join(&preferences.preferred_companies);
    if !companies.is_empty() {
        lines.push(format!("Preferred companies: {companies}"));
    }
    let locations = join(&preferences.preferred_locations);
    if !locations.is_empty() {
        lines.push(format!("Preferred locations: {locations}"));
    }
    let keywords = join(&preferences.keywords);
    if !keywords.is_empty() {
        lines.push(format!("Keywords: {keywords}"));
    }
    let excluded = join(&preferences.excluded_companies);
    if !excluded.is_empty() {
        lines.push(format!("Excluded companies: {excluded}"));
    }
    if let Some(seniority) = preferences
        .seniority
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Seniority: {seniority}"));
    }

    if lines.is_empty() {
        "No preferences are configured.".to_string()
    } else {
        lines.join("\n")
    }
}

/// Fill the triage template with the candidate summary, the preferences, and
/// the batch.
pub fn assemble(
    template: &str,
    cv_summary: &str,
    preferences: &Preferences,
    jobs: &[TriageJob<'_>],
) -> Result<String, RenderError> {
    let preferences = render_preferences(preferences);
    let jobs = render_jobs(jobs);

    render(
        template,
        &Variables {
            cv_summary,
            preferences: &preferences,
            jobs: &jobs,
            ..Variables::default()
        },
    )
}

fn join(values: &[String]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn one_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let sanitized = collapsed.replace('|', "/");
    if sanitized.is_empty() {
        "—".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt_render::estimated_tokens;

    fn preferences() -> Preferences {
        Preferences {
            preferred_companies: vec!["Anthropic".into(), "Figma".into()],
            preferred_locations: vec!["Remote".into()],
            keywords: vec!["Rust".into(), "compiler".into()],
            excluded_companies: vec!["RecruiterCo".into()],
            seniority: Some("staff".into()),
        }
    }

    fn jobs() -> [TriageJob<'static>; 2] {
        [
            TriageJob {
                id: 1,
                title: "Senior Rust Engineer",
                company: "Acme",
                brief: "Build compilers and developer tools.",
            },
            TriageJob {
                id: 2,
                title: "Brand Marketing Manager",
                company: "Globex",
                brief: "Run campaigns.",
            },
        ]
    }

    #[test]
    fn jobs_render_one_line_each_and_cannot_add_a_column() {
        let rendered = render_jobs(&[
            TriageJob {
                id: 7,
                title: "Rust | Engineer\nwith a newline",
                company: "Acme",
                brief: "",
            },
            TriageJob {
                id: 8,
                title: "Designer",
                company: "Globex",
                brief: "One line.",
            },
        ]);

        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2, "one line per posting");
        assert_eq!(lines[0], "7 | Rust / Engineer with a newline | Acme | —");
        assert_eq!(lines[1], "8 | Designer | Globex | One line.");
    }

    #[test]
    fn preferences_render_only_the_configured_fields() {
        let rendered = render_preferences(&preferences());

        assert_eq!(
            rendered,
            "Preferred companies: Anthropic, Figma\n\
             Preferred locations: Remote\n\
             Keywords: Rust, compiler\n\
             Excluded companies: RecruiterCo\n\
             Seniority: staff"
        );
    }

    #[test]
    fn empty_preferences_say_so() {
        assert_eq!(
            render_preferences(&Preferences::default()),
            "No preferences are configured."
        );
    }

    #[test]
    fn blank_list_entries_are_dropped() {
        let preferences = Preferences {
            keywords: vec!["  ".into(), "Rust".into(), String::new()],
            ..Preferences::default()
        };
        assert_eq!(render_preferences(&preferences), "Keywords: Rust");
    }

    #[test]
    fn assemble_substitutes_the_three_variables() {
        let rendered = assemble(
            EMBEDDED_PROMPT,
            "Ada, six years of Rust.",
            &preferences(),
            &jobs(),
        )
        .expect("render");

        assert!(rendered.contains("Ada, six years of Rust."));
        assert!(rendered.contains("Preferred companies: Anthropic, Figma"));
        assert!(rendered.contains("1 | Senior Rust Engineer | Acme | Build compilers"));
        assert!(rendered.contains("2 | Brand Marketing Manager | Globex | Run campaigns."));
        assert!(!rendered.contains("{{"), "every placeholder was filled");
    }

    #[test]
    fn an_unknown_variable_fails_instead_of_rendering_empty() {
        let error = assemble(
            "{{cv_summary}} {{preferences}} {{jobs}} {{not_a_variable}}",
            "Ada",
            &Preferences::default(),
            &[],
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
            "triage.md is about {} tokens; the ceiling is 800",
            estimated_tokens(chars)
        );

        for variable in ["{{cv_summary}}", "{{preferences}}", "{{jobs}}"] {
            assert!(
                EMBEDDED_PROMPT.contains(variable),
                "triage.md must use {variable}"
            );
        }
        for value in ["worth_details", "evaluate", "skip", "unsure"] {
            assert!(EMBEDDED_PROMPT.contains(value), "missing {value}");
        }
        assert!(
            EMBEDDED_PROMPT.contains("untrusted"),
            "the prompt must state the untrusted-data rule"
        );
        assert!(
            EMBEDDED_PROMPT.contains("\"job_id\""),
            "the prompt must describe the output contract"
        );
    }
}
