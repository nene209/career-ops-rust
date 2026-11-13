# Phase 3.6: PII Redaction Before LLM Calls — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.53), §12
**Status:** Planned — not started
**Last updated:** 2026-11-09
**Duration:** 3–4 days
**Depends on:** Phase 3 (`evaluation` and `cover_letter` services), Phase 2.7 (`render_cv_summary`), Phase 3.5 (the local-provider option, not a technical dependency)
**Blocks:** nothing.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Redacts the candidate's name, email, phone, and address-like location from the CV summary before evaluation and cover-letter calls, then restores the real values in the returned text. On by default, with a Settings preview. Location stays one **City / region** field; a separate address column is deferred. Streaming keeps its placeholders until the final text is restored. No migration; no new dependency. |

---

## Purpose

Evaluation and cover-letter prompts send the full CV summary, which begins with
the candidate's name and a contact line (`location | email | phone`). That is
exactly the data a user should not have to hand to a provider just to get a job
evaluation. Redaction is best-effort and local: the model sees stable
placeholders, the reply is restored before it is stored or shown, and the
provider never receives the real values.

Triage and portal suggestions do not send the CV, so they are unchanged.

### Evidence

- `repo::profile::render_cv_summary` writes `{name}` and
  `{location} | {email} | {phone}` (`crates/core/src/repo/profile.rs`).
- `evaluation::run_evaluation` and `cover_letter::run_cover_letter` call
  `profile::cv_summary` and substitute it as `{{cv_summary}}`.
- `triage` and `suggest` build their prompts without the CV.
- The profile has a single `location` field and no `address` column (0001).
- Evaluation replies are parsed into `EvaluationOutput { strengths, gaps,
  rationale, full_report_markdown, .. }`; the letter is free text.
- Streaming deltas are displayed before the final text is assembled.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Redact name, email, phone, address-like location from the CV summary | Redacting job-posting text |
| Restore placeholders in evaluation and cover-letter output | A general DLP scanner |
| Default-on toggle and a Settings preview | A separate mailing-address column |
| City/region label and address-like heuristic | Rendering an address on the CV/PDF |
| Counts-only logging | Incremental streaming restoration |
| Tests, docs, manual steps, retrospective | A local model runtime (Phase 3.5) |

---

## Decision check

1. **Redact at the CV-summary boundary, not the whole prompt.** The CV is the
   candidate's data. A posting may contain a recruiter's name or email; that is
   not the user's PII, and stripping it can damage the evaluation. The boundary
   is one function, which keeps the feature auditable.
2. **Default on.** The feature's purpose is privacy; an opt-in default would
   leave most users unprotected. The toggle and preview make the behavior
   visible, and a local provider (3.5) is the stronger control.
3. **Tokens, not blanks.** The model needs a stable noun. `⟦NAME⟧`,
   `⟦EMAIL⟧`, `⟦PHONE⟧`, `⟦NAME_1⟧`, `⟦LOCATION⟧` keep the prompt readable and
   are restored by exact substitution. Indexed name parts solve the
   "Ada" / "Lovelace" ambiguity that one `⟦NAME⟧` token cannot.
4. **Restore after parsing.** For evaluation, restore `strengths`, `gaps`,
   `rationale`, and `full_report_markdown` **after** the JSON is parsed. Restoring
   first could inject quotes or newlines into the JSON. For a letter, restore the
   free text directly.
5. **Streaming shows placeholders.** The live deltas are a preview; the stored
   and final displayed text is restored. An incremental restorer would need a
   token-boundary buffer for every delta and is not worth the complexity now.
6. **One location field, labeled City / region.** A separate `address` column
   would encourage storing PII and would have to be rendered by the CV/PDF path.
   The existing field stays; if it looks like a street address it is redacted
   even when redaction of the city is not desired.
7. **Best-effort, stated plainly.** Pattern matching cannot catch every variant
   (a nickname, a name split across lines, an unusual phone format). The UI and
   `docs/privacy.md` say so; the local provider is the guarantee, not the
   redactor.

---

## Design decisions

### 1. One module, one policy value

New `core::privacy`:

```rust
pub struct PrivacyConfig {
    /// Redact the CV summary before a provider call. Default true.
    pub redact_cv: bool,
}
```

Stored under `settings` key `privacy.redact_cv`; missing or malformed reads as
`true` with a warning, matching the `LlmConfig` policy. No migration.

```rust
pub struct ProfileIdentity {
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub location: Option<String>,
}

pub struct RedactedCv {
    pub text: String,
    /// Counts only; never the values.
    pub report: RedactionReport,
    restorer: Restorer,
}

impl RedactedCv {
    pub fn restore(&self, text: &str) -> String;
}
```

### 2. What is redacted

| Source | Match | Token |
| :--- | :--- | :--- |
| Full name | exact, plus ASCII case-insensitive, word-bounded | `⟦NAME⟧` |
| Name parts (each word ≥ 3 chars, not a common word) | ASCII case-insensitive, word-bounded | `⟦NAME_1⟧`, `⟦NAME_2⟧`, … |
| Email | exact, ASCII case-insensitive | `⟦EMAIL⟧` |
| Phone | the stored string and any candidate run whose digits match (≥ 7 digits) | `⟦PHONE⟧` |
| Location | only when it looks like a street address | `⟦LOCATION⟧` |

**Common-word stoplist** for name parts (so "May", "Will", "Mark", "Rose",
"Grace", "Hope", "King", "Young" are not redacted everywhere): a small, documented
list. The preview makes an over-redaction visible; the toggle is the escape
hatch.

**Address-like heuristic:** true when the location starts with a digit, or
contains a digit and a street suffix (`street`, `st`, `avenue`, `ave`, `road`,
`rd`, `boulevard`, `blvd`, `lane`, `ln`, `drive`, `dr`, `court`, `ct`, `way`,
`place`, `pl`, `terrace`, `ter`, `circle`, `cir`, `highway`, `hwy`, `parkway`,
`pkwy`, `square`, `sq`). A city or region passes through unchanged.

### 3. Matching and replacement

- Replacements are sorted longest-first and applied in one pass, so a token is
  never re-redacted and a full name wins over its parts.
- Email is exact; phone scanning walks digit-bearing runs and compares normalized
  digits, so `+44 20 0000 0000` and `44200000000` both match.
- Non-ASCII names use the exact stored form (the CV renderer writes it verbatim);
  ASCII gets case-insensitive matching. This avoids locale-dependent lowercasing
  that would change byte offsets.
- Empty values are skipped. A redaction that would match fewer than three
  characters is skipped. No new dependency.

### 4. Restoration

`Restorer` maps each token to its original value. `restore` replaces exact tokens
and returns the text. A reply that contains a token the restorer did not emit is
left alone and logged as `redaction_unrestored` (count only). The stored report
and letter contain real values; the provider never sees them.

### 5. Service integration

`evaluation::run_evaluation`:

1. load `PrivacyConfig`;
2. render the CV summary as today, then `privacy::redact_cv(pool, summary)`;
3. substitute the redacted text as `{{cv_summary}}`;
4. parse the reply, then restore `strengths`, `gaps`, `rationale`, and
   `full_report_markdown` before storing and returning.

`cover_letter::run_cover_letter` does the same for the CV and restores the free
text before storing. Triage and suggest are untouched.

The input-character count and the context-window check use the redacted prompt,
which is what is sent. The prompt-version hash is unchanged (it hashes the
template, not the substituted values).

### 6. Settings and preview

A **Privacy** section in Settings:

- **Redact personal details before sending to the model** (default on), with a
  one-line list of what is removed.
- **Preview what the model sees** opens a read-only monospace rendering of the
  redacted CV summary. If redaction is off, it shows the raw summary with a
  warning.
- A note that the reply is restored locally, that placeholders may appear while
  streaming, that the log records counts only, and that redaction is
  best-effort. A link to **Local / custom provider** (Phase 3.5) for users who
  want the stronger control.

The CV / Profile screen relabels **Location** to **Location (city / region)** and
adds a hint: "A city or region. A street address is treated as personal data and
redacted before sending." If the value looks like an address, a warning appears
below the field but the value is saved unchanged.

### 7. Logging and data rules

- One `info` line per redacted call: `redaction redacted={n} names={n}
  emails={n} phones={n} locations={n} restored={n} unrestored={n}` — counts only.
- Never log the values, the redacted summary, or the prompt. The existing
  `docs/logging.md` rule already forbids prompts and completions.
- The usage ledger records token counts and the model; it never records text.

### 8. Fixtures and cassettes

The Phase 3 fixtures use synthetic names, so they remain valid. If a re-record
changes a cassette, the recorded prompt now contains tokens, which is exactly what
we want; the reply may contain tokens and the restorer handles them. The
offline tests assert the redacted request and the restored output, not a
particular name.

---

## Workstream 1 — `core::privacy`

**Estimated duration:** 1–1.5 days.

### Deliverables

1. `PrivacyConfig` load/save with error-safe defaults.
2. `ProfileIdentity`, `Redactor`, `Restorer`, `RedactionReport`, `address_like`.
3. `redact_cv(pool, summary) -> Result<RedactedCv>` (loads the identity and the
   config; a passthrough when the toggle is off).
4. Unit tests:
   - full name, each name part, email, phone in several formats;
   - address-like location redacted, city/region kept;
   - overlapping matches (full name vs parts), multiple occurrences;
   - a token is not re-redacted; restore round-trips exactly;
   - the stoplist leaves common words alone;
   - empty/blank fields are skipped;
   - redaction off returns the input unchanged;
   - non-ASCII exact matching.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Name, email, phone, address-like location are replaced | Unit tests |
| City/region passes through | Unit test |
| Restore returns the original text | Round-trip test |
| Counts are reported without values | Unit test |

---

## Workstream 2 — Service integration

**Estimated duration:** 1 day.

### Deliverables

1. Evaluation: redact before render; restore the four prose fields after parse.
2. Cover letter: redact before render; restore before store.
3. `input_chars`/context checks use the redacted prompt.
4. A `redaction` info line with counts.
5. Tests with a scripted backend:
   - the prompt the backend receives contains no email, phone, full name, or
     address-like location, and does contain the tokens;
   - the stored report and letter contain the restored values;
   - the toggle off sends the raw summary;
   - a reply that echoes a token is restored;
   - a reply that mangles a token is stored as-is and counted unrestored;
   - triage and suggest prompts are unchanged.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The provider never receives the real values | Scripted-backend test |
| The stored output is restored | Evaluation and letter tests |
| Toggle off is a passthrough | Service test |

---

## Workstream 3 — Settings and profile UI

**Estimated duration:** 1 day.

### Deliverables

1. Settings **Privacy** section with the default-on toggle and the preview modal.
2. CV / Profile **Location (city / region)** label, hint, and address-like
   warning.
3. `egui_kittest`:
   - the toggle defaults on and survives a reload;
   - the preview shows tokens when on and the raw summary when off;
   - the location hint is present and the warning appears for an address-like
     value;
   - the provider/privacy copy is visible.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The user can see exactly what is sent | Preview test |
| The toggle changes the preview and the saved value | UI + app tests |
| The location guidance is visible | `egui_kittest` |

---

## Workstream 4 — Docs, manual, retrospective

**Estimated duration:** 0.5–1 day.

1. Manual script:
   - §6a: the **Location (city / region)** label and the address-like warning.
   - §6d: with redaction on, **Preview what the model sees** shows tokens; a
     real evaluation stores the report with the candidate's name restored; the
     cover letter's signature uses the real name; the logs show a `redaction`
     line with counts only.
   - §6d: turn redaction off and confirm the preview shows the raw summary.
2. `docs/privacy.md`: what is redacted, what is not (job postings, nicknames,
   unusual formats), the token format, restoration, streaming placeholders, the
   counts-only log rule, and the local-provider recommendation.
3. Cross-reference from `docs/prompt-injection.md` and `docs/logging.md`.
4. `main.md` v0.48 row and the plan changelog; retrospective with the matching
   limits and the deferred address column.
5. Full validation.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Unit | matching, tokens, stoplist, address heuristic, restore, counts |
| Core | evaluation/letter prompt has no PII, output restored, toggle off, token edge cases |
| Triage/suggest | unchanged prompts |
| `egui_kittest` | toggle, preview, location hint/warning |
| Manual | preview, evaluation, cover letter, logs |

No migration. No new dependency. No network in the tests (scripted backend).

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P3.6-R1 | **Over-redaction mangles the prompt** (a name that is a common word). | Medium | Stoplist, preview, toggle; the full name is always safe. |
| P3.6-R2 | **Under-redaction leaks a nickname or a split name.** | High | Document best-effort; redact full name plus parts; recommend the local provider. |
| P3.6-R3 | **The model rewrites a token**, so restoration misses. | Medium | Exact restore, `unrestored` count, a visible note; the stored text is still local. |
| P3.6-R4 | **Restoring before JSON parsing breaks the reply.** | High | Parse first, restore the prose fields after. |
| P3.6-R5 | **Streaming shows placeholders.** | Low | Documented; final text is restored. |
| P3.6-R6 | **The address heuristic misses a spelled-out address.** | Medium | The label and hint steer users to a city; a strict "redact location" toggle is a future option. |
| P3.6-R7 | **A prompt or value reaches the log.** | High | Counts-only logging; the existing never-log rule; a test asserting no value appears. |

---

## Exit Criteria

1. Redaction is on by default and can be turned off.
2. Evaluation and cover-letter requests contain no name, email, phone, or
   address-like location from the profile.
3. The stored report and letter contain the restored real values.
4. City/region passes through; an address-like location is redacted.
5. The Settings preview shows exactly what the model sees.
6. Logs contain counts only; no prompt, value, or completion is logged.
7. Triage and suggest are unchanged.
8. Offline suite, fmt, and clippy are clean.
9. Manual §6a/§6d, `docs/privacy.md`, the parent plan, and the retrospective are
   updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-68 | **Should job-posting contact details be redacted too?** | No. They are not the candidate's PII and stripping them can distort the evaluation. Revisit if a posting is found to carry the user's own data. |
| Q-69 | **Add a separate `address` field?** | No. It would encourage storing PII and force the CV/PDF path to render it. A city/region is enough. |
| Q-70 | **Incremental streaming restoration?** | Not now. The final text is restored; live deltas may show tokens. |
| Q-71 | **A strict "redact location even when it is a city" toggle?** | Not now. The address-like heuristic plus the local provider cover the common cases. |
| Q-72 | **Should the preview live on the CV screen instead of Settings?** | Settings. It is a send-time policy; the CV screen shows the real CV. |

---

## What Phase 3.6 Is Not

- Not a guarantee of anonymity; it is best-effort pattern matching.
- Not a job-posting sanitizer.
- Not a new profile schema or an address field.
- Not a local model runtime (Phase 3.5).
- Not a change to evaluation scoring, prompts, or the usage ledger.
