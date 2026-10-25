# Career-Ops Rust Port — Agent Handoff

**Purpose:** This file gives a fresh agent enough context to continue work on the Career-Ops Rust port without re-reading an 80-message conversation. It is a **companion** to the repo's documentation, not a replacement for it. Where a fact lives in a `docs/` file, this handoff points at it rather than restating it.

**Last updated:** 2026-10-24
**Last human decision recorded:** egui/eframe adopted as the UI framework; GPUI rejected after Phase 2.01.

---

## 1. Project snapshot

**What:** A port of the Node.js `career-ops` tool (job search pipeline manager) from file-based CLI to a Rust desktop application with SQLite state, encryption at rest, and a GUI.

**Where:** `github.com/nene209/career-ops-rust` (local working copy: `/opt/app/src` or `/home/work/github.com/nene209/career-ops-rust`, depending on machine).

**Current phase:** Between Phase 2 (complete) and Phase 2.1 (not started). The binary compiles and runs but is a state-bootstrap placeholder — no UI yet.

**The user is:** an experienced developer learning Rust. They copy-paste code into files, report compiler errors verbatim, and expect explanations of *why*, not just *what*. They have caught several mistakes in prior sessions and value honest corrections over confident-sounding prose.

---

## 2. Architecture decisions (summary + rationale)

These are settled. Do not relitigate them without a specific reason.

| Decision | Value | Why |
| :--- | :--- | :--- |
| UI framework | **egui 0.37 / eframe 0.37** | Chosen in Phase 2.01. GPUI rejected (pre-1.0, coupled test harness, no semantic query in base crate). Details in `docs/ui-framework-spike.md`. |
| Async test harness | `egui_kittest` 0.37 | Semantic queries, `harness.state()` access, snapshot support. |
| Database | SQLite via SQLCipher + `sqlx` 0.8 | Async-first, compile-time query checks, integrated migrations. |
| Browser automation | Obscura sidecar, driven via `chromiumoxide` | Smaller than bundled Chrome, CDP-compatible. `ObscuraBrowserBackend` exists and works. |
| PDF rendering | `rust-headless-chrome` in Phase 4 | Obscura can render PDFs (verified) but fidelity is unverified; keep as fallback. |
| LLM integration | `rig` (Phase 3) | Provider-agnostic client, structured output via `Extractor`. |
| Encryption | Mandatory at rest; envelope encryption (DEK wrapped by keychain KEK + passphrase KEK) | No unencrypted mode. `dek.wrapped.json` sibling file. |
| Human-in-the-loop | `applied` state reachable only via `mark_applied`, requiring `ready` | Enforced by repository layer + test. |
| Toolchain | Rust 1.95.0+ (project preference; no longer load-bearing) | GPUI required it; GPUI gone, but higher floor retained as preference. |

---

## 3. Current state

### What's done

- **Phase 1 (Foundation):** Keystore, encrypted DB, all repositories, backend traits, prompt seeding, state machine, test harness. All exit criteria complete. See `docs/phase-1-retrospective.md`.
- **Phase 2 (Acquisition):** Four ATS adapters (Greenhouse, Lever, Ashby, SmartRecruiters), HTML extraction, tier routing, `ObscuraBrowserBackend`, failure persistence, Portals service layer, manual entry. All exit criteria complete, with one documented partial (real-page HTML extraction). See `docs/phase-2-retrospective.md`.
- **Phase 2.01 (UI Framework Spike):** Four prototypes built and tested. egui selected. See `docs/ui-framework-spike.md`. Prototypes archived in `spikes/`.
- **CI:** Green. Six jobs including keychain-failure-injection, portals-service, browser-fallback. Live ATS tests run nightly.

### What's in progress

- **Nothing.** Phase 2.1 has not started. `main.rs` is a placeholder that prints the state and exits.

### What's not started

- Phase 2.1 (UI shell) — plan is ready in `docs/phase-2.1-ui-shell.md` (v0.3)
- Phases 3–7

### Files that exist and matter

```
crates/
  core/                    # All business logic; no UI dependency
    src/keystore/          # Envelope encryption, platform keychain, Argon2id
    src/db/                # SQLCipher + sqlx
    src/repo/              # One module per table
    src/backend/           # BrowserBackend trait + Obscura + Chromiumoxide impls
    src/backend.rs         # Trait definitions + MockBrowserBackend
    src/acquisition/       # ats/, html/, router.rs, manual.rs, http.rs, error.rs
    src/portals/           # health.rs, service.rs
    src/prompts_seed.rs    # Embedded prompts; update-aware seeding
    migrations/            # 0001_initial.sql, 0002_portal_fetch_status.sql
    prompts/               # modes/*.md copied from original repo
    tests/                 # Integration tests

  app/
    src/state.rs           # AppState state machine (FirstLaunch/Unlocking/Recovery/Ready)
    src/navigation.rs      # MainScreen + NavigationState
    src/async_bridge.rs    # AsyncTask<T> for tokio → eframe hand-off
    src/view_models/       # Framework-independent (empty until Phase 2.1 WS3)
    src/screens/           # egui views (empty until Phase 2.1)
    src/lib.rs             # pub mod async_bridge, navigation, screens, state, view_models
    src/main.rs            # PLACEHOLDER: state bootstrap, prints, exits

  test-support/            # Persistent in-memory keychain backend with failure injection

spikes/                    # Archived prototypes (fenestra/, iced/, egui/, gpui/)
                           # Excluded from workspace via [workspace.exclude] in root Cargo.toml

docs/
  phase-1-retrospective.md
  phase-2-retrospective.md
  phase-2.1-ui-shell.md
  phase-2.01-ui-framework-evaluation.md
  ui-framework-spike.md
```

---

## 4. Working conventions

How the previous session operated, and what the user expects:

1. **Complete files, not diffs.** When delivering code, provide the whole file with a clear "replace this file entirely" framing, unless the change is genuinely one line. The user copy-pastes.
2. **Two-step deliveries for complex work.** For multi-file deliveries, split into "Step 1 (should compile first try)" and "Step 2 (needs iteration)". The user verifies step 1 before proceeding.
3. **Explain the why.** Rust idiom explanations are welcome. The user is learning.
4. **Report errors with full context.** The user pastes compiler output verbatim. Read it carefully — the answer is usually in the error message.
5. **Do not hallucinate.** When something is not verified (a crate API, a version number, a file path), say so. The previous session had several cases where an unverified claim (e.g. `iced_test = "0.1"`, `egui_kittest::kittest::Role`, gpui-kit's `TestSupportExt` scope) wasted a compile round. Honest uncertainty is cheap; confident wrongness is expensive.
6. **Verification passes are non-negotiable.** At the end of each phase, walk every exit criterion and confirm the test exercises the property. This caught two significant bugs in Phase 1 and the missing `ObscuraBrowserBackend` in Phase 2.
7. **Plans are living documents.** Update with a version bump and changelog entry when decisions change. Do not silently edit.
8. **Spike code is throwaway.** Do not productionise it. Delete or archive when the spike concludes.
9. **Commit messages follow conventional commits.** Subject line names the primary deliverable, not the process. Body explains *why* it was missing/broken, then lists changes.

---

## 5. Immediate work: Phase 2.1 (UI Shell)

The plan is `docs/phase-2.1-ui-shell.md` v0.3. Read it before doing anything. Summary of the sequence:

**Workstream 1 (2–3 days):**
- Replace `main.rs` with `eframe::run_native` entry point holding `AppState`, `NavigationState`, and a `tokio::runtime::Runtime`
- Panel layout: `SidePanel::left` for navigation, `CentralPanel` for content
- Async bridge pattern: `AsyncTask::try_recv()` polled in `update()`; use `ctx.request_repaint_after(Duration::from_millis(50))` while a task is pending
- **`data_dir()` cleanup**: replace the `CAREER_OPS_DATA_DIR` env-var read with a `directories`-crate lookup. Add `directories = "5"` to `crates/core/Cargo.toml`. Without this, the shell cannot run on a clean machine.
- Shared components: `EmptyState`, `StatusBadge`, `LoadingSpinner`
- `labelled_input` helper (see gotcha below)
- `crates/app/docs/patterns.md` documenting list, form, async patterns

**Workstream 2 (2–3 days):** Unlock, first-launch wizard, recovery screen

**Workstream 3 (3–4 days):** Portals screen with health badges, CRUD, fetch-one, batch-fetch with progress, sidecar status, manual entry modal

**Workstream 4 (2 days):** Pipeline screen with filter, job detail placeholder, add-job button

**Workstream 5 (1 day):** Verification pass, retrospective

### Files to create/modify first

1. `crates/app/Cargo.toml` — replace GPUI deps with `egui = "0.37"`, `eframe = "0.37"`, dev-dep `egui_kittest = { version = "0.37", features = ["snapshot", "wgpu"] }`
2. `crates/app/src/main.rs` — replace the placeholder entirely
3. `crates/core/src/lib.rs` — update `data_dir()`
4. `crates/core/Cargo.toml` — add `directories = "5"`
5. `crates/app/src/screens/mod.rs` — grows into submodules as workstreams land

### What already exists and must not be touched

- `crates/app/src/state.rs` — the `AppState` state machine, tested, complete
- `crates/app/src/navigation.rs` — `MainScreen` and `NavigationState`, tested
- `crates/app/src/async_bridge.rs` — `AsyncTask<T>`, tested
- All of `crates/core/` — the service layer the shell consumes

---

## 6. Hard-won lessons (the gotchas)

Things that cost time in prior sessions. Read these before writing similar code.

### Rust / build

- **`keyring` 3.x has no `mock` feature.** The mock credential store is always compiled in. Use `keyring::mock::default_credential_builder()` directly. Platform backends (`apple-native`, `windows-native`, `linux-native`) are opt-in and must be in `Cargo.toml` or every read silently returns `NoEntry`.
- **SQLCipher + sqlx** requires `libsqlite3-sys = { features = ["bundled-sqlcipher-vendored-openssl"] }`. Migrations must run in `db::open`, not only in `db::create_and_migrate`, or a returning user never receives a new release's migration.
- **`sqlx::migrate!("./migrations")`** resolves its path relative to the crate that invokes it, not the workspace root.
- **Test binaries run with cwd = crate root**, not the shell's cwd. Use `env!("CARGO_MANIFEST_DIR")` for path resolution, or absolute paths.
- **On Windows, `$(pwd)` in Git Bash is a POSIX path** (`/d/a/...`) that `std::process::Command` cannot use. Convert with `cygpath -w`.
- **`thiserror` and `#[derive(Debug)]`** coexist on the same struct; `Error` only adds the trait impl. Adding `PartialEq` for test assertions is fine when the type contains only comparable fields.
- **Feature-gate at the module, not the item.** Wrap feature-gated code in `mod inner { ... }` and re-export the public surface, or `-D warnings` will flag every constant as `dead_code` in the disabled build.

### Testing

- **A test that passes for the wrong reason is worse than no test.** Phase 1's failure-injection test called `try_passphrase` on an already-`Ready` state machine; it passed without testing the fallback. Ask "what would this assertion fail on?" of every new test.
- **`-D warnings` does not enable `clippy::unwrap_used` or `clippy::expect_used`.** Those are restriction lints. Add `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]` at the module level to actually enforce a no-unwrap policy. Verify the lint fires by adding a deliberate unwrap.
- **Mock types that nothing constructs hide missing production code.** Every `Option<&dyn Trait>` parameter is a place where the mock can pass and the real implementation can be absent. Always have at least one test that constructs the real type, even if it is `#[ignore]` or environment-gated.
- **`egui_kittest`**: `Harness::builder().build_ui_state(|ui, app| app.ui(ui), app)` — the state is handed to the harness, not captured by the closure, so `harness.state()` works afterward. The `Role` type is `egui::accesskit::Role`, not `egui_kittest::kittest::Role`.
- **Iced's `Simulator`** matches selectors by contained text, so a heading and a button with the same text are ambiguous. The fix is to remove or rename the heading, not to add test ids.

### egui-specific

- **`ui.heading("Unlock")` and `ui.button("Unlock")` both match `get_by_label("Unlock")`.** Either rename the heading or use `get_by_role(Role::Button).name("Unlock")`.
- **`TextEdit::hint_text` is visual only.** It does not become an accessible label. Test queries must use `Role::TextInput`, and a `labelled_input` helper is needed for accessibility (WS1 deliverable 5).
- **egui does not call `update()` continuously.** Async results require `ctx.request_repaint_after(Duration::from_millis(N))` while a task is pending. Without it, the UI silently freezes.
- **`Harness::new_ui(|ui| app.ui(ui))`** borrows `app` mutably for the harness's lifetime — you cannot read `app` outside. Use `build_ui_state` and `harness.state()`.

### Browser automation

- **Obscura can render PDFs** with the `render` feature (verified: valid `%PDF` output). The Phase 4 plan assumed otherwise and has been corrected.
- **`Browser::connect` can fail with `Connection refused` even after `wait_for_port` succeeds** — the TCP listener is bound before the WebSocket handler accepts. Retry `Browser::connect` with a short delay.
- **`Browser.close().await` deadlocks after the child process is killed** because it awaits a CDP response from a dead socket. The correct shutdown is: abort the handler task, SIGTERM the child, `drop(browser)` without calling `close()`.
- **Chrome's `SingletonLock`** prevents two instances on the same profile. Give each `ChromiumConfig` a unique `user_data_dir` when instantiating more than one backend.

### Environment / CI

- **`actions/cache`** cache keys must include `runner.os` and `runner.arch` for platform-specific binaries. `Swatinem/rust-cache` handles Cargo artifacts; a separate `actions/cache` handles the Obscura binary at `vendor/`.
- **Cache saves should be gated to `main` and `dev`** via `save-if: ${{ github.ref == 'refs/heads/main' || github.ref == 'refs/heads/dev' }}` on the cache step.
- **CI on Windows requires `cygpath -w`** to convert Git Bash paths to native paths before exporting via `$GITHUB_ENV`.

---

## 7. Anti-patterns observed

Things the previous session had to correct. Avoid them.

1. **Fabricated references.** Several plan documents cited specific line numbers or crate APIs that did not exist. When writing about a crate's behaviour, prefer "check the current docs" over a specific claim.
2. **Confident answers about unverified APIs.** The `iced_test = "0.1"` version never existed; `iced_test` tracks `iced`'s version. When a version or method name is uncertain, say so and let the compiler decide.
3. **Skipping the verification pass.** Phase 1 had two defects (wrong-reason test, unenforced lint) that would have shipped without the verification pass. Phase 2 had the missing `ObscuraBrowserBackend`. Both would have caused cascading problems.
4. **Assuming a spike is a delivery.** A spike that proves technology works does not deliver the production artifact. When a plan says "implement X," verify that X exists before marking the criterion complete.
5. **Rewriting plans without a version bump.** Plans are living documents; they need a changelog entry when they change, or the record of what was decided and when is lost.
6. **Retroactive documentation.** Do not edit a closed retrospective to reflect later findings. Add a note pointing to the later phase instead.

---

## 8. Open questions

Not yet decided. Ask the user before proceeding on these.

- **Q-3:** Argon2id work factor — current parameters are RustCrypto Book minimums, may need tuning to hit 250–500ms on the target hardware. Deferred to Phase 5.
- **Q-4:** Does `rust-headless-chrome` render existing HTML/CSS templates acceptably? Or should Obscura's `printToPDF` be the Phase 4 backend? Deferred to Phase 4.
- **Q-5:** Prompt versioning semantics — current design stores `prompt_version` and `prompt_hash` on reports. Is that sufficient for re-running evaluations against prior prompt versions? Deferred to Phase 3.
- **Q-7:** Does egui 0.37 support `labelled_by` for text input accessibility? If not, the `labelled_input` helper's workaround persists to Phase 5.

---

## 9. Document map

When the user asks about something, point at the document that answers it:

| Question | Document |
| :--- | :--- |
| What is the overall architecture? | `docs/Draft Execution Plan Career-Ops Rust Desktop.txt` (main plan v0.6, after the v0.5→v0.6 updates) |
| Why is the DB encrypted the way it is? | Main plan §7; `crates/core/src/keystore/` |
| What does Phase 1 deliver? | `docs/phase-1-retrospective.md` |
| What does Phase 2 deliver? | `docs/phase-2-retrospective.md` |
| Why was GPUI abandoned? | `docs/ui-framework-spike.md` |
| What does Phase 2.1 need to build? | `docs/phase-2.1-ui-shell.md` v0.3 |
| How is the acquisition pipeline tested? | `crates/core/tests/` |
| What are the "safety rules that are not negotiable"? | README "Contributing" section |

---

## 10. If something goes wrong

If the user reports a compile error that does not match what this handoff or the plans describe:

1. **Read the error carefully.** Rust's error messages are usually correct and specific. The answer is often in the suggestion.
2. **Check the current state of the file on disk.** The plans describe intended state; the user's copy-paste may have introduced a drift.
3. **Do not guess at the fix.** Ask for the file contents if the error is not obviously explainable from the error message alone.
4. **Say what you are uncertain about.** If a crate API may have moved, say so and suggest the user check `cargo doc` or docs.rs before proceeding.
5. **Preserve the user's work.** If the fix requires rewriting a file, confirm what is there now before replacing it.

---

## Final note

The user has invested significant effort in this project's documentation. Do not rewrite it wholesale. When a fact changes, edit the specific section with a changelog entry. The value of this project is not just the code — it is that the reasoning behind every decision is recorded and traceable. Preserve that property.

If this handoff was useful, the user will continue in a fresh session. If it was not, they may return to the original conversation. Either way, do not treat this file as a substitute for careful reading of the actual code and plan documents — treat it as a map that tells you where to read.