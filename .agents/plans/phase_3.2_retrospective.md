# Phase 3.2 — Assisted Triage and Portal Suggestions: Retrospective

**Date:** 2026-11-08
**Plan:** `.agents/plans/phase_3.2_assisted_triage.md` (v0.8)
**Outcome:** all five workstreams implemented; the offline gates are green and the
manual §6f pass is the owner's.

---

## What shipped

- **Triage contract (WS1).** `crates/core/prompts/triage_batch.md` seeded as
  `triage`, `core::triage::{TriageChoice, TriageDecision, TriageOutput}`, a
  parser that validates the batch, and `prompt_render`'s per-category variable
  sets.
- **Batch service (WS2).** Migration `0008`; `triage::estimate` and
  `triage::run_triage`; one usage row with `task = 'triage'` written before the
  reply is parsed; the whole decision set applied in one transaction; a 50-job
  default bound and a 100-job ceiling.
- **Review UI (WS3).** **Triage (N)** in the Pipeline header, a cost-estimate
  confirmation, progress with Stop, a completion summary, the **Triaged**
  filter, decision badges with the stored reason, and a dismissible override
  banner.
- **Portal suggestions (WS4).** One compact call that proposes companies,
  engines, and board slugs; URLs derived and validated locally; known-site
  configs resolved through the registry; a review modal with
  **Approve and add** / **Skip** / **Fetch after adding**; no portal is written
  before approval.
- **Docs and live checks (WS5).** Manual §6f, the triage cassette and live
  test, the suggestion live test, the `docs/known-sites.md` note, and this
  retrospective.

## What held up

- **One call per batch.** The triage prompt and the suggestion prompt each fit
  in a single small call, so the cost is one row and the UI is one state
  machine. Nothing in the implementation needed a per-job loop.
- **Advisory decisions in the existing queue.** Triage writes the same three
  queue values the Phase 2.9 detail actions write. There is no parallel state,
  so every decision is reversible with an action the user already knows.
- **The estimate split.** Making the estimate a separate local read (`estimate`)
  kept the confirmation honest: the modal shows tokens and a catalog price
  before a token is spent, and the run reports the provider's actual figure.
- **Derived URLs.** The suggestion contract never accepts a URL from the model.
  A reply can only choose an engine and one safe slug, so the `P3.2-R3` risk
  ("the model invents a portal URL") is closed structurally rather than by
  prompt wording.
- **Per-category variable validation.** `validate_for_category` caught the
  triage/suggest variables at save time in the Prompt Manager instead of at
  render time, which is the same lesson Phase 3 learned with `{{jobs}}`.
- **The cassette pattern generalised.** The triage cassette reuses the
  Phase 3 recorder (`CAREER_OPS_RECORD_CASSETTES=1`) with almost no new code,
  and the live test is the only place that spends tokens.

## What was harder, and the deviations

- **A one-frame completion line is not a line anyone can read.** The plan asked
  for the triage summary to survive one render. That is ~16 ms. The summary
  uses the existing `pipeline_message` instead, which persists until the next
  action, like every other pipeline action. The running line is what clears.
- **The Triage button counts more than one run covers.** It shows every new,
  non-excluded row, while one call covers at most 50. This matches
  **Fetch descriptions (N)**, which also shows the total flagged while a click
  covers 20. The confirmation states the batch size.
- **Triage candidates come from the loaded Pipeline page.** The list loads up
  to 500 rows; a backlog larger than that is triaged in page-sized batches as
  the user reloads. This is a natural consequence of "do not load the table
  twice" and is recorded as a follow-up rather than fixed here.
- **The suggestion service returns a report, not a list.** The plan's signature
  was `Vec<PortalSuggestion>`; the modal needs the model, cost, and elapsed
  time, and Stop needs a token. `suggest_portals(pool, cancel, backend) ->
  SuggestReport` is the smallest shape that serves both.
- **The suggestion cost is shown after the call.** There is no pre-call
  confirmation for suggestions, unlike triage. The call is cheap, nothing is
  written before approval, and a second confirmation before a read-only call
  would be noise. The modal still shows the actual-or-estimated cost.
- **A duplicate board is refused at approval, not hidden beforehand.** The
  modal does not know which URLs are already portals; approving one reports
  "`<company>` is already a portal" and the edit-time row's cursor is kept.
  Pre-filtering the proposals is a UI polish item (Q-52).
- **Exit criterion 8 is split.** The recorded cassette covers all four
  decisions end to end. The rejection paths (unknown choice, missing id,
  duplicate id, empty reason) are pinned by unit and scripted-backend tests
  instead of cassettes: a cassette records what the model actually sent, and
  these replies are ones a conforming model does not send. The live test is the
  only proof that the prompt still elicits the contract.

## Test strategy as built

| Layer | What it pins |
| :--- | :--- |
| Core unit | the output schema; the batch and suggestion validators; prompt size and the variable sets; URL derivation and slug safety |
| Core integration | one batch applied with provenance in one transaction; the four decisions; bounds; cancellation; failure; the usage rows; the suggestion report's known/derived configs; duplicate and unsafe-slug refusals |
| Cassette | the four triage decisions against the shipped recording |
| `egui_kittest` | the Triage button and modal, cancel, run, progress/Stop, the Triaged filter, badges, override, banner; the Suggest button, progress/Stop, the proposal modal, approve, Skip, Fetch after adding, duplicate refusal |
| App | the triage round trip with a scripted backend; suggestion → review → approve; fetch-after-adding; the duplicate guard |
| Live (ignored) | one real triage batch (also the cassette recorder); one real suggestion call |

## Follow-ups

1. **Manual §6f** is the owner's pass: run triage on a small batch, verify the
   reasons, override a Skip, and add a suggested portal.
2. **Q-50:** should **Triage (N)** cover the whole backlog rather than the
   loaded page? Doing so needs a count query and a "continue from here" cursor,
   which is Phase 2.8's shape, and is not worth it until a real vault exceeds
   500 new rows.
3. **Q-51:** should there be a way to re-triage a row whose queue state the user
   changed? Today only `new` rows are candidates, so an override is a one-way
   door unless the user resets the queue. A **Re-triage** detail action is the
   likely shape.
4. **Q-52:** should the suggestion modal hide boards that are already portals,
   or mark them? The create-time guard is correct but late; a pre-check would be
   a nicer review.
5. **The suggestion cassette** is deliberately absent; if the suggestion prompt
   starts drifting, add one with the same recorder and assert the URL hosts, not
   the companies.

## Exit criteria

All nine are met, with the split recorded above for criterion 8:

1. The compact prompt seeds, is under its ceiling, and records its version. ✅
2. Batch triage applies decisions in one transaction, bounded at 50,
   cancellable. ✅
3. Every triaged row records all five provenance columns. ✅
4. The UI shows the estimate, the completion summary, and a Triaged filter;
   every decision is overridable. ✅
5. Suggestions propose companies, engines, roles, and rationales; approval adds
   one portal through the registry; nothing is added without approval. ✅
6. Migration 0008 applies to a 0007 database and preserves rows. ✅
7. The offline suite, fmt, and clippy are clean. ✅
8. Cassette + scripted tests cover the four decisions and the rejection paths;
   live tests are indexed in `docs/LIVE_TESTS.md`. ✅ (see the split above)
9. Manual §6f, `docs/known-sites.md`, the parent plan, and this retrospective
   are updated. ✅
