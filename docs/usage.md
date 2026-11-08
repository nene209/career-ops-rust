# LLM Usage

**Last reviewed:** 2026-11-07

Every provider call the app makes writes one row to `llm_usage`. The **Usage**
screen reads that ledger; this file records what is stored, what the numbers
mean, and what is deliberately absent.

## One row per call

The unit is a single request to the provider, not a task and not a job. An
evaluation that needed its repair retry is two rows (`evaluation` and
`evaluation_repair`); a cover letter is one. A failed or cancelled call is a row
too, because the request was sent and may have been billed.

A row records:

| Column | Meaning |
| :--- | :--- |
| `provider`, `model` | What the call was sent to, with the model after any override. |
| `task` | `evaluation`, `evaluation_repair`, `cover_letter`, and later `triage`. Free text, so a newer build's task does not need a migration. |
| `application_id`, `job_description_id` | The posting and application it concerned. Plain references, no foreign key. |
| `input_tokens`, `output_tokens`, `total_tokens` | What the provider reported, or an estimate. `NULL` when neither exists. |
| `cached_input_tokens`, `reasoning_tokens` | The parts of the counters the provider broke out. DeepSeek reports neither today. |
| `source` | `reported` or `estimated`. |
| `cost_usd`, `cost_complete` | A dollar figure and whether it is the whole charge or a lower bound. `NULL` when no price is known. |
| `outcome`, `error_kind` | `ok`, `cancelled`, or `error`, with the `LlmError` kind for the last two. |
| `duration_ms`, `created_at` | Wall time from request to reply or failure, and when the call started. |

## Reported or estimated

`rig` 0.46 reports usage on both the unary and the streamed path; DeepSeek's
dialect asks the stream for it, so the normal path is `reported`. The counters
are stored verbatim.

When a provider sends no usage block — a different provider, a transport failure,
or a cancelled stream — the counters are a four-characters-per-token estimate
and the row says `estimated`. A failed call estimates only its input: there was
no reply to estimate the output from, so `output_tokens` stays `NULL` rather
than zero.

The Usage screen always names the split, so a total is never presented as more
precise than it is.

## What the cost means

The cost is a **list-price estimate**, not an invoice. It comes from one of two
places, in this order:

1. The provider's own figure, when it sends one (`rig`'s `Usage.cost`; some
   gateways do, DeepSeek does not).
2. `rig`'s built-in model catalog, applied to the reported counters. For
   `deepseek-flash` that is 0.15 per million input tokens and 0.60 per million
   output tokens, standard tier.

If neither is available, `cost_usd` is `NULL`. The screen draws an em dash, never
`$0.00`, and the header says how many calls have no price. A price that covers
only part of a call (for example, cache tokens with no cache rate) is stored with
`cost_complete = 0` and drawn as a lower bound (`≥`).

Two consequences worth knowing:

- **`deepseek-chat` is not in the catalog**, so a stored configuration that still
  selects it runs fine but its rows are unpriced. That is why the default is
  `deepseek-flash`.
- A model renamed or added after this build ships is unpriced until `rig`'s
  catalog learns it. The counters are still recorded.

## What is not stored

- **No prompt text and no reply text.** The ledger is counters and metadata, the
  same rule as `docs/logging.md`.
- **No API key.** Keys live in the OS keychain only (`docs/logging.md`).
- **No billing reconciliation.** The provider's balance endpoint is not queried;
  `Test connection` asks for the balance but records no usage, because it spends
  no tokens.

## Lifetime and deletion

The ledger is not a child of a job. `application_id` and `job_description_id`
are references without a foreign key, so deleting a posting removes its
application, report, and letters but **keeps** the record of what it cost. The
row's job column then shows an em dash.

Usage rows are never edited and never pruned. Pruning and export are follow-ups
(Q-47), not part of this phase.

## Reading it

The **Usage** screen (sidebar, between Settings and Logs) shows:

- totals by provider and model, and by task, for **7 days**, **30 days**, or
  **All** (default 30);
- the last 50 calls with their job, tokens, source, cost, and outcome.

The screen is read-only and does not refresh itself; **Reload** reads the ledger
again. The estimates are cheap to recompute and the screen is visited rarely, so
a live-updating dashboard was deliberately not built.

## Verifying the live path

The ignored DeepSeek test (§`docs/LIVE_TESTS.md`) runs a real evaluation and
letter against a throwaway vault. If the Usage screen shows `estimated` for one
of those calls, the provider stopped reporting usage (or the stream stopped
asking for it) — record that rather than changing the ledger.
