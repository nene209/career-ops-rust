# Privacy: what leaves the machine, and what is redacted

Two different questions, and the app answers them separately:

1. **Where does the data go?** An evaluation sends your CV summary and the
   selected posting to the provider you chose. DeepSeek is hosted in the PRC; a
   **Local / custom** endpoint keeps everything on your network. That choice is
   in Settings → **AI evaluation**, and nothing is sent until you run a call.
2. **What does the provider see of you?** With redaction on (the default), the
   CV summary is rewritten locally: your name, email, phone, and a street-address
   location become placeholders. The reply is restored here, so what you read and
   what is stored carry the real values.

This file covers the second. It is best-effort by nature; the strong control is
the first, and the app says so in Settings too.

---

## What is redacted

The CV summary is the only thing redacted, and only for the two calls that carry
it (evaluation and cover letter). The identity values come from the profile the
CV renderer itself writes, so the match is against strings the app produced.

| Source | Match | Placeholder |
| :--- | :--- | :--- |
| Full name | word-bounded; case-insensitive for ASCII, exact otherwise | `⟦NAME⟧` |
| Each name part (3+ characters, not a common word) | word-bounded, case-insensitive for ASCII | `⟦NAME_1⟧`, `⟦NAME_2⟧`, … |
| Email | case-insensitive, anywhere in the text | `⟦EMAIL⟧` |
| Phone | the stored string, plus any run whose **digits** match (7+ digits) | `⟦PHONE⟧` |
| Location | only when it looks like a street address | `⟦LOCATION⟧` |

Name parts get their own indexed tokens so a reply can talk about one part
("⟦NAME_1⟧ wrote…") and still restore the right word.

### What counts as a street address

`address_like` is true when the location starts with a digit (`123 Main St`), or
contains a digit and a street suffix (`Flat 4, Elm Road`). Suffixes: street, st,
avenue, ave, road, rd, boulevard, blvd, lane, ln, drive, dr, court, ct, way,
place, pl, terrace, ter, circle, cir, highway, hwy, parkway, pkwy, square, sq.

A city or region (`Berlin, Germany`, `Remote`, `Springfield, IL`) passes through
unchanged: it is context the model can use, not an identifier.

## What is **not** redacted

- **Job postings.** A posting may name a recruiter, contain an email, or quote a
  phone number. That is not your personal data, and stripping it would damage the
  evaluation. Only the CV summary is rewritten.
- **Triage and portal suggestions.** Neither prompt includes the CV.
- **Your stored data.** The vault, the report, the letter, and every screen keep
  the real values. Redaction happens at the boundary of a provider call.
- **Nicknames and variants.** `Ada` is caught only if it is part of the stored
  name. "A. Lovelace", a name split across two lines, or a middle name the
  profile does not carry will not be.
- **Two-character values.** Every replacement is floored at three characters, so
  a name like "Li" is left alone rather than matching ordinary prose. The preview
  makes the miss visible.
- **Names that are also everyday words.** May, Will, Mark, Rose, Grace, Hope,
  King, and Young are not redacted on their own (the full name still is). The
  tradeoff is a surname that is also a common word; the preview shows it.
- **Non-ASCII case variants.** A stored `José Álvarez` matches exactly; `JOSÉ
  ÁLVAREZ` does not. ASCII-only folding is deliberate: locale-dependent
  lowercasing would change byte offsets.

## Restoration

- **Evaluation.** The score and recommendation are a number and an enum. The
  four prose fields — `strengths`, `gaps`, `rationale`, `full_report_markdown` —
  are restored **after** the JSON is parsed. Restoring first could put a quote or
  a newline inside a JSON string and break the whole reply.
- **Cover letter.** Free text, restored before it is stored, so the signature
  carries your real name.
- **A token the summary never emitted is not restored.** If the body never
  contained the bare first name, `⟦NAME_1⟧` does not exist, and a reply that
  writes it anyway is stored as it arrived. The `redaction` log line counts it as
  `unrestored` rather than guessing.
- **Streaming shows placeholders.** The live stream is a preview; the final
  assembled text, which is what gets stored and displayed afterwards, is
  restored. Incremental restoration would need a token-boundary buffer, and it is
  not worth the complexity for a few seconds of preview.

## The toggle and the preview

Settings → **Privacy**:

- **Redact personal details before sending to the model** — on by default. It is
  a stored setting (`privacy.redact_cv`); a missing or malformed value reads as
  on.
- **Removes: name, email, phone, street-address location.** — the one-line
  summary.
- **Preview what the model sees** — a window with the CV summary exactly as the
  next call would send it, plus a count of replacements. It follows the checkbox
  as it is on screen, saved or not, so you can compare before committing. With
  redaction off it shows the raw summary and says so.

The profile field is labelled **Location (city / region)**, with a warning when
the value looks like a street address. There is deliberately no separate address
field: one would encourage storing it and would have to be rendered on the CV
and in the PDF.

## Logging

One `info` line per redacted call:

```text
redaction before the evaluation call redacted=5 names=3 emails=1 phones=1 locations=0 restored=5 unrestored=0
```

Counts only. The values, the redacted summary, and the prompt are never written;
`docs/logging.md` states the rule the whole log follows.

## Tests

- `crates/core/src/privacy.rs` — 18 unit tests over matching, the stoplist, the
  address heuristic, restoration, and the counts.
- `crates/core/tests/privacy.rs` — the stored toggle (default, malformed, round
  trip) and `redact_cv` against a real profile.
- `crates/core/tests/evaluation.rs` — the prompt the backend receives carries no
  name, email, phone, or address but does carry the tokens; the stored report is
  restored; the toggle off sends the raw summary; a mangled token is stored as it
  arrived.
- `crates/core/tests/cover_letter.rs` — the same for the letter, including the
  restored signature.
- `crates/app/tests/settings_screen.rs` and `crates/app/tests/cv_profile_screen.rs`
  — the toggle, the preview window (both modes, the counts, an error), and the
  location hint/warning.

## Residual risk

Pattern matching is not a privacy guarantee. A nickname, a reflowed address, or a
number written in an unusual format can survive, and the model still learns
plenty that identifies you (employers, dates, projects). If that matters for a
particular call, use a **Local / custom** endpoint: there, no data leaves your
network regardless of what the redactor caught.
