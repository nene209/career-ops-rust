# Phase 1 Implementation Notes

**Status:** Scaffold. Not a finished Phase 1.
**Date:** 2026-10-17

## What this is

A complete, coherent scaffold of the Phase 1 foundation. The keystore,
database setup, models, backend traits, migrations, and safety tests are
real implementations intended to compile with minor iteration. The GPUI
views are placeholders.

## What is implemented

- **Keystore** — envelope encryption with DEK + keychain KEK + passphrase
  KEK, XChaCha20-Poly1305 wrapping, Argon2id derivation, atomic
  write-then-rename for `dek.wrapped.json`. The "never re-initialize" rule
  is enforced in `Keystore::initialize`.
- **Encrypted database** — SQLCipher integration via `libsqlite3-sys`
  feature flags, `PRAGMA key` with a raw hex key, consistency check
  immediately after open.
- **Models** — one type per table.
- **Repositories** — `cv_entries`, `applications`, `job_descriptions`,
  `portals`, `settings`, `templates`, `profile` and `prompts` fully implemented.
- **Backend traits** — `BrowserBackend` and `PdfBackend` as separate traits,
  with mocks. `BrowserBackend::print_to_pdf` defaults to
  `BackendUnsupported` so Obscura can rely on it.
- **Migrations** — all eleven tables from §1 of the main plan, including
  `templates` (added in v0.5).
- **State machine** — `AppState` in `crates/app/src/state.rs` encodes the
  "unlock gates the app" rule and is testable without GPUI.
- **Safety tests** — six tests covering the properties from §7 that are
  cheap to verify and expensive to miss.
- **CI** — five jobs covering three platforms, no-secret-service fallback,
  keychain sentinel, never-reinitialize, and fmt/clippy.

## What is stubbed

- **Pipeline screen** — Phase 2.1 Workstreams 1–3 render the shell, lock
  flows, and the Portals screen. The Pipeline screen (Workstream 4) is still
  an empty state.
- **Template seeding** — the `templates` table and repository are in place,
  but no template `.html` files exist yet. Seeding a template system is a
  **Phase 4** deliverable, and Phase 4 should model it on `prompts_seed.rs`
  (update-aware seeding) rather than the current
  `templates::seed_if_empty` pattern, which does not handle content
  changes across releases.
- **Argon2id tuning** — parameters are the RustCrypto Book minimums
  (19 MiB, 2 iterations, 1 parallelism). They are safe but may not hit the
  250–500 ms target on all hardware. Benchmark on your target hardware
  before tuning (Q-3).

## What is now implemented (moved out of "stubbed" since the first draft)

- **`data_dir()`** — Phase 2.1 Workstream 1 resolves the OS-specific app data
  directory via the `directories` crate. `CAREER_OPS_DATA_DIR` remains as a
  development override.
- **eframe shell** — `CareerOpsApp` runs the egui event loop, owns the tokio
  runtime, renders the sidebar, and switches the central panel through
  `NavigationState`.
- **Lock screens** — Phase 2.1 Workstream 2 renders unlock, first-launch, and
  recovery. Lock views return a `LockAction`; `CareerOpsApp` moves `AppState`
  into the async task, restores it with the result, and applies the
  filesystem recovery actions.
- **Portals screen** — Phase 2.1 Workstream 3 renders the portal list with
  health and sidecar badges, add/edit/delete forms, fetch-one, cancellable
  batch fetch with progress, and the manual-entry modal. The app owns the
  fetch source, browser backend, one `PortalsTask`, and the batch progress
  channel; `fetch_batch_cancellable` stops between portals.

- **Prompt seeding** — `crates/core/src/prompts_seed.rs` embeds the
  original `modes/*.md` files at compile time and seeds them into the
  `prompts` table. The seeding function is update-aware: it inserts new
  versions when content changes between releases, preserving historical
  reports that reference old content hashes. Wired into
  `AppState::post_unlock` and `AppState::complete_first_launch`, non-fatal
  on failure so a prompt-seeding error does not block the pipeline UI.
- **Repositories** — `cv_entries`, `applications`, `job_descriptions`,
  `portals`, `settings`, `templates`, `profile`, and `prompts` are all
  implemented. `job_descriptions` includes content-hash dedup (added in
  Phase 1 to remove the Phase 2 migration the original plan assumed).
  `applications` enforces the human-in-the-loop invariant at the
  repository layer: `mark_applied` is the only path to `applied`, and it
  requires the current status to be `ready`.
- **Test harness** — `crates/core/tests/common/mod.rs` provides
  `TestDb::new_raw()` (no keystore, works without the `keychain` feature)
  and `TestDb::new_via_keystore()` (full keystore flow, requires
  `keychain`). Integration tests for prompt seeding are in
  `crates/core/tests/prompts_seed.rs`.
- **Keychain mock** — `setup_keyring_mock()` in the shared harness.

## Known risks to resolve during compilation

1. **`sqlx` + `libsqlite3-sys` feature unification (P1-R1).** The
   `bundled-sqlcipher-vendored-openssl` feature must unify across the
   workspace. If it does not, the build will fail or SQLCipher will not be
   compiled in. This is the primary friction point and should be resolved
   on the first day.
2. **`PRAGMA key` quoting.** The form `"x'<hex>'"` is documented for
   SQLCipher but the exact sqlx `.pragma()` interaction should be verified
   against a real SQLCipher build. If it fails, fall back to the string
   passphrase form and accept the PBKDF2 cost, or use the
   `sqlx-sqlite-cipher` wrapper.
3. **`keyring` 3.x API.** `Entry::new`, `set_password`, `get_password`,
   and `Error::NoEntry` are stable in 3.x. `delete_credential` replaces
   `delete_password` from 2.x. Verify against the current docs.
4. **GPUI dependency.** `crates/app/Cargo.toml` references GPUI from the
   Zed repository. The exact tag or commit should be pinned to match a
   known working build. Alternatively, defer GPUI and ship the state
   machine behind a headless binary until GPUI stabilises.
5. **`argon2` and `chacha20poly1305` versions.** Written against 0.5 and
   0.10 respectively. API surface should be verified.
6. **`sqlx::migrate!("./migrations")` path.** The macro resolves relative
   to `CARGO_MANIFEST_DIR` of the crate that invokes it. The path
   `./migrations` is correct for `crates/core`.
7. **`templates::seed_if_empty` uses the pre-`prompts_seed.rs` pattern.**
   It seeds only when the table is empty and does not handle content
   changes across releases. Phase 4 must replace it with
   `seed_or_update_system_templates` modeled on `prompts_seed.rs`, or the
   same "how do we ship a template change in a new release?" problem will
   surface. This is a Phase 4 concern, not a Phase 1 blocker, but it
   should be tracked in the Phase 4 plan.

## What "done" would look like

Phase 1 exit criteria from the phase plan, restated as compile-and-pass:

1. `cargo test --workspace` passes on Linux, macOS, and Windows.
2. The six safety tests pass, and the sentinel test fails deliberately if
   `keyring` platform features are removed from `Cargo.toml`.
3. `AppState` reaches `Screen::Ready` only after `try_keychain` or
   `try_passphrase` returns `Unlocked`.
4. A database can be created, migrated, closed, and reopened.
5. A wrong key produces a clear `Database` error from `db::open`.
6. The GPUI shell renders the unlock, first-launch, and recovery screens.

Items 1–5 are within reach of this scaffold plus a compilation pass. Item 6
is the largest remaining effort and depends on GPUI's stability.