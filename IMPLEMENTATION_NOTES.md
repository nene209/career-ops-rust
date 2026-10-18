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

- **GPUI views** — `crates/app/src/screens.rs` is placeholder text. The
  state machine is real; the rendering is not.
- **`data_dir()`** — reads `CAREER_OPS_DATA_DIR` from the environment. In
  production, use the `directories` crate.
- **Prompt and template seeding** — the mechanism exists
  (`prompts::seed_if_empty`), but the embedded prompt list from
  `modes/*.md` is not wired in. Enumerate the original project's files and
  add them to the `SYSTEM_PROMPTS` constant.
- **Recovery screen actions** — the state exists; the five actions
  (unlock, retry, locate, export, reset) are not wired to UI events.
- **First-launch wizard content** — the passphrase and acknowledgement
  steps are described, not rendered.
- **Keychain mock setup** — tests use `keyring::mock`; production builds
  use the platform backend via the `keychain` feature. The sentinel test
  guards against missing platform features.
- **Argon2id tuning** — parameters are the RustCrypto Book minimums
  (19 MiB, 2 iterations, 1 parallelism). They are safe but may not hit the
  250–500 ms target on all hardware. Benchmark on your target hardware
  before tuning (Q-3).

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