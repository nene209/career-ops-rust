//! Strict, flat variable substitution for prompt templates.
//!
//! A template is text with `{{name}}` placeholders. Five names exist, the set is
//! closed, and an unknown one is an error rather than an empty string: a prompt
//! that silently loses its job description produces a confident evaluation of
//! nothing, which is worse than a failed render. The same rule catches a typo
//! after a field is renamed.
//!
//! There is deliberately no logic: no conditionals, loops, filters, or partials.
//! Phase 3 sends one instruction and one set of values (Q-24).

use std::fmt;

/// The CV summary's own budget, in characters (about 1.5k tokens). The job
/// text has `llm.max_job_chars`; the CV is the other half of the prompt, and
/// every flow that fills `{{cv_summary}}` uses this one number.
pub const MAX_CV_CHARS: usize = 6_000;

/// A character-based token estimate, using the same ~4 characters per token
/// proxy as the budgets. It is deliberately conservative on the low side: the
/// point is to refuse an obviously oversized prompt, not to price a call.
pub fn estimated_tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

/// Keep at most `max` characters, marking the cut with an ellipsis.
///
/// Used for a budget the user did not ask for (the CV summary), where silently
/// dropping the tail would hide that it happened.
pub fn take_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    cut
}

/// A variable a Phase 3 prompt may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variable {
    /// The rendered `cv_summary` from `repo::profile`.
    CvSummary,
    /// The job text after `text::plain_text`, never the raw markup.
    JobDescription,
    CompanyName,
    JobTitle,
    /// The letter's tone; empty when the user did not choose one.
    Tone,
}

impl Variable {
    pub const ALL: [Variable; 5] = [
        Variable::CvSummary,
        Variable::JobDescription,
        Variable::CompanyName,
        Variable::JobTitle,
        Variable::Tone,
    ];

    /// The name inside the braces. Part of the template contract.
    pub fn name(self) -> &'static str {
        match self {
            Self::CvSummary => "cv_summary",
            Self::JobDescription => "job_description",
            Self::CompanyName => "company_name",
            Self::JobTitle => "job_title",
            Self::Tone => "tone",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "cv_summary" => Some(Self::CvSummary),
            "job_description" => Some(Self::JobDescription),
            "company_name" => Some(Self::CompanyName),
            "job_title" => Some(Self::JobTitle),
            "tone" => Some(Self::Tone),
            _ => None,
        }
    }

    /// One line for the Prompt Manager's variable reference.
    pub fn description(self) -> &'static str {
        match self {
            Self::CvSummary => "The rendered CV summary from CV / Profile.",
            Self::JobDescription => "The posting, normalized to plain text.",
            Self::CompanyName => "The company on the posting.",
            Self::JobTitle => "The posting's title.",
            Self::Tone => "An optional register for a cover letter; empty when unset.",
        }
    }
}

/// The values a render substitutes. `tone` is `""` when unset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Variables<'a> {
    pub cv_summary: &'a str,
    pub job_description: &'a str,
    pub company_name: &'a str,
    pub job_title: &'a str,
    pub tone: &'a str,
}

impl Variables<'_> {
    fn value(&self, variable: Variable) -> &str {
        match variable {
            Variable::CvSummary => self.cv_summary,
            Variable::JobDescription => self.job_description,
            Variable::CompanyName => self.company_name,
            Variable::JobTitle => self.job_title,
            Variable::Tone => self.tone,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// A `{{` with no closing `}}`. `offset` is a byte position in the template.
    Unclosed { offset: usize },
    /// A name outside the closed set.
    Unknown { name: String },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unclosed { offset } => {
                write!(
                    f,
                    "prompt template has an unclosed variable at byte {offset}"
                )
            }
            Self::Unknown { name } => write!(
                f,
                "prompt template uses unknown variable `{{{{{name}}}}}`; known variables: {}",
                Variable::ALL
                    .iter()
                    .map(|variable| variable.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl std::error::Error for RenderError {}

/// The variable set the app renders for a prompt category, when the category
/// is one the app feeds.
///
/// Only `evaluate` and `cover_letter` are rendered by a pipeline. The reference
/// categories may contain their own placeholder syntax (the PDF template in
/// `pdf.md` uses `{{PHOTO}}`), so validating them against this set would reject
/// legitimate content.
pub fn variables_for_category(category: &str) -> Option<&'static [Variable]> {
    match category {
        "evaluate" | "cover_letter" => Some(&Variable::ALL),
        _ => None,
    }
}

/// Check every placeholder without substituting anything.
///
/// The Prompt Manager runs this on save, so an unknown variable is an error
/// while the user is editing rather than when an evaluation is attempted.
pub fn validate(template: &str) -> Result<(), RenderError> {
    let mut rest = template;
    let mut offset = 0usize;

    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err(RenderError::Unclosed {
                offset: offset + start,
            });
        };

        let name = &after[..end];
        if Variable::parse(name).is_none() {
            return Err(RenderError::Unknown {
                name: name.trim().to_string(),
            });
        }

        let consumed = start + 2 + end + 2;
        rest = &rest[consumed..];
        offset += consumed;
    }

    Ok(())
}

/// Substitute every placeholder in `template`.
///
/// Placeholders may carry surrounding whitespace (`{{ job_title }}`). Text with
/// no placeholders is returned unchanged, so a plain user prompt works too.
pub fn render(template: &str, variables: &Variables<'_>) -> Result<String, RenderError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut offset = 0usize;

    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err(RenderError::Unclosed {
                offset: offset + start,
            });
        };

        let name = &after[..end];
        let Some(variable) = Variable::parse(name) else {
            return Err(RenderError::Unknown {
                name: name.trim().to_string(),
            });
        };
        out.push_str(variables.value(variable));

        let consumed = start + 2 + end + 2;
        rest = &rest[consumed..];
        offset += consumed;
    }

    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variables() -> Variables<'static> {
        Variables {
            cv_summary: "Ada Lovelace\nExperience:\n- Analytical engine",
            job_description: "Build compilers.",
            company_name: "Acme",
            job_title: "Compiler Engineer",
            tone: "",
        }
    }

    #[test]
    fn every_variable_is_substituted() {
        let template =
            "{{company_name}} / {{job_title}}\n{{job_description}}\n{{cv_summary}}\n[{{tone}}]";
        let rendered = render(template, &variables()).expect("render");

        assert_eq!(
            rendered,
            "Acme / Compiler Engineer\nBuild compilers.\nAda Lovelace\nExperience:\n- Analytical engine\n[]"
        );
    }

    #[test]
    fn whitespace_inside_the_braces_is_allowed() {
        let rendered = render("{{ job_title }}", &variables()).expect("render");
        assert_eq!(rendered, "Compiler Engineer");
    }

    #[test]
    fn a_repeated_variable_is_substituted_every_time() {
        let rendered = render("{{tone}}{{tone}}", &variables()).expect("render");
        assert_eq!(rendered, "");
    }

    #[test]
    fn an_unknown_variable_is_an_error_not_an_empty_string() {
        let error = render("{{job_title}} at {{company}}", &variables()).expect_err("unknown");
        assert_eq!(
            error,
            RenderError::Unknown {
                name: "company".into()
            }
        );
        assert!(error.to_string().contains("company_name"));
    }

    #[test]
    fn an_unclosed_placeholder_is_an_error() {
        let error = render("hello {{job_title", &variables()).expect_err("unclosed");
        assert_eq!(error, RenderError::Unclosed { offset: 6 });
    }

    #[test]
    fn text_without_placeholders_passes_through() {
        let template = "Plain instructions, no variables.";
        assert_eq!(render(template, &variables()).expect("render"), template);
    }

    #[test]
    fn validate_accepts_the_known_set_and_rejects_the_rest() {
        assert!(validate("{{company_name}} / {{ job_title }}").is_ok());
        assert_eq!(
            validate("{{PHOTO}}").expect_err("unknown"),
            RenderError::Unknown {
                name: "PHOTO".into()
            }
        );
        assert_eq!(
            validate("open {{job_title").expect_err("unclosed"),
            RenderError::Unclosed { offset: 5 }
        );
    }

    #[test]
    fn only_the_rendered_categories_have_a_variable_set() {
        assert!(variables_for_category("evaluate").is_some());
        assert!(variables_for_category("cover_letter").is_some());
        assert!(
            variables_for_category("render").is_none(),
            "reference prompts may use their own placeholders"
        );
        assert!(variables_for_category("utility").is_none());
    }
}
