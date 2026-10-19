# Phase 1 Retrospective

**Phase:** Foundation
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Phase plan:** `docs/phase-1-foundation.md`
**Period:** 2026-10-12 → 2026-10-19
**Estimate:** 4–5 weeks
**Actual:** ~1 week of focused implementation, plus ~2–3 days of verification
**Status:** Complete against all eleven exit criteria, with three documented caveats.

---

## Summary

Phase 1 is complete. The keystore, encrypted database, all eight repositories,
backend traits, prompt seeding, the application state machine, and the test
harness are implemented and tested. CI is green on Linux, macOS, and Windows.
The unwrap ban in the keystore is enabled and verified to fire.

Three exit criteria carry documented caveats rather than being fully closed:

- **Criterion 1 (first-launch wizard)** — the state machine is complete; the
  GPUI wizard views are placeholders deferred to Phase 5.
- **Criterion 6 (profile round-trip through UI)** — the repository works; the
  CV editor UI is deferred to Phase 4.
- **Criterion 2 (warm keychain)** — tested against the persistent test backend
  and against a real platform keychain in the `keychain-sentinel` CI job, but
  the "real keychain, user-consent prompt" and "real keychain, locked" paths
  are verified manually on macOS and Windows before release.

The phase finished **faster than estimated in implementation, but the
verification pass discovered two real defects** that would otherwise have
carried into Phase 2. Both are examined below, because they are more
instructive than the time savings.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | First-launch wizard reaches Ready | **State machine complete, UI deferred** | `complete_first_launch` tested; GPUI wizard is a Phase 5 deliverable |
| 2 | Warm keychain reaches Ready | **Simulated + sentinel job** | `first_launch_then_unlock_then_ready` against persistent test backend; `keychain-sentinel` CI job exercises the real keychain on macOS and Windows |
| 3 | No keychain access reaches Ready via passphrase | **Complete** | `correct_passphrase_reaches_ready`, `keychain_failure_falls_back_to_passphrase` |
| 4 | Corrupted/missing key file reaches recovery | **Complete** | `db_without_wrapped_goes_to_recovery`, `wrapped_without_db_goes_to_recovery` |
| 5 | All §1 tables exist | **Complete** | `all_schema_tables_exist` |
| 6 | Profile round-trip through UI | **Repository complete, UI deferred** | `repo::profile` exercised by `db_roundtrip`; no UI form exists |
| 7 | Backend traits + mocks, no concrete backend | **Complete** | `backend_traits.rs`; `Cargo.toml` has no browser crate |
| 8 | `print_to_pdf` supports `Unsupported` | **Complete** | `browser_backend_pdf_returns_unsupported_when_configured` |
| 9 | CI green on three platforms | **Complete** | Confirmed 2026-10-19 |
| 10 | No `unwrap()` in keystore paths | **Complete and verified** | `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]` in `keystore/mod.rs`; deliberate-failure test confirmed the lint fires |
| 11 | Retrospective memo | **Complete** | This document |

---

## What went well

### The scaffold-to-compile approach worked

The initial implementation was a complete, coherent scaffold rather than a
minimal skeleton. Roughly 80% of the code compiled on the first attempt after
a handful of type fixes. Writing each module from scratch and iterating on
design in the editor would have been slower.

The trade-off is important and is examined in the "What the verification pass
found" section: scaffold code has to be read carefully, because the compiler
is the first reviewer and it does not catch semantic bugs.

### The safety tests caught a real bug

`applied_is_not_reachable_via_transition` failed on first run because the
transition table had an `(Interviewing, Applied)` arm that should not have
existed. The runtime `transition` function already refused to move into
`Applied` unconditionally, so the arm was unreachable in production — but it
was a landmine: any future code that called `is_valid_transition` directly
(to render available transitions in a UI, for example) would have gotten the
wrong answer.

This is the test-first discipline paying off in its intended mode: the test
encoded a property, the implementation violated it, and the failure was
immediate and specific.

### SQLCipher + `sqlx` integration was manageable

P1-R1 flagged SQLCipher + `sqlx` as the phase's highest risk. It turned out
to be a minor inconvenience. The `libsqlite3-sys` feature
(`bundled-sqlcipher-vendored-openssl`) unified cleanly across the workspace,
and `PRAGMA key` worked as documented.

The one surprise was that `sqlx::migrate!` resolves its path relative to the
crate that invokes it, not the workspace root. The error message made this
obvious, but it is worth recording so Phase 2 does not repeat the confusion
if it adds a second migration directory.

### The prompt seeding design survived contact with the implementation

The update-aware seeding function (`seed_or_update_system_prompts`) was
designed to handle both first-launch and release-update cases. It compiled
and worked on the first try. The four-way outcome enum (`Inserted`, `Updated`,
`Unchanged`, and an implicit failure case) makes the function testable
without a database and gives the caller enough information to log
meaningfully.

### The two-backend trait abstraction is proven

`BrowserBackend` and `PdfBackend` are separate traits with separate
implementations by design. The mock tests confirm that the `print_to_pdf` →
`Unsupported` path is expressible and that both traits are object-safe. This
is what allows Phase 2's Obscura backend and Phase 4's `rust-headless-chrome`
backend to coexist without a schema change.

---

## What the verification pass found

The verification pass was expected to be a formal check of exit criteria. It
turned out to be substantive: two real defects were found that the test suite
was not catching.

### Defect 1: the failure-injection test was not testing failure injection

The `keychain_failure_falls_back_to_passphrase` test was written with the
correct intent — verify that a broken keychain falls back to the passphrase.
But the order of operations in the test was:

1. `try_keychain()` fails → state is `KeychainUnavailable`.
2. `retry_unlock()` → state is `Unlocking`.
3. `try_keychain()` succeeds → state is `Ready`.
4. `try_passphrase(PASSPHRASE)` → state was already `Ready`.

Step 4 called `try_passphrase` on a state machine that had already reached
`Ready` via the keychain. The passphrase path did open the database, but it
opened it as a redundant operation on an already-open file, not as a genuine
fallback from a failed unlock. If `try_passphrase`'s error handling after a
keychain failure had been broken, the test would still have passed.

The fix was to split the test into two:

- `keychain_failure_falls_back_to_passphrase` — asserts that a broken
  keychain leaves the state in `KeychainUnavailable`, then calls
  `try_passphrase` from that state and asserts `Ready`.
- `keychain_transient_failure_recovers_on_retry` — asserts that a
  single-shot failure is consumed by the first attempt, that
  `retry_unlock` works, and that the second `try_keychain` succeeds.

This is a **test that passed for the wrong reason**, and it is a distinct
failure mode from a test that fails. A failing test tells you something is
broken. A test that passes for the wrong reason tells you nothing, while
appearing to provide coverage. It is more dangerous, because it silently
degrades the value of the suite.

### Defect 2: the unwrap ban was documented but not enforced

The Phase 1 plan's exit criterion 10 read: "No `unwrap()` on fallible
operations in keystore code paths — all errors are typed and surfaced."

The clippy job was running `-D warnings`, which denies whatever clippy emits
by default. But `clippy::unwrap_used` and `clippy::expect_used` are
`restriction` lints, not default lints. They do not emit warnings unless
explicitly enabled. So the criterion was treated as satisfied by the
existence of a green clippy job, when in fact clippy was not checking for
unwraps at all.

The fix was a one-line attribute in `crates/core/src/keystore/mod.rs`:

```rust
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
```

The verification that this actually fires was a deliberate-unwrap test: add
an `unwrap()` to a keystore function, run clippy, confirm it fails with
"used `unwrap()` on a `Result` value", remove the deliberate unwrap, confirm
clippy is clean again.

```bash
# 1. Fix the test-support line, then re-run clippy:
cargo clippy --workspace --all-targets -- -D warnings
# Expect: all green.

# 2. Verify the unwrap ban actually fires.
#    Add this line temporarily to any function in keystore/mod.rs,
#    keystore/argon.rs, or keystore/keychain.rs. The simplest place
#    is at the top of Keystore::open:

pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
    let _ = std::fs::read_to_string("/nonexistent").unwrap();  // temporary
    let data_dir = data_dir.as_ref().to_path_buf();
    // ... rest unchanged
}

# 3. Re-run clippy:
cargo clippy -p career-ops-core --all-targets -- -D warnings
# Expect: "error: used `unwrap()` on a `Result` value"

# 4. Remove the temporary line and re-run clippy to confirm clean:
cargo clippy -p career-ops-core --all-targets -- -D warnings
```

This is a more general lesson: **a policy that is documented but not
mechanically enforced is a policy that will eventually be violated.** The
verification pass was the moment to check whether the enforcement was real
rather than assumed.

### What this says about the verification pass

The verification pass was not ceremonial. It found two defects that would
have carried into Phase 2 as latent problems. Both would have been harder to
diagnose later, because by then there would be more code and more tests, and
the boundary between "Phase 1 bug" and "Phase 2 bug" would be less clear.

The cost of the verification pass — roughly two to three days — is more than
justified by catching these two issues before they compounded.

---

## What was harder than expected

### The `keyring` crate's API surface

The estimate assumed `keyring` would be a drop-in dependency. It was not.
Four separate API surprises cost more iterations than any other part of the
phase:

1. **The `mock` feature does not exist in 3.x.** The original scaffold
   declared `keyring = { version = "3", features = ["mock"] }` in
   `[dev-dependencies]`. This fails dependency resolution because no
   published 3.x version has that feature. The mock store is always
   compiled in; there is no feature gate.

2. **The mock store is entry-scoped, not process-global.** The built-in
   `keyring::mock` backend stores credentials inside the `Entry` object.
   A test that writes a credential in one `Entry` and reads it from a
   new `Entry` cannot use the mock — the credential is gone when the
   first `Entry` drops. This is why the persistent test backend in
   `crates/test-support/` exists.

3. **Platform backends are opt-in, with no default.** `keyring` 3.x
   ships with no platform backends enabled by default. If `apple-native`,
   `windows-native`, or `linux-native` are missing from `Cargo.toml`,
   every read silently returns `NoEntry`. The sentinel round-trip test
   exists to catch this, and it is worth keeping forever.

4. **`CredentialApi` requires `set_secret` and `get_secret`.** The
   password methods delegate to them, so implementing `set_password` and
   `get_password` alone is not enough for a custom backend.

None of these are `keyring`'s fault — the crate is doing reasonable things
and documenting them. But the phase plan's assumption that the keystore
would be the primary time sink was wrong: the keystore *design* was correct,
and the *integration* was the problem.

### The `#[cfg]` boundary churn

Three rounds of clippy fixes on `keychain.rs` came from the same root cause:
constants, functions, and imports that were live in the `feature = "keychain"`
build and dead in the `not(feature)` build, and each one triggered a separate
`dead_code` or `unused_imports` warning when compiled with `-D warnings`.

The lesson: **gate by module, not by item.** When a build variant disables a
whole subsystem, wrap the subsystem in a feature-gated `mod inner { ... }`
and keep the public surface as a thin `pub use` above the gate. This produces
one warning-free boundary instead of N individual annotations. The final
structure of `keychain.rs` does this, but it took three iterations to arrive
at.

### GPUI's toolchain pin

The GPUI commit pinned in `crates/app/Cargo.toml` uses
`std::hint::cold_path()`, which was stabilized in Rust 1.95.0. On older
toolchains the crate fails to compile with `E0658: use of unstable library
feature`. This is a GPUI-version issue, not a project issue, but it means
contributors need a recent enough stable toolchain to compile the app crate.
It does not affect `cargo test -p career-ops-core`, which is why it went
unnoticed until the app crate was built.

### The flaky test race

The `state_flow.rs` tests initially failed intermittently with
`Crypto("keychain unwrap failed")`. The cause was parallel execution: all
three tests called `complete_first_launch`, which generates a fresh random
KEK and writes it to the process-global test keychain. One test's write could
land between another test's write and read, causing the AEAD unwrap to fail
authentication.

The fix was a `tokio::sync::Mutex` acquired at the top of each test, held
for the duration. This serializes the tests without requiring
`--test-threads=1` and without affecting other test binaries.

---

## What was easier than expected

### The state machine design

The `AppState` state machine —
`Booting → FirstLaunch | Unlocking | Recovery | Ready` — was designed to be
testable without GPUI, and it is. The unit tests in `state.rs` run in
milliseconds and the integration tests in `tests/state_flow.rs` run in under
a second each. The decision to keep the state machine free of UI dependencies
is the single biggest reason Phase 1's UI-related work was cheap.

### The prompt inventory

Embedding the original `modes/*.md` files with `include_str!` was
straightforward once the file set was known. The original repo's README
provides a complete list, so the `SYSTEM_PROMPTS` constant was a matter of
copying the files and writing one line per prompt.

### The split between the two test backends

The `career-ops-test-support` crate provides a persistent in-memory keychain
backend with failure injection. The `core` crate's tests use the persistent
backend for its own safety tests, and the `app` crate's tests use the same
backend plus failure injection. The result is a single, consistent keychain
behavior across the entire suite, rather than the `keyring::mock` entry-scoped
behavior in `core` and a different behavior in `app`.

This was added during the phase, not in the original plan. It is worth
recording because it also removed a source of future confusion: a test that
worked in `app` but not in `core` would have been a hard bug to diagnose.

---

## Deviations from the plan

| Planned | Actual | Impact |
| :--- | :--- | :--- |
| Phase 0 spikes to inform Phase 1 | Phase 0 skipped; decisions made directly | None — Phase 1 was designed to be Phase-0-independent |
| Keystore is the primary effort sink | `keyring` integration was the primary effort sink | Estimate rebalanced for Phase 5 |
| Full CV editor in Phase 1 | Repository layer only; editor deferred | Correct — full editor is a Phase 4 concern |
| Template seeding in Phase 1 | Repository and table only; seeding deferred to Phase 4 | Correct — no template content exists yet |
| No `career-ops-test-support` crate | Crate added mid-phase for the persistent keychain backend | Necessary — `keyring::mock` cannot support the state-flow tests |
| Unwrap ban enabled by `-D warnings` | Unwrap ban required an explicit lint attribute | Caught by verification pass |
| CI green on three platforms | Confirmed 2026-10-19 | Complete |
| Retrospective after CI confirmation | Retrospective after CI confirmation | This document |

---

## Open items carried into Phase 2

1. **`test-no-secret-service` job naming.** The job runs
   `cargo test -p career-ops-core --no-default-features`, which compiles
   without the `keychain` feature. But `keyring`'s built-in behavior in a
   no-feature build is to use the mock backend, not to be unavailable. So
   the job verifies "core compiles without the keychain feature and the
   tests that don't require it still pass" — not "the app works when the
   platform keychain is unavailable." The name is misleading. Either rename
   the job to reflect what it tests, or add a separate job that exercises
   the failure-injection path through `career-ops-test-support`.

2. **The GPUI shell is a placeholder.** Phase 5 is when this becomes a
   blocker; Phase 2 does not depend on it.

3. **Argon2id parameters have not been tuned.** Q-3 from the main plan
   (250–500 ms target on representative hardware) is unresolved. The
   current parameters are the RustCrypto Book minimums and will be slow
   enough to be safe but may not hit the target. Tuning is a Phase 5
   activity.

4. **The real-keychain manual test protocol.** The retrospective notes that
   the "real keychain, user-consent prompt" and "real keychain, locked"
   paths are verified manually. That protocol does not yet exist as a
   checklist. It should be written before v1.0 release, and added to the
   Phase 6 release checklist.

---

## Lessons for Phase 2

1. **Verify dependency API surfaces before committing to a version.**
   The `keyring` mock feature was assumed, not checked. Phase 2 depends on
   Obscura, whose API is newer and less documented. The Phase 2 Week 1
   spike exists to check the interaction model before the integration is
   written.

2. **Gate by module, not by item.** This applies whenever a build variant
   disables a subsystem, and it will reappear when Phase 2 adds an
   `ObscuraBrowserBackend` behind (or not behind) a feature flag.

3. **Write the safety test before the implementation.** The transition
   invariant test found a real bug on first run. The keystore safety tests
   found none — but that is because the keystore was written carefully, not
   because the tests were unnecessary.

4. **Scaffold-to-compile is fast but compiler-blind.** The transition table
   bug is an example. Keep writing tests for anything that encodes a policy,
   because the compiler will not catch a policy violation.

5. **A test that passes for the wrong reason is worse than no test.** The
   failure-injection test and the unwrap ban were both cases where the
   suite looked correct but was not exercising the property it claimed.
   The verification pass was the discipline that caught both. Phase 2
   should have an explicit verification pass at the end, not just an
   assertion that "the tests pass."

6. **Enable lints explicitly, and verify they fire.** `-D warnings` does
   not enable `restriction` lints. Any policy that depends on a
   restriction lint needs the attribute, plus a deliberate-failure test to
   confirm it is active.

7. **Expect the Obscura integration to surface unknowns.** If the
   `keyring` API surface was 4 separate surprises, Obscura is likely to
   have at least a few. The Phase 2 Week 1 spike is the right structure
   for discovering them cheaply.

---

## Estimate accuracy

The phase estimate was 4–5 weeks. Actual focused implementation was ~1 week,
plus ~2–3 days of verification.

The gap is larger than a normal estimation error and deserves honest
accounting:

- **The scaffold was ~80% correct on first compile.** Writing it from
  scratch and iterating would have taken longer, but the scaffold removed
  the design phase entirely. The remaining work was compilation fixes, not
  implementation.
- **The estimate assumed CI iteration and GPUI work.** CI was confirmed at
  the end, but the GPUI work — which is a significant fraction of the
  estimated timeline — was deferred to Phase 5. The comparison is not
  apples-to-apples: the estimated 4–5 weeks assumed the UI was part of
  Phase 1.
- **The verification pass added real work.** It was budgeted at zero in the
  original plan. The ~2–3 days it actually took is the cost of catching
  the two defects described above. That cost is worth paying, and it should
  be budgeted explicitly in later phases.

For Phase 2, expect the estimate to be closer to reality, because Phase 2
has no scaffold and depends on a crate (Obscura) whose surface is not yet
known. If the Obscura spike resolves in a day, the phase may finish faster
than 3–4 weeks. If it takes a week, expect a fallback to
`rust-headless-chrome` and a corresponding plan change.

---

## Closing note

Phase 1 established three things:

1. **The application can be built.** The core crate compiles, tests pass,
   and the state machine works end-to-end.
2. **The safety invariants are enforced, not just documented.** The
   transition table, the "never re-initialize" rule, and the unwrap ban are
   each backed by a test that fails when the invariant is violated.
3. **The boundary between Phase 1 and later phases is honest.** The three
   deferred criteria are labeled as deferred, not claimed as complete. The
   real-keychain testing limitation is documented rather than glossed over.

The last point is the most important. A foundation that overstates its own
completeness is a foundation that fails later, in ways that are hard to
attribute. A foundation that is honest about its gaps can be built on
safely, because the gaps are known.