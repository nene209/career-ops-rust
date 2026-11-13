//! Usage view-model: the ledger's rows shaped for the screen.
//!
//! Pure — no egui, no clock. The range selection is here because it decides the
//! query's lower bound, and the formatting is here because "never present a
//! number as more precise than it is" is a rule worth testing without a UI.

use chrono::{DateTime, Duration, Utc};

use career_ops_core::llm::LlmProvider;
use career_ops_core::repo::llm_usage::{ProviderModelTotals, RecentCall, TaskTotals};

/// The period the screen asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Range {
    Last7Days,
    #[default]
    Last30Days,
    All,
}

impl Range {
    pub fn all() -> &'static [Range] {
        &[Self::Last7Days, Self::Last30Days, Self::All]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Last7Days => "7 days",
            Self::Last30Days => "30 days",
            Self::All => "All",
        }
    }

    /// The query's lower bound. `None` means all time.
    pub fn since(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Self::Last7Days => Some(now - Duration::days(7)),
            Self::Last30Days => Some(now - Duration::days(30)),
            Self::All => None,
        }
    }
}

/// The header's totals, summed from the per-model groups so the two cannot
/// disagree.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UsageTotals {
    pub calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    /// `None` when no call in the range has a price.
    pub cost_usd: Option<f64>,
    /// Rows with no price, or with a price that is only a lower bound.
    pub unpriced_calls: i64,
    pub reported_calls: i64,
    pub estimated_calls: i64,
}

impl UsageTotals {
    pub fn from_groups(groups: &[ProviderModelTotals]) -> Self {
        let mut totals = Self::default();
        let mut cost = 0.0;
        let mut any_priced = false;

        for group in groups {
            totals.calls += group.calls;
            totals.input_tokens += group.input_tokens;
            totals.output_tokens += group.output_tokens;
            totals.total_tokens += group.total_tokens;
            totals.unpriced_calls += group.unpriced_calls;
            totals.reported_calls += group.reported_calls;
            totals.estimated_calls += group.estimated_calls;
            if let Some(group_cost) = group.cost_usd {
                cost += group_cost;
                any_priced = true;
            }
        }

        totals.cost_usd = any_priced.then_some(cost);
        totals
    }

    /// One line: the counts, then the cost when there is one.
    pub fn summary(&self) -> String {
        let calls = match self.calls {
            1 => "1 call".to_string(),
            calls => format!("{} calls", format_tokens(calls)),
        };

        let cost = if self.cost_usd.is_some() {
            format!(" · {}", format_cost(self.cost_usd, self.unpriced_calls > 0))
        } else {
            String::new()
        };

        format!(
            "{calls} · {} input · {} output · {} total{cost}",
            format_tokens(self.input_tokens),
            format_tokens(self.output_tokens),
            format_tokens(self.total_tokens),
        )
    }

    /// How much of the total is reported and how much is estimated, plus the
    /// rows whose cost is unknown.
    pub fn provenance(&self) -> String {
        let mut parts = Vec::new();
        if self.reported_calls > 0 {
            parts.push(format!("{} reported", self.reported_calls));
        }
        if self.estimated_calls > 0 {
            parts.push(format!("{} estimated", self.estimated_calls));
        }

        let mut line = if parts.is_empty() {
            "No calls in this period.".to_string()
        } else {
            parts.join(", ")
        };

        if self.unpriced_calls > 0 {
            let noun = if self.unpriced_calls == 1 {
                "call has"
            } else {
                "calls have"
            };
            line.push_str(&format!(
                " · {} {noun} no price",
                format_tokens(self.unpriced_calls)
            ));
        }

        line
    }
}

/// One row of the recent-calls list, with its cells already formatted.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentUsageRow {
    pub when: String,
    pub model: String,
    pub task: String,
    /// `Title · Company`, or an em dash when the posting is gone.
    pub job: String,
    /// `input / output`, either side an em dash when unreported.
    pub tokens: String,
    pub source: String,
    pub cost: String,
    pub outcome: String,
}

impl From<RecentCall> for RecentUsageRow {
    fn from(row: RecentCall) -> Self {
        let job = match (row.job_title.as_deref(), row.company.as_deref()) {
            (Some(title), Some(company)) => format!("{title} · {company}"),
            (Some(title), None) => title.to_string(),
            (None, Some(company)) => company.to_string(),
            (None, None) => "—".to_string(),
        };

        let tokens = match (row.input_tokens, row.output_tokens) {
            (Some(input), Some(output)) => {
                format!("{} / {}", format_tokens(input), format_tokens(output))
            }
            (Some(input), None) => format!("{} / —", format_tokens(input)),
            (None, Some(output)) => format!("— / {}", format_tokens(output)),
            (None, None) => "—".to_string(),
        };

        Self {
            when: row.created_at.format("%Y-%m-%d %H:%M").to_string(),
            model: row.model,
            task: row.task,
            job,
            tokens,
            source: row.source,
            cost: format_cost(row.cost_usd, row.cost_complete == Some(false)),
            outcome: row.outcome,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UsageViewModel {
    pub range: Range,
    pub totals: UsageTotals,
    pub by_provider_model: Vec<ProviderModelTotals>,
    pub by_task: Vec<TaskTotals>,
    pub recent: Vec<RecentUsageRow>,
}

impl UsageViewModel {
    /// Build the screen's data from the three repository reads.
    ///
    /// The stored provider id is shown as its label, so a row reads "Local /
    /// custom" rather than "custom". An id this build does not know is shown
    /// verbatim: a row in the ledger should never become unreadable.
    pub fn from_rows(
        range: Range,
        providers: Vec<ProviderModelTotals>,
        by_task: Vec<TaskTotals>,
        recent: Vec<RecentCall>,
    ) -> Self {
        let by_provider_model: Vec<ProviderModelTotals> =
            providers.into_iter().map(named_provider).collect();
        let totals = UsageTotals::from_groups(&by_provider_model);
        Self {
            range,
            totals,
            by_provider_model,
            by_task,
            recent: recent.into_iter().map(RecentUsageRow::from).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.totals.calls == 0
    }
}

/// A group's provider id as the screen spells it.
///
/// The ledgers store the id (`custom`), not the label (`Local / custom`), so
/// renaming a label never rewrites history. An unknown id is left as it is.
fn named_provider(mut group: ProviderModelTotals) -> ProviderModelTotals {
    if let Some(label) = LlmProvider::parse(&group.provider).map(|provider| provider.label()) {
        group.provider = label.to_string();
    }
    group
}

/// Group digits in threes: `118204` becomes `118,204`.
pub fn format_tokens(tokens: i64) -> String {
    let digits = tokens.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A cost cell.
///
/// `None` is an em dash, never a zero. A partial price is a lower bound; a
/// complete one is still an estimate, because the catalog price is a list
/// price, not an invoice.
pub fn format_cost(cost: Option<f64>, lower_bound: bool) -> String {
    match cost {
        Some(total) if lower_bound => format!("≥ {}", format_usd(total)),
        Some(total) => format!("est. {}", format_usd(total)),
        None => "—".to_string(),
    }
}

/// USD with enough decimals to keep a small call visible.
fn format_usd(total: f64) -> String {
    let decimals = if total >= 0.01 {
        2
    } else if total >= 0.0001 {
        4
    } else {
        6
    };

    let mut text = format!("{total:.decimals$}");
    // Trim trailing zeros but keep two decimals, so `$0.42` does not become
    // `$0.4200` and `$0.0001` keeps its significant digits.
    if let Some((whole, fraction)) = text.split_once('.') {
        let trimmed = fraction.trim_end_matches('0');
        let fraction = if trimmed.len() < 2 {
            format!("{trimmed:0<2}")
        } else {
            trimmed.to_string()
        };
        text = format!("{whole}.{fraction}");
    }

    format!("${text}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn group(
        cost: Option<f64>,
        unpriced: i64,
        reported: i64,
        estimated: i64,
    ) -> ProviderModelTotals {
        ProviderModelTotals {
            provider: "deepseek".into(),
            model: "deepseek-flash".into(),
            calls: reported + estimated,
            input_tokens: 100,
            output_tokens: 20,
            total_tokens: 120,
            cost_usd: cost,
            unpriced_calls: unpriced,
            reported_calls: reported,
            estimated_calls: estimated,
        }
    }

    #[test]
    fn the_range_helper_names_seven_days_thirty_days_and_all() {
        let now = Utc.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).unwrap();

        assert_eq!(Range::Last7Days.since(now), Some(now - Duration::days(7)));
        assert_eq!(Range::Last30Days.since(now), Some(now - Duration::days(30)));
        assert_eq!(Range::All.since(now), None);
        assert_eq!(Range::default(), Range::Last30Days);
        assert_eq!(
            Range::all()
                .iter()
                .map(|range| range.label())
                .collect::<Vec<_>>(),
            vec!["7 days", "30 days", "All"]
        );
    }

    #[test]
    fn totals_sum_the_groups_and_keep_a_missing_cost_missing() {
        let totals = UsageTotals::from_groups(&[group(Some(0.5), 0, 2, 0), group(None, 1, 0, 1)]);

        assert_eq!(totals.calls, 3);
        assert_eq!(totals.input_tokens, 200);
        assert_eq!(totals.output_tokens, 40);
        assert_eq!(totals.total_tokens, 240);
        assert_eq!(totals.cost_usd, Some(0.5), "only priced groups are summed");
        assert_eq!(totals.unpriced_calls, 1);
        assert_eq!(totals.reported_calls, 2);
        assert_eq!(totals.estimated_calls, 1);
    }

    #[test]
    fn no_priced_group_leaves_the_cost_empty() {
        let totals = UsageTotals::from_groups(&[group(None, 1, 0, 1)]);
        assert_eq!(totals.cost_usd, None);
        assert_eq!(totals.unpriced_calls, 1);
    }

    #[test]
    fn the_summary_names_the_counts_and_the_cost() {
        let totals = UsageTotals::from_groups(&[group(Some(0.0018), 0, 1, 0)]);
        assert_eq!(
            totals.summary(),
            "1 call · 100 input · 20 output · 120 total · est. $0.0018"
        );

        let several = UsageTotals::from_groups(&[group(Some(0.42), 0, 10, 2)]);
        assert_eq!(
            several.summary(),
            "12 calls · 100 input · 20 output · 120 total · est. $0.42"
        );
    }

    #[test]
    fn the_summary_is_a_lower_bound_when_a_call_is_unpriced() {
        let totals = UsageTotals::from_groups(&[group(Some(0.5), 1, 1, 1)]);
        assert!(
            totals.summary().contains("≥ $0.50"),
            "summary was {}",
            totals.summary()
        );
    }

    #[test]
    fn the_provenance_line_separates_reported_from_estimated() {
        let totals = UsageTotals::from_groups(&[group(Some(0.5), 0, 10, 2)]);
        assert_eq!(totals.provenance(), "10 reported, 2 estimated");

        let only_estimated = UsageTotals::from_groups(&[group(None, 1, 0, 2)]);
        assert_eq!(
            only_estimated.provenance(),
            "2 estimated · 1 call has no price"
        );

        let only_reported = UsageTotals::from_groups(&[group(Some(0.5), 0, 3, 0)]);
        assert_eq!(only_reported.provenance(), "3 reported");

        let none = UsageTotals::from_groups(&[]);
        assert_eq!(none.provenance(), "No calls in this period.");
    }

    #[test]
    fn the_screen_names_the_stored_provider_id() {
        let mut local = group(None, 1, 1, 0);
        local.provider = "custom".into();
        local.model = "llama3.2".into();
        let unknown = ProviderModelTotals {
            provider: "a_later_provider".into(),
            ..group(None, 1, 0, 1)
        };

        let view =
            UsageViewModel::from_rows(Range::All, vec![local, unknown], Vec::new(), Vec::new());

        assert_eq!(view.by_provider_model[0].provider, "Local / custom");
        assert_eq!(
            view.by_provider_model[1].provider, "a_later_provider",
            "an id this build does not know is shown as stored"
        );
    }

    #[test]
    fn tokens_are_grouped_in_threes() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(7), "7");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_000), "1,000");
        assert_eq!(format_tokens(118_204), "118,204");
        assert_eq!(format_tokens(1_234_567), "1,234,567");
    }

    #[test]
    fn a_cost_is_an_estimate_or_a_lower_bound_or_nothing() {
        assert_eq!(format_cost(Some(0.5), false), "est. $0.50");
        assert_eq!(format_cost(Some(0.5), true), "≥ $0.50");
        assert_eq!(format_cost(None, false), "—");
        assert_eq!(format_cost(None, true), "—");
    }

    #[test]
    fn a_small_cost_keeps_its_significant_digits() {
        assert_eq!(format_cost(Some(0.0001), false), "est. $0.0001");
        assert_eq!(format_cost(Some(0.0018), false), "est. $0.0018");
        assert_eq!(format_cost(Some(1.25), false), "est. $1.25");
        assert_eq!(format_cost(Some(0.0), false), "est. $0.00");
    }

    #[test]
    fn a_row_formats_its_cells_and_survives_a_deleted_job() {
        let row = RecentCall {
            id: 1,
            provider: "deepseek".into(),
            model: "deepseek-flash".into(),
            task: "evaluation".into(),
            job_title: Some("Engineer".into()),
            company: Some("Acme".into()),
            input_tokens: Some(8_120),
            output_tokens: Some(940),
            total_tokens: Some(9_060),
            source: "reported".into(),
            cost_usd: Some(0.0018),
            cost_complete: Some(true),
            outcome: "ok".into(),
            error_kind: None,
            duration_ms: 900,
            created_at: Utc.with_ymd_and_hms(2026, 11, 7, 14, 2, 0).unwrap(),
        };

        let formatted = RecentUsageRow::from(row);
        assert_eq!(formatted.when, "2026-11-07 14:02");
        assert_eq!(formatted.job, "Engineer · Acme");
        assert_eq!(formatted.tokens, "8,120 / 940");
        assert_eq!(formatted.cost, "est. $0.0018");
        assert_eq!(formatted.source, "reported");
        assert_eq!(formatted.outcome, "ok");

        let gone = RecentCall {
            id: 2,
            provider: "deepseek".into(),
            model: "deepseek-flash".into(),
            task: "evaluation".into(),
            job_title: None,
            company: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            source: "estimated".into(),
            cost_usd: None,
            cost_complete: None,
            outcome: "error".into(),
            error_kind: Some("transport".into()),
            duration_ms: 12,
            created_at: Utc.with_ymd_and_hms(2026, 11, 7, 14, 3, 0).unwrap(),
        };
        let formatted = RecentUsageRow::from(gone);
        assert_eq!(formatted.job, "—");
        assert_eq!(formatted.tokens, "—");
        assert_eq!(formatted.cost, "—");
    }

    #[test]
    fn the_view_model_is_empty_without_rows() {
        let view = UsageViewModel::from_rows(Range::All, Vec::new(), Vec::new(), Vec::new());
        assert!(view.is_empty());
        assert_eq!(view.range, Range::All);
        assert_eq!(view.totals.calls, 0);
    }
}
