//! Prompt Manager view state.
//!
//! The list has one row per `(name, is_system)` thread. A user copy and the
//! shipped system prompt are different threads with independent version
//! sequences, so showing them separately is what makes "which one is actually
//! used" visible. `active_for_use` prefers the user copy; the list marks that
//! row **In use** and leaves the system row as reference.
//!
//! The diff is computed once per click (not per frame): the reference prompts
//! are large, and an LCS over every line on every paint would make the screen
//! feel broken.

use std::collections::{HashMap, HashSet};

use career_ops_core::model::Prompt;
use career_ops_core::repo::prompts::{PromptThread, PromptUsageEntry};

/// One thread's latest version, as the list renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptThreadRow {
    pub name: String,
    pub category: String,
    pub is_system: bool,
    pub current_id: i64,
    pub current_version: i64,
    pub version_count: i64,
    pub updated_at: String,
    /// This owner is what `active_for_use` resolves to.
    pub in_use: bool,
}

impl PromptThreadRow {
    pub fn owner_label(&self) -> &'static str {
        if self.is_system {
            "System"
        } else {
            "Your copy"
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptsViewModel {
    pub threads: Vec<PromptThreadRow>,
}

impl PromptsViewModel {
    pub fn from_threads(threads: Vec<PromptThread>) -> Self {
        let mut rows: Vec<PromptThreadRow> = threads
            .into_iter()
            .map(|thread| PromptThreadRow {
                name: thread.name,
                category: thread.category,
                is_system: thread.is_system,
                current_id: thread.current_id,
                current_version: thread.current_version,
                version_count: thread.version_count,
                updated_at: thread.updated_at.format("%Y-%m-%d %H:%M").to_string(),
                in_use: false,
            })
            .collect();

        // A user copy wins, which is exactly `active_for_use`'s rule.
        let user_names: HashSet<String> = rows
            .iter()
            .filter(|row| !row.is_system)
            .map(|row| row.name.clone())
            .collect();
        for row in &mut rows {
            row.in_use = !row.is_system || !user_names.contains(&row.name);
        }

        Self { threads: rows }
    }

    /// The categories present, in the order the query returned them.
    pub fn categories(&self) -> Vec<&str> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for row in &self.threads {
            if seen.insert(row.category.as_str()) {
                out.push(row.category.as_str());
            }
        }
        out
    }

    pub fn threads_in<'a>(
        &'a self,
        category: &'a str,
    ) -> impl Iterator<Item = &'a PromptThreadRow> {
        self.threads
            .iter()
            .filter(move |row| row.category == category)
    }

    pub fn thread(&self, name: &str, is_system: bool) -> Option<&PromptThreadRow> {
        self.threads
            .iter()
            .find(|row| row.name == name && row.is_system == is_system)
    }

    /// The thread `active_for_use` would pick for `name`.
    pub fn active(&self, name: &str) -> Option<&PromptThreadRow> {
        self.thread(name, false).or_else(|| self.thread(name, true))
    }

    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }
}

/// One stored version of the selected thread, with what used it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptVersionRow {
    pub id: i64,
    pub version: i64,
    pub content_hash: String,
    pub updated_at: String,
    pub content: String,
    pub usage: Vec<PromptUsageRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptUsageRow {
    pub kind: &'static str,
    pub id: i64,
    /// "Engineer at Acme", or a fallback when the join found no title.
    pub posting: String,
    pub generated_at: String,
    pub model_used: String,
}

/// The selected thread's versions, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptDetail {
    pub name: String,
    pub category: String,
    pub is_system: bool,
    pub versions: Vec<PromptVersionRow>,
}

impl PromptDetail {
    pub fn from_versions(
        name: &str,
        category: &str,
        is_system: bool,
        versions: Vec<Prompt>,
        usage: &HashMap<i64, Vec<PromptUsageEntry>>,
    ) -> Self {
        let versions = versions
            .into_iter()
            .map(|prompt| PromptVersionRow {
                id: prompt.id,
                version: prompt.version,
                content_hash: prompt.content_hash,
                updated_at: prompt.updated_at.format("%Y-%m-%d %H:%M").to_string(),
                content: prompt.content,
                usage: usage
                    .get(&prompt.id)
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|entry| PromptUsageRow {
                                kind: entry.kind.label(),
                                id: entry.id,
                                posting: match (&entry.job_title, &entry.company) {
                                    (Some(title), Some(company)) => {
                                        format!("{title} at {company}")
                                    }
                                    (Some(title), None) => title.clone(),
                                    (None, Some(company)) => company.clone(),
                                    (None, None) => "Posting no longer stored".into(),
                                },
                                generated_at: entry
                                    .generated_at
                                    .format("%Y-%m-%d %H:%M")
                                    .to_string(),
                                model_used: entry.model_used.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();

        Self {
            name: name.to_string(),
            category: category.to_string(),
            is_system,
            versions,
        }
    }

    pub fn latest(&self) -> Option<&PromptVersionRow> {
        self.versions.first()
    }

    pub fn version(&self, id: i64) -> Option<&PromptVersionRow> {
        self.versions.iter().find(|row| row.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }
}

/// Which thread the detail pane is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSelection {
    pub name: String,
    pub is_system: bool,
}

impl PromptSelection {
    pub fn new(name: impl Into<String>, is_system: bool) -> Self {
        Self {
            name: name.into(),
            is_system,
        }
    }
}

/// Transient screen state: the selection, the editor, and the cached diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptsView {
    pub selected: Option<PromptSelection>,
    /// A historical version shown instead of the latest, when the user picked
    /// one from the version list.
    pub selected_version: Option<i64>,
    /// The editor for a user thread. Unused for a system thread.
    pub editor: String,
    pub dirty: bool,
    pub compare: bool,
    /// Cached diff between the shown version and the latest of the same thread.
    pub diff: Vec<DiffLine>,
}

impl PromptsView {
    /// Select a thread and load its latest content into the editor.
    pub fn select(&mut self, selection: PromptSelection, detail: Option<&PromptDetail>) {
        self.selected = Some(selection);
        self.selected_version = None;
        self.compare = false;
        self.diff.clear();
        self.dirty = false;
        self.editor = if detail.is_some_and(|detail| !detail.is_system) {
            detail
                .and_then(|detail| detail.latest())
                .map(|row| row.content.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
    }

    /// Show a historical version. `None` returns to the latest.
    pub fn show_version(&mut self, version_id: Option<i64>, detail: Option<&PromptDetail>) {
        self.selected_version = version_id;
        self.compare = false;
        self.diff.clear();
        if version_id.is_none() {
            if let Some(detail) = detail {
                if !detail.is_system {
                    if let Some(latest) = detail.latest() {
                        self.editor = latest.content.clone();
                        self.dirty = false;
                    }
                }
            }
        }
    }

    /// The content the detail pane shows: the picked version, else the latest.
    pub fn viewed_content<'a>(&self, detail: &'a PromptDetail) -> Option<&'a str> {
        match self.selected_version {
            Some(id) => detail.version(id).map(|row| row.content.as_str()),
            None => detail.latest().map(|row| row.content.as_str()),
        }
    }

    /// The version row the detail pane shows.
    pub fn viewed_version<'a>(&self, detail: &'a PromptDetail) -> Option<&'a PromptVersionRow> {
        match self.selected_version {
            Some(id) => detail.version(id),
            None => detail.latest(),
        }
    }

    /// Toggle the diff and compute it against the thread's latest version.
    ///
    /// A diff of the latest version against itself is empty; the screen hides
    /// the toggle there.
    pub fn set_compare(&mut self, on: bool, detail: Option<&PromptDetail>) {
        self.compare = on;
        self.diff.clear();
        if !on {
            return;
        }
        let Some(detail) = detail else { return };
        let Some(latest) = detail.latest() else {
            return;
        };
        let Some(viewed) = self.viewed_version(detail) else {
            return;
        };
        if viewed.id == latest.id {
            return;
        }
        self.diff = diff_lines(&viewed.content, &latest.content);
    }

    /// The editor content differs from the latest version.
    pub fn editor_is_changed(&self, detail: Option<&PromptDetail>) -> bool {
        let Some(detail) = detail else { return false };
        if detail.is_system {
            return false;
        }
        detail
            .latest()
            .is_some_and(|latest| self.editor != latest.content)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Same,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

/// How many LCS cells a diff may allocate before falling back.
///
/// A 1,500-line reference prompt is 2.25M cells; the cap keeps a pathological
/// pair from allocating hundreds of megabytes on a click.
const MAX_DIFF_CELLS: usize = 4_000_000;

/// A line diff from `old` to `new`.
///
/// The result is a unified stream: unchanged lines with `Same`, lines only in
/// `old` with `Removed`, and lines only in `new` with `Added`. Above the cell
/// cap the result is still correct, just unrefined: every old line, then every
/// new line.
pub fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();

    if a.len().saturating_mul(b.len()) > MAX_DIFF_CELLS {
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend(a.into_iter().map(|line| DiffLine {
            kind: DiffKind::Removed,
            text: line.to_string(),
        }));
        out.extend(b.into_iter().map(|line| DiffLine {
            kind: DiffKind::Added,
            text: line.to_string(),
        }));
        return out;
    }

    // lcs[i][j] = length of the longest common subsequence of a[i..], b[j..].
    let width = b.len() + 1;
    let mut lcs = vec![0u32; (a.len() + 1) * width];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i * width + j] = if a[i] == b[j] {
                lcs[(i + 1) * width + j + 1] + 1
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }

    let mut out = Vec::with_capacity(a.len().max(b.len()));
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out.push(DiffLine {
                kind: DiffKind::Same,
                text: a[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if lcs[(i + 1) * width + j] >= lcs[i * width + j + 1] {
            out.push(DiffLine {
                kind: DiffKind::Removed,
                text: a[i].to_string(),
            });
            i += 1;
        } else {
            out.push(DiffLine {
                kind: DiffKind::Added,
                text: b[j].to_string(),
            });
            j += 1;
        }
    }
    while i < a.len() {
        out.push(DiffLine {
            kind: DiffKind::Removed,
            text: a[i].to_string(),
        });
        i += 1;
    }
    while j < b.len() {
        out.push(DiffLine {
            kind: DiffKind::Added,
            text: b[j].to_string(),
        });
        j += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_copy_is_the_one_in_use() {
        let view = PromptsViewModel::from_threads(vec![
            thread("evaluate", true, 1),
            thread("evaluate", false, 2),
            thread("scan", true, 1),
        ]);

        let user = view.thread("evaluate", false).expect("user row");
        assert!(user.in_use);
        let system = view.thread("evaluate", true).expect("system row");
        assert!(!system.in_use, "a user copy wins over the system row");
        assert!(view.thread("scan", true).expect("scan").in_use);
        assert_eq!(view.categories(), vec!["evaluate", "scan"]);
        assert!(view.active("evaluate").is_some_and(|row| !row.is_system));
    }

    #[test]
    fn selecting_a_thread_loads_only_user_content_into_the_editor() {
        let detail = detail(false, &["newest", "older"]);
        let mut view = PromptsView::default();

        view.select(
            PromptSelection::new("evaluate", true),
            Some(&detail_for(true)),
        );
        assert!(view.editor.is_empty(), "system prompts are read-only");

        view.select(PromptSelection::new("evaluate", false), Some(&detail));
        assert_eq!(view.editor, "newest");
        assert!(!view.dirty);
    }

    #[test]
    fn the_diff_marks_unchanged_old_and_new_lines() {
        let diff = diff_lines("one\ntwo\nthree", "one\nTWO\nthree");
        assert_eq!(
            diff,
            vec![
                DiffLine {
                    kind: DiffKind::Same,
                    text: "one".into()
                },
                DiffLine {
                    kind: DiffKind::Removed,
                    text: "two".into()
                },
                DiffLine {
                    kind: DiffKind::Added,
                    text: "TWO".into()
                },
                DiffLine {
                    kind: DiffKind::Same,
                    text: "three".into()
                },
            ]
        );
    }

    #[test]
    fn an_identical_diff_is_all_unchanged() {
        let diff = diff_lines("a\nb", "a\nb");
        assert!(diff.iter().all(|line| line.kind == DiffKind::Same));
    }

    #[test]
    fn comparing_a_historical_version_computes_against_the_latest() {
        let detail = detail(false, &["current", "previous"]);
        let previous_id = detail.versions[1].id;
        let mut view = PromptsView::default();
        view.select(PromptSelection::new("evaluate", false), Some(&detail));
        view.selected_version = Some(previous_id);

        view.set_compare(true, Some(&detail));
        assert_eq!(
            view.diff,
            vec![
                DiffLine {
                    kind: DiffKind::Removed,
                    text: "previous".into()
                },
                DiffLine {
                    kind: DiffKind::Added,
                    text: "current".into()
                },
            ]
        );

        // Comparing the latest with itself is empty and the screen hides it.
        view.selected_version = None;
        view.set_compare(true, Some(&detail));
        assert!(view.diff.is_empty());
    }

    fn thread(name: &str, is_system: bool, version: i64) -> PromptThread {
        PromptThread {
            name: name.into(),
            category: if name == "scan" { "scan" } else { "evaluate" }.into(),
            is_system,
            current_id: version,
            current_version: version,
            version_count: version,
            updated_at: chrono::Utc::now(),
        }
    }

    fn detail(is_system: bool, contents: &[&str]) -> PromptDetail {
        let versions: Vec<Prompt> = contents
            .iter()
            .enumerate()
            .map(|(index, content)| Prompt {
                id: (index + 1) as i64,
                name: "evaluate".into(),
                category: "evaluate".into(),
                content: (*content).into(),
                is_system,
                version: (contents.len() - index) as i64,
                content_hash: format!("hash-{index}"),
                supersedes_id: None,
                is_current: index == 0,
                retired_at: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .collect();
        PromptDetail::from_versions("evaluate", "evaluate", is_system, versions, &HashMap::new())
    }

    fn detail_for(is_system: bool) -> PromptDetail {
        detail(is_system, &["system text"])
    }
}
