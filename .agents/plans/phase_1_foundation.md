# Phase 1: Foundation — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Status:** Draft — revised for v0.5 parent
**Last updated:** 2026-10-17
**Duration:** 4–5 weeks
**Depends on:** v0.5 main plan decisions (Phase 0 skipped — see Entry Conditions)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial Phase 1 plan, written against parent plan v0.4. |
| v0.2 | Updated for parent plan v0.5. Removed Phase 0 consumption table (Phase 0 skipped). Added v0.5 decisions as fixed inputs. Corrected Workstream 5 scoping note to name Obscura and `rust-headless-chrome` as the deferred backends. Added Open Questions section (Q-3). No material change to workstreams, estimates, or exit criteria. |

---

## Purpose

Phase 1 establishes the load-bearing subsystems that every later phase depends on: the keystore, the encrypted database, the domain types, the GPUI shell, and the backend trait definitions. Nothing in Phase 1 is user-facing in a complete sense — the pipeline is empty, there is no acquisition, no AI integration, no PDF output. What Phase 1 delivers is a **secure, tested skeleton** that later phases fill in.

The phase is sequenced so that the **keystore is built first**. This is not a stylistic preference. Every other subsystem requires an open database, and the database cannot be opened without a DEK, and the DEK cannot be obtained without the keystore. Building anything else first would mean stubbing the database and then reworking it. Additionally, a late change to key management requires re-encrypting all existing data — so getting it right before data exists is materially cheaper.

---

## Entry Conditions

Phase 1 begins directly from the v0.5 main plan. **Phase 0 was skipped**, and Phase 1 was designed to be independent of Phase 0 outcomes — which is why skipping it has no material impact on this phase's scope, workstreams, or estimates.

The v0.5 decisions that Phase 1 inherits as **fixed inputs**:

| Decision | Implication for Phase 1 |
| :--- | :--- |
| **Browser backend is required** (not feature-gated) | The `BrowserBackend` trait is on the critical path for Phase 2. It is not a speculative abstraction. |
| **Two-backend architecture** (Obscura for scraping, `rust-headless-chrome` for rendering) | The `BrowserBackend` and `PdfBackend` traits are defined as **separate** traits with separate implementations. See §4a and §6 of the main plan. |
| **`printpdf` is post-v1** (Track A) | Phase 1 does not need to accommodate a `printpdf` backend. The `PdfBackend` trait is designed to support it later, but Phase 1 ships only trait definitions and mocks. |
| **Phase 0 is skipped** | There is no fixture corpus from Phase 0. Phase 2 builds its own fixtures. Phase 1 does not depend on any Phase 0 artifact. |

Phase 1 consumes nothing from Phase 2 or later. It is self-contained and can begin as soon as the v0.5 plan is adopted.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Keystore subsystem: envelope encryption, keychain, unlock, recovery | Acquisition logic (`core::acquisition`) |
| Encrypted SQLite database with SQLCipher | Obscura or `rust-headless-chrome` implementation |
| Full schema migrations for all §1 tables | PDF rendering backend |
| Domain types and repository layer for `profile`, `cv_entries`, `applications`, `job_descriptions`, `prompts`, `portals`, `settings`, `templates` | `rig` integration and LLM calls |
| GPUI application shell with navigation | Prompt seeding and management UI |
| Unlock, first-launch, and recovery UI flows | File-based data migration |
| `BrowserBackend` and `PdfBackend` trait definitions + mocks | Full CV editor (deferred to later phase) |
| Test harness and CI configuration | Visual regression testing |

---

## Sequencing Rationale

The build order within Phase 1 is driven by hard dependencies, not preference:

```
W1: Keystore core (DEK, KEK, envelope, AEAD, keychain)
    └── W2: Encrypted DB (SQLCipher, sqlx, migrations)
        └── W3: Domain types + repositories
            └── W4: GPUI shell (unlock → ready state machine)
                └── Integration: real UI on real data

W5: Backend traits (parallel, no dependencies)
W6: Test/CI infrastructure (woven throughout)
```

The GPUI shell needs to host the unlock flow before the database opens, so it cannot be built after the DB without rework. The unlock UI is built against the keystore's public API, and the keystore is developed and tested headlessly before the UI exists. W5 and W6 run in parallel with the main sequence since they have no dependencies on the keystore or database.

---

## Workstream 1 — Keystore Subsystem

**Priority:** First. Everything else depends on it.
**Estimated duration:** 1.5–2 weeks

### Deliverables

1. `core::keystore` module with a public API for:
   - First-launch initialization
   - Unlock via keychain
   - Unlock via passphrase
   - Key rotation (passphrase change; full DEK rotation)
   - Recovery-state detection
2. `dek.wrapped.json` versioned file format
3. Keychain integration using the `keyring` crate with all platform features explicitly enabled
4. Argon2id passphrase derivation with tuned work factor
5. AEAD-wrapped DEK (AES-256-GCM or XChaCha20-Poly1305)
6. Zeroization of in-memory key material

### Public API Sketch

```rust
pub struct Keystore { /* ... */ }

pub enum UnlockOutcome {
    Unlocked(Zeroizing<[u8; 32]>),
    NeedsPassphrase,
    KeychainUnavailable,
    RecoveryRequired(RecoveryReason),
}

impl Keystore {
    pub fn initialize(passphrase: &str) -> Result<Self>;
    pub fn unlock_with_keychain(&self) -> Result<UnlockOutcome>;
    pub fn unlock_with_passphrase(&self, passphrase: &str) -> Result<UnlockOutcome>;
    pub fn rotate_passphrase(&self, old: &str, new: &str) -> Result<()>;
    pub fn export_encrypted_bundle(&self, dest: &Path) -> Result<()>;
}
```

### Critical Implementation Rules

1. **Never re-initialize.** If `dek.wrapped.json` exists on disk, the application must not generate a new DEK without explicit, informed user confirmation gated behind a typed confirmation phrase.
2. **Distinguish `NoEntry` from unwrap failure.** `keyring` returning `NoEntry` on first launch is expected. `NoEntry` when `dek.wrapped.json` already contains a keychain wrap is an error state, not an invitation to start over.
3. **Verify keys before writes.** After `PRAGMA key`, execute a read against a known row before allowing any writes.
4. **AEAD for wrapping.** A corrupted wrap must fail loudly rather than silently produce a wrong DEK.
5. **Zeroize everything.** Use `zeroize` on `DEK`, `KEK`, and derived key material. Minimize their lifetime in memory.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| First-launch flow generates DEK, writes keychain entry, writes wrapped file | Integration test |
| Unlock via keychain succeeds on a warm keychain | Integration test |
| Unlock via passphrase succeeds when keychain unavailable | Integration test with keychain backend disabled |
| Wrong passphrase fails without producing a DEK | Unit test |
| Corrupted wrap fails without producing a DEK | Unit test (bit-flip in ciphertext) |
| Missing `dek.wrapped.json` with existing DB blocks startup | Integration test |
| Existing `dek.wrapped.json` blocks fresh initialization | Integration test |
| Passphrase change re-wraps without re-encrypting the DB | Integration test |
| `keyring` sentinel round-trip works on each target platform | CI job per platform |

---

## Workstream 2 — Encrypted Database Foundation

**Priority:** After W1 core is stable.
**Estimated duration:** 1 week

### Deliverables

1. SQLCipher integration via `libsqlite3-sys` with the appropriate feature flags
2. `sqlx` connection setup with `PRAGMA key` applied at open time
3. Key consistency check immediately after open
4. Versioned migration system using `sqlx::migrate!`
5. Initial migration covering all §1 tables (including `templates`, added in v0.5)
6. Connection management suitable for a desktop app (pool or single connection with serialized writes)

### Critical Concerns

1. **SQLCipher + `sqlx` integration friction.** `sqlx` uses `libsqlite3-sys` under the hood. SQLCipher integration typically requires either a `bundled-sqlcipher` feature on `libsqlite3-sys` or linking against a system SQLCipher. Verify the correct feature configuration during the first day of this workstream; if it proves problematic, escalate immediately rather than burning days.
2. **Test strategy.** Most tests should use unencrypted in-memory SQLite for speed. A small dedicated integration suite exercises the encrypted, file-backed path end-to-end.
3. **Migration discipline.** Once Phase 1 ships, migrations are append-only. Never edit a released migration.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Database opens with SQLCipher using a DEK from the keystore | Integration test |
| Wrong key fails the consistency check before any write | Integration test |
| All §1 tables (including `templates`) created by the initial migration | Schema assertion test |
| Migrations run idempotently | Integration test (run twice) |
| Foreign keys enforced (`PRAGMA foreign_keys = ON`) | Unit test |
| Indexes present on `applications.status`, `applications.job_description_id`, `prompts.category` | Schema assertion test |

---

## Workstream 3 — Domain Types and Repositories

**Priority:** After W2 migrations exist.
**Estimated duration:** 1 week

### Deliverables

1. Rust structs for every §1 table, with `sqlx::FromRow` derives
2. Repository modules: `core::profile`, `core::pipeline`, `core::prompts`, `core::portals`, `core::settings`, `core::templates`
3. CRUD operations for each entity
4. JSON column handling via `serde_json` with validation on read
5. Domain-level types (e.g. `ApplicationStatus` enum, `FetchTier` enum) distinct from raw DB rows

### Scoping Note

This workstream delivers the **repository layer**, not the UI. Forms and editors are built in later workstreams and phases. Phase 1 includes only minimal GPUI forms for profile identity fields (name, email, location) to prove the round-trip. The full CV editor is deferred — it is a substantial UI effort and does not belong in the foundation phase.

The `templates` repository is included even though templates are not used until Phase 4. Defining the repository alongside the schema keeps the migration and the type system in sync, and avoids a schema-and-types retrofit later.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Full CRUD for `profile`, `cv_entries`, `portals`, `prompts`, `settings`, `templates` | Unit tests per repository |
| `applications` insert/read/update with status transitions | Unit tests |
| JSON column round-trip preserves structure | Unit tests |
| Invalid JSON in a column produces a typed error, not a panic | Unit test |
| `ApplicationStatus` and `FetchTier` enums reject unknown values from the DB | Unit test |

---

## Workstream 4 — GPUI Application Shell

**Priority:** After W1 core API is stable (unlock UI needs it).
**Estimated duration:** 1–1.5 weeks

### Deliverables

1. Main application window with `gpui-kit` integrated
2. Application state machine: `Booting → FirstLaunch | Unlocking | Recovery | Ready`
3. **Unlock screen** — passphrase entry, keychain retry, error display
4. **First-launch wizard** — passphrase creation with explanation of why it is required
5. **Recovery screen** — the five options from §7 of the main plan (passphrase, retry keychain, locate key file, export encrypted DB, last-resort reset)
6. Navigation shell with sidebar or tab structure
7. Placeholder screens (empty states) for Dashboard, Pipeline, Job Detail, CV/Profile, Portals, Prompts, Settings
8. Theme and styling baseline
9. Minimal profile identity form wired to the repository layer

### Critical Design Rules

1. **Unlock gates the app.** No screen other than unlock, first-launch, or recovery is reachable until a DEK is obtained. This is a hard architectural constraint, not a UI convention.
2. **Recovery is a first-class state.** Not an error dialog, not a modal. Users in this state are already worried; the UI must be calm, explanatory, and offer a way forward.
3. **No silent fallback.** If the keychain is unavailable, the UI says so. If the passphrase is wrong, the UI says so. The application does not pretend to work.
4. **Backend selection is not exposed yet.** The settings screen in Phase 1 is a placeholder. Backend selection UI arrives in Phase 5, once both backends (Obscura in Phase 2, `rust-headless-chrome` for PDF in Phase 4) exist.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| App starts to unlock screen with an existing `dek.wrapped.json` | Manual + automated UI test |
| App starts to first-launch wizard with no key files | Manual + automated UI test |
| App starts to recovery screen when unlock fails | Manual + automated UI test |
| Unlock with valid passphrase reaches Ready state | UI integration test |
| No route to Ready state without a DEK | Architectural test (assert gating) |
| Profile form round-trips data to the database | UI integration test |
| Navigation reaches all placeholder screens | Smoke test |

---

## Workstream 5 — Backend Trait Definitions

**Priority:** Parallel with W2–W4, no dependencies.
**Estimated duration:** 2–3 days

### Deliverables

1. `BrowserBackend` trait per §4a of the main plan (v0.5)
2. `PdfBackend` trait per §6 of the main plan (v0.5)
3. Mock implementations of each for testing
4. Shared types: `RenderedPage`, `PdfOptions`, `RenderOpts`
5. Documentation of the two-backend architecture (scraping vs. rendering) and the `Unsupported` return path for `BrowserBackend::print_to_pdf`

### Scoping Note

Phase 1 defines the traits and mocks. It does **not** implement the Obscura backend (Phase 2) or the `rust-headless-chrome` `PdfBackend` (Phase 4). The purpose of defining traits in Phase 1 is to lock the interface before implementations arrive, so that later work cannot accidentally couple the application to a specific crate.

**v0.5-specific requirement:** `BrowserBackend::print_to_pdf` must be allowed to return `PdfError::Unsupported`. Obscura does not render PDFs (§6 of the main plan), so the scraping backend will return this error. The trait must support that return path without breaking, and the mock must be able to simulate it for testing.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Traits compile and are object-safe (or otherwise usable as `dyn`) | Compile test |
| Mock `BrowserBackend` returns a canned `RenderedPage` | Unit test |
| Mock `BrowserBackend` can return `PdfError::Unsupported` from `print_to_pdf` | Unit test |
| Mock `PdfBackend` returns deterministic bytes for fixed input | Unit test |
| No concrete browser or PDF crate appears in `Cargo.toml` | Build assertion |
| Trait documentation states the two-backend architecture and the `Unsupported` path | Review |

---

## Workstream 6 — Test and CI Infrastructure

**Priority:** Woven throughout, with dedicated setup at the start.
**Estimated duration:** 2–3 days of focused work, plus ongoing.

### Deliverables

1. Test harness setup: `sqlx::test` patterns, fixture directory conventions
2. Keychain mock for tests (in-memory backend implementing the same interface)
3. CI configuration for Linux, macOS, and Windows
4. **Dedicated CI job: unlock without Secret Service.** Runs the passphrase unlock path on Linux with no keychain backend available, asserting graceful fallback.
5. **Dedicated CI job: keychain sentinel round-trip.** Writes and reads a sentinel value through `keyring`, asserting platform features are enabled. Fails loudly if they are not.
6. **Dedicated CI job: "never re-initialize" assertion.** Constructs a state where `dek.wrapped.json` exists but cannot be unwrapped, runs startup, asserts no new DEK is generated and the DB file is untouched.
7. Coverage reporting baseline

### Critical Testing Principles

1. Most tests use unencrypted in-memory SQLite. The encrypted path is exercised by a small, dedicated integration suite.
2. Backend trait tests use mocks by default. Real backend tests are environment-gated and opt-in.
3. The three dedicated CI jobs above are **not optional**. They encode safety properties that are easy to break accidentally and expensive to debug in production.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| CI runs on all three platforms | CI configuration |
| Keychain sentinel job fails when `keyring` platform features are removed | Deliberate-failure test |
| No-Secret-Service job passes with passphrase fallback | CI green |
| Never-re-initialize job passes | CI green |
| Test suite completes in under 5 minutes on a developer laptop | Timing check |

---

## Critical Test-First Items

These tests should be written **before** the corresponding implementation, because they encode safety properties rather than functionality:

| Test | Encodes | Workstream |
| :--- | :--- | :--- |
| Keychain sentinel round-trip | `keyring` platform features present | W1, W6 |
| Never re-initialize on existing `dek.wrapped.json` | No destructive fallback on unlock failure | W1, W6 |
| Wrong key fails before any write | Key consistency check enforced | W2 |
| Wrong passphrase does not produce a DEK | AEAD correctness | W1 |
| No route to Ready without a DEK | Unlock gates the app | W4 |
| Unlock works without Secret Service | Passphrase fallback functional | W1, W6 |
| `BrowserBackend::print_to_pdf` returns `Unsupported` for the mock | Two-backend architecture supported | W5 |

---

## Exit Criteria

Phase 1 is complete when **all** of the following are true:

1. A new user can launch the app, complete the first-launch wizard, and reach the Ready state.
2. A returning user with a warm keychain reaches Ready without entering a passphrase.
3. A returning user with no keychain access reaches Ready via passphrase.
4. A user with a corrupted or missing key file sees the recovery screen, not a crash or a silent reset.
5. All §1 tables exist with correct schema, indexes, and foreign keys — including `templates`.
6. Profile identity data round-trips from UI to database to UI.
7. `BrowserBackend` and `PdfBackend` traits are defined, mocked, and tested, with no concrete backend crate in `Cargo.toml`.
8. `BrowserBackend::print_to_pdf` supports the `Unsupported` return path.
9. CI is green on Linux, macOS, and Windows, including the three dedicated safety jobs.
10. No `unwrap()` on fallible operations in keystore code paths — all errors are typed and surfaced.
11. A short Phase 1 retrospective memo exists, recording what was harder or easier than estimated.

---

## Phase 1 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P1-R1 | **SQLCipher + `sqlx` integration friction.** Feature configuration may prove difficult, and the combination is less documented than plain SQLite. | High | Timebox to 1 day. If unresolved, evaluate `libsqlite3-sys` with `bundled-sqlcipher-vendored-openssl` or a pure-Rust alternative (`limbo`, `turso`) before proceeding. Escalate rather than persist. |
| P1-R2 | **`keyring` platform backend gaps.** Missing features cause silent `NoEntry`; zeroization is incomplete. | High | Explicit feature flags; sentinel round-trip test; treat keys as sensitive and minimize lifetime. Already tracked as R-10 in the main plan. |
| P1-R3 | **Argon2id work factor tuning.** Requires real hardware calibration; wrong tuning either weakens security or creates a 2-second unlock delay. | Medium | Benchmark on representative hardware during W1. Target 250–500 ms. Make the parameter a build-time constant, not a runtime setting. (See Q-3.) |
| P1-R4 | **GPUI API churn mid-implementation.** GPUI is pre-1.0; breaking changes may land during the 4–5 week phase. | Medium | Pin to a specific commit or version. Isolate GPUI behind app-level view types so churn is absorbed in one place. |
| P1-R5 | **Encrypted-path test coverage gap.** Most tests use in-memory unencrypted SQLite; the encrypted path may be under-tested. | Medium | Dedicated integration suite for the encrypted, file-backed path. Treat it as a required CI job, not optional. |
| P1-R6 | **Recovery UX scope creep.** The recovery screen is easy to over-design. | Low | Ship the five options from §7. Polish in Phase 5. Resist adding features. |
| P1-R7 | **"Just start building the real pipeline" temptation.** Scope creep into Phase 2 work. | Medium | Phase 1 exit criteria are explicit. Do not begin acquisition, AI, or rendering work until they are met. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | W1 keystore core: DEK/KEK generation, envelope wrapping, AEAD, `keyring` integration, Argon2id calibration | Keystore module compiles and passes unit tests headlessly |
| **2** | W1 unlock flows, recovery detection, rotation; W6 test infrastructure setup | Keystore complete with all critical tests passing; CI configured on three platforms |
| **3** | W2 SQLCipher + `sqlx` integration, initial migration, key consistency check; W5 trait definitions | Encrypted DB opens with a real DEK; traits defined and mocked |
| **4** | W3 domain types and repositories | All repositories implemented and tested |
| **5** | W4 GPUI shell, unlock/first-launch/recovery UI, navigation, profile form; integration | End-to-end: launch → unlock → ready → profile round-trip |

**Total:** 4–5 weeks with buffer. If any week overruns by more than 2 days, escalate — Phase 1 has hard dependencies in Phase 2 and slip here cascades.

---

## Open Questions (Phase 1 Owns)

The main plan (v0.5) tracks open questions in Appendix B. Phase 1 is responsible for resolving one of them:

| # | Question | Resolution phase |
| :--- | :--- | :--- |
| Q-3 | What is the target Argon2id work factor for representative hardware? | Phase 1, Week 1 |

The other Appendix B questions belong to later phases and do not affect Phase 1.

---

## What Phase 1 Is Not

Phase 1 is **not** a working application. It is a secure, tested skeleton. Specifically:

- There is no way to add a job posting.
- There is no evaluation, no cover letter generation, no PDF output.
- There is no Obscura or `rust-headless-chrome` implementation — only traits and mocks.
- There is no `rig` integration, no LLM calls, no prompt seeding.
- There is no file-based data migration.
- The CV editor is a placeholder, not an editor.
- The settings screen is a placeholder. Backend selection arrives in Phase 5.

The temptation to add "just one pipeline feature" before Phase 1 is complete is the most common way foundation phases overrun. Resist it. Phase 2 is where the application becomes useful.

---

The single most important thing to take from this plan: **write the keystore's safety tests before writing the keystore.** The three dedicated CI jobs — keychain sentinel, never-re-initialize, no-Secret-Service — encode properties that are cheap to verify and catastrophic to miss. Everything else in Phase 1 is scaffolding. Those three tests are the actual foundation.