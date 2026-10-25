# UI Framework Spike: Results and Recommendation

**Phase:** 2.01
**Parent plan:** `docs/phase-2.01-ui-framework-evaluation.md`
**Status:** Complete
**Date:** 2026-10-24
**Prepared by:** Implementation session

---

## Purpose

This document records the outcome of Phase 2.01, a one-week evaluation of four Rust UI frameworks against the criteria in the spike plan: testability and performance as primary, developer experience and API stability as secondary.

The trigger for the spike was friction with GPUI during the aborted start of Phase 2.1. The question it set out to answer was not "is GPUI bad" but "given five more phases of UI work, is GPUI the right foundation, or is one of the alternatives materially better on the criteria that matter?"

The answer this document reaches is **egui**, with **gpui-kit** recorded as the alternative worth revisiting if it reaches crates.io and its testing guide is completed.

---

## Methodology

Four prototypes of the same **Unlock Screen** were built, one per framework. Each prototype had the same four elements:

1. A passphrase text input
2. An "Unlock" button that sets an error on a wrong passphrase and clears it on the right one
3. A "Switch to Recovery" button that sets a flag on the application state
4. An error display area

Each prototype was tested headlessly, with the same assertions:

- Type a wrong passphrase → the error is set on the app state and rendered in the accessibility tree
- Type the correct passphrase → the error is cleared
- Both buttons are reachable and clickable

The prototypes were built sequentially, with the compile and runtime errors encountered at each step recorded.

### What was measured, and what was not

The plan called for four measurements per framework: idle CPU, binary size, prototype lines of code, and compile rounds.

**Idle CPU and binary size were not measured.** The prototypes were evaluated on their testing capability, their API ergonomics, and their ecosystem signals. That gap is material and is called out again in "Limits of this evaluation." The performance ranking below is based on published figures and on the rendering models each framework advertises, not on spike measurements.

**Two ecosystem datasets were collected after the prototypes were built:** crates.io download counts and GitHub repository activity. Both are recorded in the next section, and both changed the conclusion relative to the prototype evidence alone.

---

## Adoption and maintenance

Download counts measure use; repository activity measures the maintainers' capacity to respond. Neither is sufficient alone, and they disagree in instructive ways.

### crates.io downloads (rolling 90-day window)

| Framework | Recent downloads | Ratio to egui | Last publish |
| :--- | ---: | ---: | :--- |
| **egui** | 7,726,163 | 1× | **9 days ago** |
| **iced** | 696,992 | 11× fewer | 11 months ago |
| **gpui** | 222,412 | 35× fewer | ~1 year ago |
| **fenestra** | 288 | ~27,000× fewer | 2 months ago |

Two caveats on these numbers: downloads include CI, mirrors, and transitive resolution rather than human adoption; and the gpui figure describes a year-old 0.2.2 snapshot on crates.io, not the git commit the project actually depended on. Git fetches do not count as crates.io downloads, so gpui's real usage is invisible here.

### GitHub repository activity

| Framework | Stars | Forks | Contributors | Last commit | Notable signal |
| :--- | ---: | ---: | ---: | :--- | :--- |
| **egui** | ~31.6k | ~2.2k | — | 2026-10-19 | Maintainers reply within ~1 day; health score 87/100 |
| **iced** | ~31.7k | ~1.7k | **321** | 5–7 days ago | **493 open issues**; 0.14.0 released 2025-12-07 |
| **gpui-kit** | **14.9k–17.6k** | ~1,074 | — | 2026-10-22 | **Powers Longbridge Pro commercially**; +1,175 stars in one week |
| **fenestra** | **5–13** | — | **1** | — | 152 downloads/month on core; 78/month on shell |
| *(Zed, for gpui)* | *~92k* | *~11k* | *—* | *2026-10-20* | *3,510 commits in 26 weeks — but this is editor work, not library work* |

### What the two datasets together say

**egui is the only framework that is strong on both axes.** Large download base, active repository, responsive maintainers, and a release within the last two weeks. If a bug is hit in Phase 3, someone else has probably already reported it and someone is fixing it.

**iced is used but slow.** 31.7k stars and 321 contributors is real adoption, but 493 open issues and no release in 11 months suggests maintainer bandwidth is the constraint. This materially weakens the spike's earlier characterisation of iced as "the most mature alternative" — it is mature in *capability* and slow in *delivery*.

**gpui-kit is far stronger than the spike assumed.** The prototype doc described it as "young, git dependency," implying a hobby project. It is not. It has 14.9k–17.6k stars, over 1,000 forks, and it was extracted from a shipped commercial desktop application (Longbridge Pro) rather than designed in isolation. That is a materially different adoption profile, and it means the two documentation gaps the spike hit (`gpui_kit::init` not in the guide, no external window helper) are gaps in the guide, not signs of an unmaintained project.

**fenestra is a solo project.** 5–13 stars, one author, 152 downloads per month. The testing story is genuinely the best of the four, but a framework with a dozen stars cannot support a five-phase dependency. This is the finding that most changes the document: fenestra was previously recorded as the runner-up, and it should not be.

---

## Per-framework findings

### egui 0.31

**Testing capability: strong, with one accessibility gap.**

egui's `egui_kittest` harness is built on AccessKit, the same accessibility layer the other frameworks use, and its queries are semantic by default: `get_by_label`, `get_by_role`, `get_by_name`. The `Harness::builder().build_ui_state(|ui, app: &mut App| ..., app)` constructor hands the application state to the harness rather than capturing it in a closure, which means the test can read `harness.state()` after any number of `run()` calls without a borrow conflict. That is a small design choice with a large ergonomic effect: state assertions are one line, not a workaround.

The one real gap is **accessibility of text inputs**. `egui::TextEdit::singleline(...).hint_text("Passphrase")` renders the hint text visually, but does **not** expose it as an accessible label. The accessibility tree in the spike shows:

```
Node { role: TextInput, value: "", focused: false, ... }
```

with no `label` field. A screen-reader user hears "text input, blank" with no indication that the field is for a passphrase, and a test cannot query the input by label — it has to query by `Role::TextInput`, which is fine for one input but will not scale to a form with several.

That gap is a Phase 5 concern, not a Phase 2.1 one, but it is worth recording now because Phase 5's plan explicitly includes an accessibility audit.

The other finding is the familiar heading collision. `ui.heading("Unlock")` produces a Label node whose *value* is `"Unlock"`, and `get_by_label("Unlock")` matches both that label and the button's label. Renaming the heading was the fix, as in iced.

`Role` is an AccessKit type, re-exported as `egui::accesskit::Role`, not `egui_kittest::kittest::Role`. `egui_kittest` re-exports the query traits, not the AccessKit enums.

**Ecosystem:** 7.7M downloads/90d, released 9 days ago, ~31.6k stars, maintainers respond within a day, health score 87/100.

**Rounds to a working test: 2.**

### gpui-kit (git, pre-0.3)

**Testing capability: strongest runtime API, highest setup cost, and better adoption than expected.**

gpui-kit's testing layer closes the gap that raw GPUI has: base GPUI offers coordinate-based clicks (`simulate_click(point(px(x), px(y))))` with no semantic query, while gpui-kit adds `window.find("id")`, `window.find_all_by_role(Role::Button)`, `window.find_by_label("...")`, `window.click("id", cx)`, `window.input(text, cx)`, and element assertions (`.role()`, `.label()`, `.value()`, `.focused()`, `.bounds()`). It also supports `window.within("scope")` to restrict a query to a subtree, which matters for the dialogs Phase 2.1 will add.

The distinctive advantage is that **no test-only ids had to be added to the prototype**. The `id("passphrase")`, `id("unlock")`, `id("recovery")`, and `id("error-status")` are used by the production accessibility tree — which is why `window.find` can locate them. That is the opposite of the iced and egui prototypes, where a heading had to be removed or renamed to make label queries work.

The distinctive cost is **setup complexity**. Getting the prototype from "compiles" to "tests pass" took four rounds:

1. `test_support()` needs `TestSupportExt` in scope (one import)
2. `Root` is at `gpui_kit::component::Root`, not `gpui_kit::Root`
3. The window-opening helper is not documented for external test code, and `TestAppContext::update` is `pub(crate)` in the pinned version — the working pattern is `cx.open_window(...)` with `Root::new(...)`, which required reading the framework's own test suite
4. `gpui_kit::init` must be called at the start of each test to register the `Theme` global; the testing guide's minimal example does not mention this, and the failure is a runtime panic

Two of those four were documentation gaps, one was a naming move, one was a missing import.

**Ecosystem correction.** The prototype write-up described gpui-kit as "young, git dependency." That undersold it. It has 14.9k–17.6k stars, 1,074 forks, and was extracted from a commercially shipped desktop application rather than designed in isolation. The two documentation gaps are gaps in the *guide*, not evidence of an unmaintained project. The framework is also **not on crates.io** — it is a git dependency pinned to a commit, which itself depends on a pinned GPUI commit. That is two levels of pin, and it remains the real cost.

**Rounds to a working test: 4 (3 compile, 1 runtime).**

### iced 0.14

**Testing capability: adequate, with one production-code cost.**

Iced's `iced_test` crate provides a `Simulator` with `click`, `typewrite`, `find`, and `into_messages`. The model is to drive the UI, drain the emitted messages, apply them to the application state, and assert — which is explicit and easy to follow. Headless mode testing was added in 0.14.0.

The one capability gap relative to egui is the query model. `Simulator::click` takes a selector, and a `&str` selector matches on **contained text**. That is simple, but it means a heading `text("Unlock")` and a button `button("Unlock")` are indistinguishable to the query. The spike hit this:

```
ui.click("Unlock")  // ambiguous: matches both the heading and the button
```

The fix was to **remove the heading from the production view**. That is the one case in the spike where a test forced a change to production code, not to test scaffolding.

**Ecosystem correction.** 31.7k stars and 321 contributors is real adoption. But **493 open issues** and no release since 0.14.0 on 2025-12-07 suggest maintainer bandwidth is the constraint. The spike previously called iced "the most mature alternative"; it is mature in capability and slow in delivery, and both halves of that matter for a five-phase dependency.

`iced_test` versions track `iced` rather than having their own line, so `iced_test = "0.1"` did not resolve; the correct form is `iced_test = "0.14"` alongside `iced = "0.14"`.

**Rounds to a working test: 2.**

### fenestra 0.41

**Testing capability: strongest of the four. Adoption: disqualifying.**

Fenestra's harness (`fenestra::shell::Harness`) is purpose-built for headless testing in a way the others are not:

- **Three assertion levels.** The test can assert on `h.app()` (application state), on `h.query(&by::...)` (accessibility tree), and on `h.render()` (pixels). The other three frameworks support the first two; only fenestra's harness captures pixels without a separate renderer.
- **Explicit determinism.** The harness fixes scale at 1.0, enables reduced motion, embeds fonts, and gives the app an explicit clock, so animations only advance on `h.pump()`. That is why the `renders_to_pixels_without_a_display_server` test passed on the first try, while no other prototype even attempted a snapshot.
- **Documented query priority.** The framework's testing guide states the preferred order — role + name, then label, then value, then id — and enforces it: a query that matches zero or several nodes panics rather than silently picking one.

**Adoption correction.** The prototype write-up called fenestra "the runner-up." That was wrong. The repository has **5–13 stars**, a single author, and 152 downloads per month on `fenestra-core` (78 on `fenestra-shell`). It is at **0.41** with approximately **38 breaking releases** between 0.39 and 0.41. A framework that almost nobody uses, churning rapidly, is the worst possible profile for a five-phase dependency: there is no external bug-finding, no third-party migration guides, and no reasonable expectation that the API will settle.

The one structural finding stands: a fenestra button carries the same accessible name as its own text child, so `by::label("Unlock")` matches two nodes and `by::role(Semantics::Button).name("Unlock")` is required. The `by::id` escape hatch also did not work on buttons in this version, while it did on `text_input`. That is a real design property, but it is not the reason fenestra was rejected — adoption is.

**Rounds to a working test: 4 (3 were author errors, not framework errors).**

---

## Comparison matrix

| Criterion | Fenestra 0.41 | iced 0.14 | egui 0.31 | gpui-kit pre-0.3 |
| :--- | :--- | :--- | :--- | :--- |
| **Headless automation** | Yes — `Harness::new` | Yes — `simulator` | Yes — `Harness` | Yes — `#[gpui_kit::test]` |
| **Semantic queries** | `by::role().name()`, `by::label`, `by::id` | text selectors | `get_by_label`, `get_by_role` | `find`, `find_all_by_role`, `find_by_label` |
| **Click by id/name** | Yes | Yes | Yes | Yes |
| **Assert on app state** | `h.app()` | `Simulator` + state | `harness.state()` | entity state via `.read(cx)` |
| **Snapshot testing** | **Built in** — `h.render()` → PNG | feature flag | feature flag | separate renderer |
| **Determinism** | **Documented** — explicit clock, embedded fonts | not documented | not documented | not documented |
| **Production changes forced by tests** | Button ids added (no effect, removed) | **Heading removed** | Heading renamed | **None** |
| **Accessibility of text input** | no accessible name | not investigated | no accessible label | **explicit `.aria_label()`** |
| **Test API discoverability** | Documented priority order | Simple; selector model documented | Harness methods documented | Spread across 4 traits; guide incomplete |
| **Rounds to a working test** | 4 (3 author errors) | 2 | 2 | 4 (2 docs gaps) |
| **crates.io recent downloads** | **288** | 696,992 | **7,726,163** | git only |
| **GitHub stars** | **5–13** | 31.7k | 31.6k | **14.9k–17.6k** |
| **Contributors** | **1** | **321** | — | — |
| **Last release** | 2 months ago | **11 months ago** | **9 days ago** | git only |
| **Commercial production use** | none | yes | yes | **yes (Longbridge Pro)** |
| **Idle CPU (not measured)** | documented near-zero | reactive; low by design | near-zero by design | low by design |
| **Binary size (not measured)** | unknown | ~11 MB | ~5 MB | ~12 MB |

---

## Recommendation

**egui.**

The reasoning, weighted by the criteria the spike was written against and by the ecosystem data collected after the prototypes:

### Why egui wins

**Testability.** egui is the only framework that combined semantic queries, a purpose-built harness, and no required production-code change, without also requiring a git dependency. iced matches the "mature harness" bar but fails on semantic queries and needed a production change to pass the test. gpui-kit has the best runtime query API but the highest setup cost and the least stable packaging. fenestra has the strongest harness on paper and the weakest adoption of any framework evaluated.

**Maintenance capacity.** egui is the only framework of the four with all three of: a release in the last two weeks, a maintainer response time measured in days, and a download base large enough that most bugs are found by someone else first. In a five-phase project, that combination is worth more than any single capability advantage another framework offers.

**Performance, directionally.** egui is immediate-mode, which costs more per frame than a retained or reactive model on a static screen. But it repaints only on interaction or animation, so the *idle* CPU number — the one the spike plan asked for — is expected to be near zero, and the framework's memory footprint is the smallest of the four. For a data-dense desktop application with a dozen screens, the difference between immediate and retained rendering is real but not the dominant cost.

### The trade-off this choice accepts

**The accessibility gap.** `hint_text` not being exposed as an accessible label is a real problem for Phase 5. It is workable — a labelled wrapper around the input, or a `labelled_by` association if egui's AccessKit output supports it — but it is work that a framework with a first-class `.aria_label()` (gpui-kit) does not require. The recommendation accepts this as the cost of choosing a framework that is otherwise the right fit.

**Immediate-mode CPU on complex screens.** The Pipeline screen will be a virtualized list with filter tabs and, in Phase 3, per-row status. If the list grows to thousands of rows and egui's immediate-mode overhead becomes visible, the mitigation is `egui_extras`' virtualized rows, which do exactly what the plan needs. That is a known path, not an open question.

### Why not the alternatives

**gpui-kit** is now the strongest runner-up, and the prototype write-up underrated it. It has 14.9k–17.6k stars, over 1,000 forks, and it was extracted from a commercially shipped desktop application — which is a stronger adoption signal than iced's 31.7k stars with a year between releases. Its runtime query API is the best of the four, and it is the only prototype that needed no test-only ids. What it loses on is packaging: a git dependency pinned to a commit that is itself pinned to a GPUI commit, with a testing guide that does not cover the init step or the external window helper. **If gpui-kit reaches crates.io and completes its testing documentation, it becomes a genuine candidate and this ranking should be revisited.** That is a concrete, checkable condition, not a vague "keep an eye on it."

**iced** is the most conventional alternative and would be a defensible choice. It loses to egui on semantic queries, on the production-code change the spike required, and now on release cadence — 11 months between 0.14.0 and the present, with 493 open issues. It is mature in capability and slow in delivery.

**fenestra** is not a foundation. The testing story is the best of the four and would be worth borrowing from, but 5–13 stars, one author, and 38 breaking releases make it unsuitable for a dependency that five phases will rest on. It is recorded here so the next person considering it knows both halves: excellent harness, insufficient adoption.

---

## Limits of this evaluation

Three things bound what the recommendation is claiming.

**No performance measurements were taken.** The performance ranking is based on rendering models and published figures, not on idle CPU or binary size from the spike. The plan called for those measurements and they were skipped. If performance turns out to be decisive — which it may, since the main plan weights it highly — they should be taken before Phase 2.1 commits to egui. The four prototypes still compile; measuring them is 20 minutes of work.

**Only the Unlock Screen was prototyped.** That screen exercises a form, two buttons, and a conditional error display. It does not exercise a virtualized list, a modal dialog, or a table with column resizing — all of which Phase 2.1 needs and Phase 3 will add. The frameworks may differ on those, and the spike's evidence does not cover them.

**Ecosystem data is a snapshot.** Download counts are a rolling 90-day window; star counts and issue counts move. The comparisons are valid as of 2026-10-24. The two findings most likely to change are gpui-kit's crates.io status and iced's release cadence, and both are noted above as revisitable conditions rather than settled facts.

---

## What changes

If this recommendation is accepted:

1. **The Phase 2.1 plan is rewritten against egui.** The view-model layer, the navigation state, and the async bridge from the aborted Phase 2.1 plan are framework-independent and survive unchanged. The view layer and the test patterns are replaced.

2. **The main plan's §12 is updated** to reflect egui as the UI framework, replacing GPUI. The GPUI pin, the toolchain requirement that came from it, and the `gpui-tokio-bridge` reference are removed.

3. **The CI is updated.** The egui/eframe dependencies do not require `std::hint::cold_path`, so the Rust 1.95.0 requirement that came from the GPUI pin may be relaxed — verify against the actual toolchain before changing the minimum.

4. **The `crates/app` crate is amended.** `Cargo.toml` replaces `gpui` with `egui`/`eframe`. `src/main.rs` and `src/screens.rs` (both placeholders) are rewritten. `src/state.rs`, `src/navigation.rs`, `src/async_bridge.rs`, and `src/view_models/` survive.

5. **The abandoned GPUI code is deleted.** The four spikes remain, archived, in `spikes/`. The GPUI prototype is the reference for "this was evaluated and not chosen."

---

## Appendix: what each prototype proved

| Prototype | Passing tests | The test that mattered most |
| :--- | :--- | :--- |
| **Fenestra** | 4/4 | `renders_to_pixels_without_a_display_server` — first and only headless PNG render in the spike |
| **Iced** | 1/1 | `unlock_screen_headless` — required removing a heading from production code to pass |
| **egui** | 3/3 | `unlock_screen_wrong_passphrase_sets_error` — state assertion via `harness.state()` and rendered assertion via `get_by_label` |
| **gpui-kit** | 3/3 | `recovery_button_is_present_and_clickable` — `find_all_by_role(Role::Button)` returned exactly two nodes without any test-only ids |

All four prototypes compile and their tests pass. The recommendation is not about capability — every framework here can drive the Unlock Screen headlessly. It is about which capability the project can afford to depend on for the next five phases, and which of the four has the maintainer capacity to answer a bug report in month four.