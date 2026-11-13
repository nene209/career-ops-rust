# Phase 3.6 — PII Redaction Before LLM Calls: Retrospective

**Date:** 2026-11-13
**Plan:** `.agents/plans/phase_3.6_pii_redaction.md` (v0.5)
**Outcome:** all four workstreams implemented; fmt, clippy, and the offline suite
are green. The manual §6a.13 and §6d.10–6d.13 steps are the owner's.

---

## What shipped

- **`core::privacy` (WS1).** One policy value (`privacy.redact_cv`, default on,
  a malformed stored value stays on), the identity read from the profile the CV
  renderer already uses, a pure `redact(summary, identity, enabled)` and a
  storage-aware `redact_cv(pool, summary)`. Matching is a single left-to-right
  pass over longest-first candidates: the full name beats its parts at the same
  offset, tokens are never re-redacted, ASCII values fold case, and a non-ASCII
  value matches exactly. Name parts get `⟦NAME_1⟧`, `⟦NAME_2⟧`; the phone is
  matched literally and by digit run; the location only when it looks like a
  street address.
- **Both provider calls (WS2).** The CV is redacted after `take_chars` and before
  `render`, so the prompt, the character count, the context-window estimate, and
  the usage estimates describe what is actually sent. The evaluation restores
  `rationale`, `full_report_markdown`, `strengths`, and `gaps` **after** the JSON
  is parsed; the letter restores its free text before the insert. Each call emits
  one counts-only `redaction` line.
- **Visibility (WS3).** Settings → **Privacy**: the toggle with its own save, a
  one-line list of what is removed, a plainly-worded caveat, and **Preview what
  the model sees** — a window with the summary in monospace and a replacement
  count, following the checkbox as it is on screen. CV / Profile relabels
  **Location (city / region)**, adds the hint, and warns about an address-like
  value without changing what is saved.
- **Docs (WS4).** `docs/privacy.md` (what is redacted, what is not, tokens,
  restoration, the toggle, logging, tests, residual risk), manual **§6a.13** and
  **§6d.10–6d.13**, and cross-references from `docs/logging.md` and
  `docs/prompt-injection.md`.

## What held up

- **Redacting at the CV boundary, not the whole prompt.** It kept the change to
  one function and one call site per service, and it is the reason a posting's
  recruiter name is still visible to the model.
- **Restoring after the parse.** A name with a quote in it would have broken the
  JSON if the substitution had run first. The structural fields made this easy:
  only four prose fields can carry a placeholder.
- **Counts-only logging with no escape hatch.** The line has no field that could
  carry a value, so the rule is enforced by the shape of the event.
- **The preview as app-prepared text.** The screen stays pure and the test can
  render both modes without a database.

## Deviations, and why

1. **The restorer is built from the tokens the summary actually emitted.** The
   plan implied a fixed map. Building it from emitted tokens makes `tokens()`
   honest for the preview and makes `restored`/`unrestored` mean something. The
   consequence surfaced in a cover-letter test: a reply that invents `⟦NAME_1⟧`
   when the body never held the bare first name is stored as it arrived and
   counted, not restored. That is the right tradeoff — guessing which part of a
   name an invented token meant would be worse — and it is documented in
   `docs/privacy.md`.
2. **The phone has two matchers.** The plan listed "the stored string and any
   candidate run whose digits match". Both are implemented: the literal catches a
   stored value shorter than seven digits where it appears, the digit run catches
   a differently formatted occurrence of the same number.
3. **The preview follows the form's checkbox, not the saved value.** Comparing
   before committing is the point of a preview; the plan did not say which, and
   the form value is the more useful one.
4. **The window is titled "What the model sees", not "Preview what the model
   sees".** The button already carries that label, and two identical accessible
   names make `egui_kittest` selectors ambiguous (and a screen reader confusing).
5. **Two new `SettingsAction` variants rather than one combined save.** The AI
   section set the precedent: a failure has to name what failed, and preferences,
   the model, the key, and the toggle live in four different places.

## A test-infrastructure gotcha

`wait_until` in `crates/app/src/app.rs` is a hand-kept list of `poll_*` calls.
A new task family that is not added there does not fail to compile; it times out
after 20 seconds with "condition not met". That happened once during this phase.
When adding a polled task, add it to `wait_until` in the same commit.

## Test strategy as built

| Layer | What it pins |
| :--- | :--- |
| Unit (`privacy.rs`) | every source, the stoplist, the address heuristic, overlapping matches, no re-redaction, round-trip restore, the counts, the toggle, the config default |
| Core integration (`privacy.rs`) | stored toggle default/malformed/round trip, `redact_cv` against a real profile, an address-like location from a real summary |
| Core integration (`evaluation.rs`, `cover_letter.rs`) | the prompt carries tokens and no secret, the stored output is restored, the toggle off is a passthrough, a mangled token is stored as-is |
| `egui_kittest` (`settings_screen.rs`, `cv_profile_screen.rs`) | the toggle, a stored off value, the preview request, the counts line, the off warning, an error, the location hint and warning |
| App (`app.rs`) | the toggle loads and saves across a reload, and the preview reflects both modes |

## Follow-ups

- **A separate address column** stays deferred (the plan's Q). One field with a
  warning is cheaper and discourages storing an address at all.
- **Nicknames and split names** are the largest remaining gap. A small
  "additional names to redact" list in the profile is the obvious extension; it
  is a schema question (a JSON column, not a migration) and was not in scope.
- **Two-character names** are not redacted. Lowering the floor would redact
  ordinary prose; the preview shows the miss. Revisit if a user reports it.
- **Incremental streaming restoration** would remove the few seconds of
  placeholders. It needs a token-boundary buffer; the final text is already
  correct, so this is polish.
- **`unrestored > 0` is only visible in the log.** A one-line note in the
  evaluation tab ("the model wrote a placeholder it was never given") is a
  reasonable future touch; it is a display change, not a privacy one.
- **A redaction corpus.** The current tests are case-by-case. A fixture list of
  CV shapes (accented names, hyphenated surnames, international phones) would
  make the limits measurable rather than argued.

## Exit criteria

| Criterion | Status |
| :--- | :--- |
| 1. Name, email, phone, address-like location are replaced | Unit + core tests |
| 2. City/region passes through | Unit + core tests |
| 3. Restore returns the original | Round-trip test |
| 4. Counts reported without values | Unit test + log line |
| 5. The provider never receives the real values | Evaluation and letter tests |
| 6. The stored output is restored | Evaluation and letter tests |
| 7. Toggle off is a passthrough | Service and app tests |
| 8. The user can see exactly what is sent | Preview window + kittest |
| 9. Manual §6a/§6d, `docs/privacy.md`, parent plan, retrospective | Done (manual steps await the owner) |
