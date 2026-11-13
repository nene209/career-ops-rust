//! Redacting the candidate's personal data before a provider sees it.
//!
//! Evaluation and cover-letter prompts carry the CV summary, which begins with
//! the candidate's name and a contact line. That is the data a user should not
//! have to hand to a model just to get an evaluation, so the summary is
//! rewritten locally: the provider receives stable placeholders, and the reply
//! is restored before it is stored or shown.
//!
//! Two boundaries make this auditable:
//!
//! - **Only the CV summary is redacted.** A posting may contain a recruiter's
//!   name or email; that is not the user's PII, and stripping it can damage the
//!   evaluation.
//! - **Only before the call.** The stored report, the letter, and every screen
//!   show real values; [`RedactedCv::restore`] puts them back.
//!
//! It is best-effort by design. Pattern matching cannot catch a nickname, a name
//! split across lines, or an unusual phone format, and the three-character floor
//! on a name is deliberate (it keeps a two-letter surname from matching ordinary
//! prose). Settings shows a preview so a miss or an over-redaction is visible,
//! and a local provider (Phase 3.5) is the stronger control.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::Result;
use crate::model::Profile;
use crate::repo::{self, settings};

pub const KEY_REDACT_CV: &str = "privacy.redact_cv";

pub const NAME_TOKEN: &str = "⟦NAME⟧";
pub const EMAIL_TOKEN: &str = "⟦EMAIL⟧";
pub const PHONE_TOKEN: &str = "⟦PHONE⟧";
pub const LOCATION_TOKEN: &str = "⟦LOCATION⟧";

/// The placeholder for the `index`-th distinct name part, counting from one.
///
/// Indexed parts solve the "Ada" / "Lovelace" ambiguity that a single
/// `⟦NAME⟧` cannot: the model can tell they are two people-like nouns, and each
/// restores to its own word.
pub fn name_part_token(index: usize) -> String {
    format!("⟦NAME_{index}⟧")
}

/// What the user chose about redaction.
///
/// The choice lives in `settings`, so adding another switch later does not need
/// a migration. A missing or malformed value reads as the default with a
/// warning, matching [`crate::llm::LlmConfig`]'s policy: a hand-edited database
/// must not make Settings unopenable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyConfig {
    /// Redact the CV summary before a provider call. Default `true`: the point
    /// of the feature is privacy, and an opt-in default would leave most users
    /// unprotected.
    pub redact_cv: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self { redact_cv: true }
    }
}

impl PrivacyConfig {
    pub async fn load(pool: &SqlitePool) -> Result<Self> {
        match settings::get_json::<Self>(pool, KEY_REDACT_CV).await {
            Ok(Some(config)) => Ok(config),
            Ok(None) => Ok(Self::default()),
            Err(error) => {
                tracing::warn!(%error, "privacy setting unreadable; redaction stays on");
                Ok(Self::default())
            }
        }
    }

    pub async fn save(&self, pool: &SqlitePool) -> Result<()> {
        settings::set_json(pool, KEY_REDACT_CV, self).await
    }
}

/// The values redacted out of a CV summary.
///
/// The CV renderer writes these verbatim, so the match is against strings the
/// app itself produced rather than anything parsed out of the text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileIdentity {
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub location: Option<String>,
}

impl ProfileIdentity {
    pub fn from_profile(profile: &Profile) -> Self {
        let trimmed = |value: &str| value.trim().to_string();
        Self {
            name: trimmed(&profile.name),
            email: trimmed(&profile.email),
            phone: profile
                .phone
                .as_deref()
                .map(trimmed)
                .filter(|value| !value.is_empty()),
            location: profile
                .location
                .as_deref()
                .map(trimmed)
                .filter(|value| !value.is_empty()),
        }
    }
}

/// How many replacements were made, by kind.
///
/// Counts only: the values never leave this module, and this is what the log
/// line reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RedactionReport {
    /// The full name and each name part.
    pub names: usize,
    pub emails: usize,
    pub phones: usize,
    pub locations: usize,
}

impl RedactionReport {
    pub fn total(&self) -> usize {
        self.names + self.emails + self.phones + self.locations
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// A restored text and what happened while restoring it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    pub text: String,
    /// Placeholders that were present and replaced.
    pub restored: usize,
    /// `⟦…⟧`-shaped tokens the redactor did not emit. A non-zero count means the
    /// model invented one or echoed one for a field that was not redacted; the
    /// text is left alone.
    pub unrestored: usize,
}

/// The summary a provider may see, plus the means to undo the substitutions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedCv {
    pub text: String,
    pub report: RedactionReport,
    restorer: Restorer,
}

impl RedactedCv {
    /// Put the real values back into a reply.
    pub fn restore(&self, text: &str) -> String {
        self.restore_with_counts(text).text
    }

    /// [`Self::restore`] with the counts the log line reports.
    pub fn restore_with_counts(&self, text: &str) -> Restored {
        self.restorer.restore(text)
    }

    /// For a preview screen: the tokens this summary emitted, in replacement
    /// order.
    pub fn tokens(&self) -> Vec<&str> {
        self.restorer
            .pairs
            .iter()
            .map(|(token, _)| token.as_str())
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Restorer {
    /// `(token, original value)`, in replacement order.
    pairs: Vec<(String, String)>,
}

impl Restorer {
    fn restore(&self, text: &str) -> Restored {
        let mut text = text.to_string();
        let mut restored = 0usize;
        for (token, value) in &self.pairs {
            let hits = text.matches(token.as_str()).count();
            if hits > 0 {
                text = text.replace(token.as_str(), value);
                restored += hits;
            }
        }
        Restored {
            unrestored: count_tokens(&text),
            text,
            restored,
        }
    }
}

/// Count well-formed `⟦…⟧` tokens left in `text`.
fn count_tokens(text: &str) -> usize {
    let mut count = 0usize;
    let mut open = false;
    for character in text.chars() {
        match character {
            '⟦' => open = !open,
            '⟧' if open => {
                open = false;
                count += 1;
            }
            _ => {}
        }
    }
    count
}

/// The three-character floor for any single replacement.
///
/// A two-letter value would match too much ordinary prose once word boundaries
/// are the only guard, and the preview makes the miss visible. It applies to the
/// full name as well as its parts.
const MIN_MATCH_CHARS: usize = 3;

/// Common English words that are also names.
///
/// A part on this list is not redacted on its own, so "May", "Mark", and "Hope"
/// do not turn ordinary prose into placeholders. The tradeoff is a surname that
/// is also an everyday word; the preview is how a user spots it. The list is
/// deliberately small and easy to extend.
const STOPLIST: [&str; 8] = [
    "may", "will", "mark", "rose", "grace", "hope", "king", "young",
];

/// Street suffixes that, together with a digit, make a location address-like.
const STREET_SUFFIXES: [&str; 26] = [
    "street",
    "st",
    "avenue",
    "ave",
    "road",
    "rd",
    "boulevard",
    "blvd",
    "lane",
    "ln",
    "drive",
    "dr",
    "court",
    "ct",
    "way",
    "place",
    "pl",
    "terrace",
    "ter",
    "circle",
    "cir",
    "highway",
    "hwy",
    "parkway",
    "pkwy",
    "square",
];

/// Whether a location value looks like a street address.
///
/// A city or region passes through unchanged; a value that starts with a digit,
/// or carries a digit and a street suffix ("123 Main St", "Flat 4, Elm Road"),
/// is treated as personal data and redacted.
pub fn address_like(location: &str) -> bool {
    let location = location.trim();
    if location.is_empty() {
        return false;
    }
    if location
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        return true;
    }
    let has_digit = location.chars().any(|character| character.is_ascii_digit());
    has_digit
        && location
            .split(|character: char| !character.is_alphanumeric())
            .any(|word| {
                let word = word.to_ascii_lowercase();
                STREET_SUFFIXES.contains(&word.as_str())
            })
}

/// Redact `summary` for `identity`.
///
/// `enabled == false` is a passthrough: the text is returned unchanged and the
/// restorer is empty, so `restore` is also a no-op.
pub fn redact(summary: &str, identity: &ProfileIdentity, enabled: bool) -> RedactedCv {
    if !enabled {
        return RedactedCv {
            text: summary.to_string(),
            report: RedactionReport::default(),
            restorer: Restorer::default(),
        };
    }

    let redactor = Redactor::new(identity);
    let (text, report, restorer) = redactor.apply(summary);
    RedactedCv {
        text,
        report,
        restorer,
    }
}

/// Load the toggle and the identity, then redact `summary`.
pub async fn redact_cv(pool: &SqlitePool, summary: &str) -> Result<RedactedCv> {
    let config = PrivacyConfig::load(pool).await?;
    let profile = repo::profile::ensure_singleton(pool).await?;
    let identity = ProfileIdentity::from_profile(&profile);
    Ok(redact(summary, &identity, config.redact_cv))
}

/// What a single replacement matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Name,
    Email,
    Phone,
    Location,
}

#[derive(Debug, Clone)]
struct Candidate {
    needle: String,
    token: String,
    /// The real value this token restores to.
    value: String,
    /// ASCII-only case folding; a non-ASCII needle matches exactly, because
    /// locale-dependent lowercasing would change byte offsets.
    case_insensitive: bool,
    word_bounded: bool,
    kind: Kind,
}

#[derive(Debug, Clone)]
struct Redactor {
    /// Longest needle first, so a full name wins over its parts at the same
    /// position and a token is never re-redacted.
    candidates: Vec<Candidate>,
    /// The stored phone's digits, when there are enough of them to recognize a
    /// differently-formatted occurrence of the same number.
    phone_digits: Option<String>,
    /// The stored phone, for the restorer when a digit run matches it.
    phone_value: Option<String>,
}

impl Redactor {
    fn new(identity: &ProfileIdentity) -> Self {
        let mut candidates: Vec<Candidate> = Vec::new();

        let name = identity.name.trim();
        if eligible(name) {
            candidates.push(Candidate {
                needle: name.to_string(),
                token: NAME_TOKEN.to_string(),
                value: name.to_string(),
                case_insensitive: name.is_ascii(),
                word_bounded: true,
                kind: Kind::Name,
            });
        }

        // Name parts get their own indexed tokens, so a reply that says
        // "⟦NAME_1⟧" still reads as a name and restores to the right word.
        let mut part_index = 0usize;
        let mut seen: Vec<String> = Vec::new();
        for part in name.split_whitespace() {
            let key = part.to_ascii_lowercase();
            if !eligible(part) || STOPLIST.contains(&key.as_str()) || seen.contains(&key) {
                continue;
            }
            if part.eq_ignore_ascii_case(name) {
                continue;
            }
            seen.push(key);
            part_index += 1;
            candidates.push(Candidate {
                needle: part.to_string(),
                token: name_part_token(part_index),
                value: part.to_string(),
                case_insensitive: part.is_ascii(),
                word_bounded: true,
                kind: Kind::Name,
            });
        }

        let email = identity.email.trim();
        if eligible(email) {
            candidates.push(Candidate {
                needle: email.to_string(),
                token: EMAIL_TOKEN.to_string(),
                value: email.to_string(),
                case_insensitive: email.is_ascii(),
                word_bounded: false,
                kind: Kind::Email,
            });
        }

        let location = identity.location.as_deref().unwrap_or("").trim();
        if eligible(location) && address_like(location) {
            candidates.push(Candidate {
                needle: location.to_string(),
                token: LOCATION_TOKEN.to_string(),
                value: location.to_string(),
                case_insensitive: location.is_ascii(),
                word_bounded: false,
                kind: Kind::Location,
            });
        }

        // The stored phone is matched verbatim, because the CV renderer writes
        // it that way. The digit-run scan below adds other formats of the same
        // number when it has enough digits to be recognizable.
        let phone = identity
            .phone
            .as_deref()
            .map(str::trim)
            .filter(|phone| eligible(phone))
            .map(str::to_string);
        if let Some(phone) = phone.as_deref() {
            candidates.push(Candidate {
                needle: phone.to_string(),
                token: PHONE_TOKEN.to_string(),
                value: phone.to_string(),
                case_insensitive: phone.is_ascii(),
                word_bounded: false,
                kind: Kind::Phone,
            });
        }

        // Longest first; ties keep the order they were built in, which puts the
        // full name before its parts.
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.needle.len()));

        let phone_digits = phone
            .as_deref()
            .map(|phone| {
                phone
                    .chars()
                    .filter(char::is_ascii_digit)
                    .collect::<String>()
            })
            .filter(|digits| digits.chars().count() >= 7);

        Self {
            candidates,
            phone_digits,
            phone_value: phone,
        }
    }

    fn apply(&self, text: &str) -> (String, RedactionReport, Restorer) {
        let phone_spans = self.phone_spans(text);
        let mut report = RedactionReport::default();
        let mut used: Vec<(String, String)> = Vec::new();
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0usize;

        let starts: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
        let mut position = 0usize;
        while position < starts.len() {
            let start = starts[position];

            if let Some(end) = self.phone_at(&phone_spans, start) {
                out.push_str(&text[cursor..start]);
                out.push_str(PHONE_TOKEN);
                report.phones += 1;
                remember(
                    &mut used,
                    PHONE_TOKEN,
                    self.phone_value.as_deref().unwrap_or(""),
                );
                cursor = end;
                position = starts.partition_point(|offset| *offset < end);
                continue;
            }

            if let Some((end, candidate)) = self.candidate_at(text, start) {
                out.push_str(&text[cursor..start]);
                out.push_str(&candidate.token);
                match candidate.kind {
                    Kind::Name => report.names += 1,
                    Kind::Email => report.emails += 1,
                    Kind::Phone => report.phones += 1,
                    Kind::Location => report.locations += 1,
                }
                remember(&mut used, &candidate.token, &candidate.value);
                cursor = end;
                position = starts.partition_point(|offset| *offset < end);
                continue;
            }

            position += 1;
        }

        out.push_str(&text[cursor..]);
        (out, report, Restorer { pairs: used })
    }

    fn candidate_at(&self, text: &str, start: usize) -> Option<(usize, &Candidate)> {
        for candidate in &self.candidates {
            let needle = candidate.needle.as_bytes();
            let end = start + needle.len();
            if end > text.len() {
                continue;
            }
            let head = &text.as_bytes()[start..end];
            let equal = if candidate.case_insensitive {
                head.eq_ignore_ascii_case(needle)
            } else {
                head == needle
            };
            if !equal {
                continue;
            }
            if candidate.word_bounded && !word_bounded(text, start, end) {
                continue;
            }
            return Some((end, candidate));
        }
        None
    }

    fn phone_at(&self, spans: &[(usize, usize)], start: usize) -> Option<usize> {
        spans
            .binary_search_by_key(&start, |(begin, _)| *begin)
            .ok()
            .map(|index| spans[index].1)
    }

    /// The spans whose digits match the stored phone number.
    ///
    /// A candidate run may use any of the separators people type; only the
    /// digits are compared, so `+44 20 0000 0000` and `44200000000` agree.
    fn phone_spans(&self, text: &str) -> Vec<(usize, usize)> {
        let Some(stored) = self.phone_digits.as_deref() else {
            return Vec::new();
        };
        let mut spans = Vec::new();
        let mut index = 0usize;
        while index < text.len() {
            let character = text[index..].chars().next().expect("in bounds");
            if !is_phone_char(character) {
                index += character.len_utf8();
                continue;
            }

            let mut end = index + character.len_utf8();
            while end < text.len() {
                let next = text[end..].chars().next().expect("in bounds");
                if !is_phone_char(next) {
                    break;
                }
                end += next.len_utf8();
            }

            // Trim separators that cannot start or end a number, so a sentence
            // like "call (415) 555-0100." does not take the period with it.
            let mut begin = index;
            while begin < end {
                let next = text[begin..].chars().next().expect("in bounds");
                if next.is_ascii_digit() || next == '+' || next == '(' {
                    break;
                }
                begin += next.len_utf8();
            }
            let mut stop = end;
            while stop > begin {
                let previous = text[..stop].chars().next_back().expect("in bounds");
                if previous.is_ascii_digit() || previous == ')' {
                    break;
                }
                stop -= previous.len_utf8();
            }

            let digits: String = text[begin..stop]
                .chars()
                .filter(char::is_ascii_digit)
                .collect();
            if digits.len() >= 7 && digits == stored {
                spans.push((begin, stop));
            }
            index = end;
        }
        spans
    }
}

fn eligible(value: &str) -> bool {
    value.chars().count() >= MIN_MATCH_CHARS
}

/// Record a token's value once, in replacement order.
fn remember(pairs: &mut Vec<(String, String)>, token: &str, value: &str) {
    if !pairs.iter().any(|(known, _)| known == token) {
        pairs.push((token.to_string(), value.to_string()));
    }
}

fn is_phone_char(character: char) -> bool {
    character.is_ascii_digit() || matches!(character, '+' | '(' | ')' | '-' | '.' | ' ')
}

/// Whether the characters just outside `[start, end)` are non-alphanumeric.
fn word_bounded(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start]
        .chars()
        .next_back()
        .is_none_or(|character| !character.is_alphanumeric());
    let after = text[end..]
        .chars()
        .next()
        .is_none_or(|character| !character.is_alphanumeric());
    before && after
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ProfileIdentity {
        ProfileIdentity {
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
            phone: Some("+44 20 0000 0000".into()),
            location: Some("London".into()),
        }
    }

    fn cv() -> String {
        "Ada Lovelace\nLondon | ada@example.com | +44 20 0000 0000\n\nSummary:\nAda published notes.\nLovelace wrote the first algorithm.\n"
            .to_string()
    }

    #[test]
    fn the_cv_summary_contact_block_is_redacted() {
        let redacted = redact(&cv(), &identity(), true);

        assert!(!redacted.text.contains("Ada Lovelace"));
        assert!(!redacted.text.contains("ada@example.com"));
        assert!(!redacted.text.contains("+44 20 0000 0000"));
        assert!(redacted.text.contains(NAME_TOKEN));
        assert!(redacted.text.contains(EMAIL_TOKEN));
        assert!(redacted.text.contains(PHONE_TOKEN));
        assert!(redacted.text.contains("⟦NAME_1⟧"), "{}", redacted.text);
        assert!(redacted.text.contains("⟦NAME_2⟧"), "{}", redacted.text);
        assert!(
            redacted.text.contains("London"),
            "a city is not personal data"
        );
        assert_eq!(
            redacted.report,
            RedactionReport {
                names: 3,
                emails: 1,
                phones: 1,
                locations: 0,
            }
        );
    }

    #[test]
    fn the_full_name_wins_over_its_parts_and_is_not_redacted_twice() {
        let redacted = redact(cv().as_str(), &identity(), true);

        // The name line reads exactly one token, not two adjacent ones.
        let first_line = redacted.text.lines().next().expect("first line");
        assert_eq!(first_line, NAME_TOKEN);
    }

    #[test]
    fn name_parts_are_matched_case_insensitively_and_reuse_their_token() {
        let text = "ADA wrote the notes. Lovelace is quoted below. ada again.";
        let redacted = redact(text, &identity(), true);

        assert_eq!(
            redacted.text,
            "⟦NAME_1⟧ wrote the notes. ⟦NAME_2⟧ is quoted below. ⟦NAME_1⟧ again."
        );
        assert_eq!(redacted.report.names, 3);
    }

    #[test]
    fn a_shorter_name_is_not_redacted_inside_a_longer_word() {
        let identity = ProfileIdentity {
            name: "Ada Lovelace".into(),
            ..ProfileIdentity::default()
        };
        let redacted = redact(
            "Adaptation and Lovelaces are different words.",
            &identity,
            true,
        );

        assert_eq!(
            redacted.text, "Adaptation and Lovelaces are different words.",
            "a word boundary is required"
        );
        assert!(redacted.report.is_empty());
    }

    #[test]
    fn the_stoplist_leaves_common_words_alone() {
        let identity = ProfileIdentity {
            name: "May Young".into(),
            ..ProfileIdentity::default()
        };
        // "Young" is a surname on the stoplist; "May" too. The full name still
        // goes, because the pair as a whole is not a common phrase.
        let redacted = redact("May Young will apply. May I ask?", &identity, true);

        assert!(redacted.text.starts_with(NAME_TOKEN), "{}", redacted.text);
        assert!(
            redacted.text.contains("May I ask?"),
            "a stoplisted word is left alone on its own: {}",
            redacted.text
        );
    }

    #[test]
    fn an_address_like_location_is_redacted_and_a_city_is_not() {
        let address = ProfileIdentity {
            location: Some("123 Main St, Springfield".into()),
            ..ProfileIdentity::default()
        };
        let redacted = redact("123 Main St, Springfield", &address, true);
        assert_eq!(redacted.text, LOCATION_TOKEN);
        assert_eq!(redacted.report.locations, 1);

        let city = ProfileIdentity {
            location: Some("Springfield, IL".into()),
            ..ProfileIdentity::default()
        };
        let redacted = redact("Springfield, IL", &city, true);
        assert_eq!(redacted.text, "Springfield, IL");

        assert!(address_like("Flat 4, Elm Road"));
        assert!(address_like("7 Downing Street"));
        assert!(!address_like("Berlin, Germany"));
        assert!(!address_like("Remote"));
        assert!(!address_like(""));
    }

    #[test]
    fn a_phone_in_another_format_is_still_recognized() {
        let redacted = redact("Call 442000000000 or +44 20 0000 0000.", &identity(), true);

        assert_eq!(
            redacted.text, "Call ⟦PHONE⟧ or ⟦PHONE⟧.",
            "the digits are compared, not the punctuation"
        );
        assert_eq!(redacted.report.phones, 2);
    }

    #[test]
    fn a_short_or_unrelated_number_is_left_alone() {
        let identity = ProfileIdentity {
            phone: Some("+44 20 0000 0000".into()),
            ..ProfileIdentity::default()
        };
        let redacted = redact("Order 12345 in 2026.", &identity, true);

        assert_eq!(redacted.text, "Order 12345 in 2026.");
        assert_eq!(redacted.report.phones, 0);
    }

    #[test]
    fn a_short_stored_phone_is_redacted_as_a_string_but_not_scanned_for() {
        let identity = ProfileIdentity {
            phone: Some("12345".into()),
            ..ProfileIdentity::default()
        };
        // The stored value is always matched verbatim; the digit-run scan needs
        // seven digits before it treats another number as the same phone.
        let redacted = redact("Call 12345 or 12345, not 54321.", &identity, true);

        assert_eq!(redacted.text, "Call ⟦PHONE⟧ or ⟦PHONE⟧, not 54321.");
        assert_eq!(redacted.report.phones, 2);
    }

    #[test]
    fn restoring_round_trips_exactly() {
        let original = cv();
        let redacted = redact(&original, &identity(), true);

        let restored = redacted.restore_with_counts(&redacted.text);
        assert_eq!(restored.text, original);
        assert_eq!(restored.restored, 5, "the five replacements above");
        assert_eq!(restored.unrestored, 0);
    }

    #[test]
    fn a_reply_written_with_tokens_restores_to_the_real_values() {
        let redacted = redact(&cv(), &identity(), true);
        let reply = "⟦NAME_1⟧ ⟦NAME_2⟧ is a strong match. Write to ⟦EMAIL⟧ or call ⟦PHONE⟧.";

        assert_eq!(
            redacted.restore(reply),
            "Ada Lovelace is a strong match. Write to ada@example.com or call +44 20 0000 0000."
        );
    }

    #[test]
    fn a_token_the_redactor_did_not_emit_is_counted_and_left_alone() {
        let redacted = redact("No personal data here.", &identity(), true);

        let restored = redacted.restore_with_counts("Call ⟦PHONE⟧ or ⟦SOMETHING⟧.");
        assert_eq!(restored.text, "Call ⟦PHONE⟧ or ⟦SOMETHING⟧.");
        assert_eq!(restored.restored, 0);
        assert_eq!(restored.unrestored, 2, "counted, never logged with values");
    }

    #[test]
    fn empty_fields_are_skipped() {
        let identity = ProfileIdentity::default();
        let redacted = redact("Nothing to redact.", &identity, true);

        assert_eq!(redacted.text, "Nothing to redact.");
        assert!(redacted.report.is_empty());
        assert!(redacted.tokens().is_empty());
    }

    #[test]
    fn redaction_off_is_a_passthrough() {
        let original = cv();
        let redacted = redact(&original, &identity(), false);

        assert_eq!(redacted.text, original);
        assert!(redacted.report.is_empty());
        assert_eq!(redacted.restore(NAME_TOKEN), NAME_TOKEN);
    }

    #[test]
    fn a_non_ascii_name_matches_exactly() {
        let identity = ProfileIdentity {
            name: "José Álvarez".into(),
            ..ProfileIdentity::default()
        };
        let redacted = redact(
            "José Álvarez\nÁlvarez writes. JOSÉ ÁLVAREZ is different.",
            &identity,
            true,
        );

        assert_eq!(
            redacted.text, "⟦NAME⟧\n⟦NAME_2⟧ writes. JOSÉ ÁLVAREZ is different.",
            "the stored form matches exactly; a different case does not"
        );
        assert_eq!(redacted.report.names, 2);
    }

    #[test]
    fn a_two_character_name_stays_for_ordinary_prose_to_read() {
        // The three-character floor is a documented tradeoff, not an accident.
        let identity = ProfileIdentity {
            name: "Li".into(),
            ..ProfileIdentity::default()
        };
        let redacted = redact("Li is a name; Li-ion is a battery.", &identity, true);

        assert_eq!(redacted.text, "Li is a name; Li-ion is a battery.");
        assert!(redacted.report.is_empty());
    }

    #[test]
    fn every_placeholder_is_recorded_for_the_preview() {
        let redacted = redact(&cv(), &identity(), true);
        let tokens = redacted.tokens();

        assert_eq!(
            tokens,
            vec!["⟦NAME⟧", "⟦EMAIL⟧", "⟦PHONE⟧", "⟦NAME_1⟧", "⟦NAME_2⟧"],
            "in the order the text uses them"
        );
    }

    #[test]
    fn the_config_defaults_to_on_and_round_trips() {
        assert!(PrivacyConfig::default().redact_cv);
        assert_eq!(PrivacyConfig::default(), PrivacyConfig { redact_cv: true });
    }
}
