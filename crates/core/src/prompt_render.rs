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
}
