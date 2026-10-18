# Phase 5: Polish & Hardening — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.4), §12
**Status:** Draft
**Last updated:** 2026-10-12
**Duration:** 3–4 weeks
**Depends on:** Phase 4 completion (see Entry Conditions)

---

## Purpose

Phase 5 turns a working application into a shippable one. It closes the gap between "it works on the developer's machine" and "someone who is not the developer can use it without frustration." This is the phase where the settings screen becomes complete, key rotation is exposed, the onboarding flow explains the encryption model in plain language, error handling is audited for panics and silent failures, and platform-specific testing is comprehensive.

Phase 5 is **not** a feature phase. Feature work ended in Phase 4. Phase 5 is about correctness, completeness, and user experience. The most common way this phase overruns is by adding features under the guise of "polish" — a new template, a new prompt type, a new backend. Every one of those additions displaces hardening work that is genuinely necessary for release.

The phase has three defining characteristics:

1. **Settings becomes complete.** Every user-configurable option is exposed, every compile-time capability is visible, and every destructive action has a confirmation flow.
2. **Platform reality is tested, not assumed.** macOS Keychain, Windows Credential Manager, and Linux Secret Service (present, absent, and locked) are each exercised. The passphrase fallback is verified on all three.
3. **Error handling is audited.** No `unwrap()` on fallible operations outside of documented invariants. Every error path produces a typed error and a user-visible message with an actionable next step.

---

## Entry Conditions

Phase 5 begins after Phase 4 exit criteria are met. The following prior outputs are consumed:

| Prior Output | How Phase 5 Uses It |
| :--- | :--- |
| Keystore with rotation primitives (Phase 1) | Exposed via Settings UI |
| `settings` table and repository (Phase 1) | Completed with all configurable options |
| GPUI shell and navigation (Phase 1) | Extended with Onboarding, Help, and About screens |
| All backend traits and implementations (Phases 1–4) | Backend availability UI reflects compile-time state |
| Evaluation and cover letter UIs (Phase 3) | Polish: empty states, loading states, error surfaces |
| PDF preview and render history (Phase 4) | Polish: render options, error handling |
| Test/CI infrastructure (Phase 1) | Extended with platform-specific and coverage jobs |

**Phase 5 assumes all v1 features are implemented.** If a feature is missing, it is deferred to a future release, not added in Phase 5.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Complete Settings screen (all user-configurable options) | New features of any kind |
| Key rotation UI (passphrase change, full DEK rotation) | File-based data migration (Phase 6) |
| Backend selection & availability UI | Packaging and distribution (Phase 6) |
| Onboarding flow (encryption, backups, API keys) | Localization / internationalization |
| Error handling and panic audit | Telemetry or analytics |
| Accessibility: keyboard navigation, focus management | Full screen reader support (GPUI limitation) |
| Platform-specific testing matrix | Mobile or web builds |
| Performance tuning (startup, DB, UI responsiveness) | Micro-optimizations without measured impact |
| Backup, export, and recovery polish | Cloud sync |
| Documentation: user guide, troubleshooting, README | Marketing material |
| Visual polish: empty states, loading states, error states | Visual redesign |
| Test coverage completion | Test framework changes |

---

## Sequencing Rationale

Phase 5 work is less strictly sequential than prior phases, but there is a natural order:

```
W1: Settings completion + Key Rotation UI + Backend availability UI
    └── W2: Onboarding + Error handling audit + Panic audit
        └── W3: Platform-specific testing + Performance tuning
            └── W4: Visual polish + Documentation + Coverage completion

Parallel: Backup/export polish, accessibility, help content
```

Settings comes first because key rotation, backend selection, and provider configuration all live there. Onboarding comes second because it explains the settings model — writing it before the settings exist would require rework. Platform testing is scheduled once the application is feature-complete, so the tests exercise the real thing rather than a moving target.

---

## Workstream 1 — Complete Settings Screen

**Priority:** First. Everything else references settings.
**Estimated duration:** 1 week

### Deliverables

1. **General settings:**
   - Default page size (A4 / Letter) for PDF rendering
   - Default CV template and cover letter template selection
   - Theme (light / dark / system)
   - Data directory location (read-only display with an "Open in file manager" action)

2. **AI Provider settings:**
   - Provider selection (Anthropic / OpenAI / Gemini / others)
   - Model selection per provider, filtered by the capability registry from Phase 3
   - Capability indicators: structured output, streaming, context window size
   - API key entry with a "Test connection" action
   - API key status display ("Configured" / "Not configured" / "Invalid" — never displays the key itself)
   - Warning when the selected model lacks a required capability for evaluation

3. **Backend settings:**
   - `BrowserBackend` selection (from compiled-in options)
   - `PdfBackend` selection (from compiled-in options)
   - Compile-time availability display: unavailable backends are greyed out with explanatory tooltips
   - Browser binary path override (advanced, for users with system Chrome)

4. **Security settings:**
   - Key rotation (see Workstream 2)
   - Backup guidance with a link to the relevant help section
   - Data export (see Workstream 9)

5. **About screen:**
   - Version number
   - Build information (git commit, build date)
   - Compiled-in features (which backends are available)
   - License and attributions
   - Link to the documentation

### Critical Design Rules

1. **Settings never display secrets.** API keys are shown as "Configured" or "Not configured," never as a masked partial string. There is no "reveal" action.
2. **Compile-time availability is honest.** A backend that was not feature-enabled is not selectable. The tooltip explains what feature would enable it. Do not silently hide it.
3. **Capability warnings are actionable.** When a model lacks structured output support, the warning names the affected feature and suggests an alternative model.
4. **Settings changes are immediate, not save-gated.** Every change applies on modification. No "Save" button for the settings screen. This is a deliberate choice: it removes the "did I save?" ambiguity that plagues settings screens.
5. **Destructive actions require confirmation.** Data export is not destructive. Key rotation, data reset, and backend switching that invalidates cached data are.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Every settings value persists across restart | UI integration test |
| API keys are never displayed in the UI or logs | Log audit + UI inspection |
| Unavailable backends are greyed out with explanation | UI test with feature disabled build |
| Capability warnings appear for unsupported models | UI test with fixture provider data |
| Settings changes apply immediately | UI test |
| Destructive actions require confirmation | UI test |
| About screen displays version and compiled features | UI test |

---

## Workstream 2 — Key Rotation UI

**Priority:** After the Settings screen exists to host it.
**Estimated duration:** 2–3 days

### Deliverables

1. **Change passphrase flow:**
   - Requires current passphrase (verifies the user has access)
   - Prompts for a new passphrase with strength feedback
   - Re-wraps the DEK with a new KEK-passphrase
   - Does not re-encrypt the database (cheap, near-instant)
   - Confirms success with a clear message

2. **Full DEK rotation flow:**
   - Requires current passphrase and explicit confirmation
   - Generates a new DEK
   - Runs `PRAGMA rekey` to atomically re-encrypt the database
   - Re-wraps the new DEK with both KEK paths
   - Displays progress (this is expensive on large databases)
   - Cancellable? **No** — `PRAGMA rekey` is atomic and cannot be safely interrupted. The UI must warn of this before starting.

3. **Backup guidance during rotation.** Before a full DEK rotation, prompt the user to export an encrypted backup. This is the single safety net if rotation fails mid-operation.

4. **Rotation failure handling.** If rotation fails, the database may be in an unrecoverable state. The UI must:
   - Detect the failure
   - Preserve the old `dek.wrapped.json` until the new one is confirmed working
   - Offer to restore from a backup if one exists
   - Never partially update the key file

### Critical Implementation Notes

1. **Passphrase change is the common case.** Full DEK rotation is rare. The UI should make the distinction clear: "Change passphrase" is a lightweight operation; "Rotate encryption key" is a heavyweight, risky operation.
2. **Re-wrap is atomic per wrap.** Each wrap (keychain, passphrase) is written independently. If the passphrase wrap succeeds but the keychain wrap fails, the user has a working passphrase path and a broken keychain path. The UI must reflect this partial state.
3. **`PRAGMA rekey` timing matters.** SQLCipher rewrites the entire database. For a 50MB database, this may take 10–30 seconds. Show progress, not a spinner.
4. **Rotation is not a security event by default.** Changing a passphrase because the user wants a stronger one is a normal operation. Do not frame it as a security incident unless the user indicates one.
5. **Test the failure paths.** A rotation that fails halfway is the worst-case scenario. Test it deliberately by injecting a failure.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Passphrase change succeeds and old passphrase no longer works | Integration test |
| Passphrase change does not re-encrypt the database (fast) | Timing test |
| Full DEK rotation succeeds and new key file is written | Integration test |
| Rotation prompts for a backup before starting | UI test |
| Rotation failure preserves the old key file | Fault injection test |
| Cancellable? Rotation UI warns it is not | UI test |
| Partial wrap state (keychain fails, passphrase succeeds) is handled | Fault injection test |

---

## Workstream 3 — Backend Selection & Availability UI

**Priority:** After the Settings screen exists.
**Estimated duration:** 2–3 days

### Deliverables

1. **Backend selection UI** in Settings:
   - `BrowserBackend` picker showing compiled-in options
   - `PdfBackend` picker showing compiled-in options
   - Unavailable options greyed out with tooltips explaining which Cargo feature enables them
2. **Runtime backend status** on relevant screens:
   - Portals Configuration shows whether Tier 2 is available
   - CV editor preview shows whether a non-browser `PdfBackend` is available
3. **Browser binary configuration:**
   - Path override for system Chrome
   - "Detect Chrome" action that searches common install locations
   - Status display: "Using bundled binary" / "Using system Chrome at {path}" / "Not found"

### Critical Design Rules

1. **Compile-time availability is a first-class UI concept.** Users who download a build without the `browser-backend` feature must understand why certain options are unavailable. The tooltip is the answer, and it must be specific: "This build was compiled without the `browser-backend` feature. Download the full build from {link} to enable it."
2. **Backend switching is immediate.** Changing the `PdfBackend` takes effect on the next render, not after a restart.
3. **No silent fallback.** If a selected backend becomes unavailable (e.g. system Chrome was uninstalled), the UI surfaces the change explicitly. It does not silently switch to a different backend.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Compiled-in backends are selectable | UI test |
| Unavailable backends are greyed out with specific tooltips | UI test with feature disabled build |
| Backend selection persists across restart | Integration test |
| Browser path override is applied | UI test |
| Backend status is displayed on relevant screens | UI test |

---

## Workstream 4 — Onboarding & First-Run Experience

**Priority:** After Settings is complete. Onboarding explains settings.
**Estimated duration:** 1 week

### Deliverables

1. **Multi-step first-run wizard:**
   - **Welcome** — one-paragraph description of what the application does
   - **Encryption explanation** — plain-language explanation of why a passphrase is required, what it protects, and what happens if it is lost
   - **Passphrase creation** — with strength feedback and a confirmation field
   - **Passphrase acknowledgement** — the user types a confirmation that they understand the passphrase cannot be recovered
   - **Backup guidance** — explains where `dek.wrapped.json` lives, and that it should be backed up alongside the encrypted database
   - **AI provider setup** — optional, can be skipped. Provider selection and API key entry with a "Test connection" action
   - **Optional: initial portal setup** — a suggested list of common ATS platforms to add as portals
   - **Completion** — summary of what was configured and where to find help

2. **Returning-user first-launch** (not first-ever): skips onboarding and goes to unlock.

3. **Onboarding progress is persisted** so that a crash or quit mid-onboarding resumes at the correct step.

4. **Skip options** where safe. The AI provider step is skippable. The passphrase step is not.

### Critical Design Rules

1. **The passphrase step cannot be skipped.** It is the only recovery path for the database. The UI must explain why it is required without being alarming.
2. **The acknowledgement step is deliberate friction.** Users must type a phrase confirming they understand that losing both the keychain and the passphrase means permanent data loss. This is the single most important safety communication in the entire application.
3. **Backup guidance is specific.** "Back up `dek.wrapped.json` and `career-ops.db` together" is actionable. "Back up your data" is not.
4. **Onboarding is not a tutorial.** It configures the minimum required for a working application. Feature explanations belong in help content, not the wizard.
5. **The wizard is fully keyboard-navigable.** No mouse required.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| First-ever launch shows the wizard | UI test |
| Wizard completes and reaches Ready state | UI integration test |
| Passphrase step cannot be skipped | UI test |
| Acknowledgement requires typed confirmation | UI test |
| Wizard resumes at the correct step after a crash | Fault injection test |
| AI provider step is skippable | UI test |
| Second launch skips the wizard | UI test |

---

## Workstream 5 — Error Handling & Panic Audit

**Priority:** Parallel with platform testing.
**Estimated duration:** 3–5 days

### Deliverables

1. **Panic audit.** Every `unwrap()`, `expect()`, `panic!()`, `unreachable!()`, and `todo!()` in non-test code is audited. Each is either:
   - Removed and replaced with a typed error
   - Documented as an invariant with a comment explaining why it cannot fail
   - Moved behind a `debug_assert!` where the failure is recoverable in release
2. **CI panic ban.** A CI job that fails if new `unwrap()`/`expect()` are added to designated modules (`core::keystore`, `core::llm`, `core::acquisition`, `core::render`, all UI event handlers). Uses `clippy` lints (`unwrap_used`, `expect_used`) with module-level allows for documented exceptions.
3. **Error taxonomy review.** Every error type is reviewed for:
   - Is it typed (not `Box<dyn Error>`)?
   - Does it carry enough context for a user-facing message?
   - Does it distinguish transient from permanent failures?
   - Does it avoid leaking sensitive data (API keys, DEK material) in `Display` or `Debug`?
4. **User-facing error messages.** Every error path has a corresponding user-visible message. The message:
   - States what failed in plain language
   - Offers a next step if one exists
   - Does not include raw error strings unless the user opts into "Show details"
5. **Global error boundary in GPUI.** Unhandled errors surface as a non-fatal notification, not a crash. The application remains usable.

### Critical Implementation Notes

1. **The keystore is the highest-priority audit target.** A panic in the keystore code path is a data-loss event, not an inconvenience.
2. **`Debug` derive is dangerous on types holding secrets.** Audit every type that contains a `SecretString`, `Zeroizing<[u8; 32]>`, or raw key material. Implement `Debug` manually to redact. This is already a rule from Phase 1; Phase 5 verifies it is applied consistently.
3. **Error messages are not logs.** A user-facing message and a log entry are different artifacts. The message is short and actionable. The log has the full context. Do not conflate them.
4. **The "Show details" pattern is the right default.** Users see a clean message; developers and power users can expand to see the full error. This satisfies both audiences without compromising either.
5. **No `anyhow` in library modules.** `anyhow::Error` erases type information, which makes programmatic error handling impossible. Use `thiserror` for typed errors in library modules. Reserve `anyhow` for binary-level orchestration where the error is terminal.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| CI panic-ban job passes | CI green |
| Every user-facing error path has a message | Manual audit |
| No secret material appears in `Display` or `Debug` output | Automated test + code review |
| Unhandled errors surface as notifications, not crashes | Fault injection test |
| Error taxonomy is documented | Documentation check |

---

## Workstream 6 — Accessibility & Keyboard Navigation

**Priority:** Parallel with error handling.
**Estimated duration:** 3–5 days

### Deliverables

1. **Full keyboard navigation:**
   - Tab order is logical on every screen
   - Focus is visible and never trapped
   - Escape closes modals and cancels in-progress operations
   - Enter activates the primary action
   - Common shortcuts: `Cmd/Ctrl+K` for command palette (if implemented), `Cmd/Ctrl+F` for search (where applicable), `Cmd/Ctrl+,` for settings
2. **Focus management:**
   - Focus moves to the first interactive element when a screen opens
   - Focus returns to the triggering element when a modal closes
   - Focus is trapped within modals (cannot tab out)
3. **Contrast compliance:**
   - Text meets WCAG AA contrast ratios (4.5:1 for body text, 3:1 for large text)
   - Interactive elements have a visible focus indicator with sufficient contrast
4. **Reduced motion support:**
   - Respect the OS reduced-motion preference
   - Disable non-essential animations (transitions, fades) when enabled
5. **Screen reader support — documented limitation:**
   - GPUI's accessibility support is limited as of the current version
   - Document what is and is not supported
   - Do not claim screen reader support if it is not verified

### Critical Implementation Notes

1. **GPUI accessibility is a known gap.** The framework is pre-1.0 and its accessibility story is not mature. Do not attempt to work around this with a custom accessibility layer. Document the limitation honestly and revisit when GPUI adds support.
2. **Keyboard navigation is achievable and should not be sacrificed.** Every action must be reachable without a mouse. This is a baseline, not a nice-to-have.
3. **Focus indicators are not optional.** A visible focus ring is the difference between "keyboard-navigable" and "keyboard-hostile."
4. **Test on real hardware.** Keyboard navigation on Linux behaves differently than macOS than Windows. Test all three.
5. **The onboarding wizard is the highest-priority target.** New users are the least likely to know the shortcuts and the most likely to rely on Tab.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Every action is reachable via keyboard | Manual audit on all three platforms |
| Focus is visible on every interactive element | Visual test |
| Tab order is logical on every screen | Manual audit |
| Modals trap focus and return focus on close | UI test |
| Reduced-motion preference is respected | Manual test |
| Contrast ratios meet WCAG AA | Automated contrast check |
| Accessibility limitations are documented | Documentation check |

---

## Workstream 7 — Platform-Specific Testing & Hardening

**Priority:** After all features are complete.
**Estimated duration:** 1 week

### Deliverables

1. **Platform test matrix:**

| Platform | Keychain | Passphrase | No-Secret-Service |
| :--- | :--- | :--- | :--- |
| macOS (Keychain) | ✅ Required | ✅ Required | N/A |
| Windows (Credential Manager) | ✅ Required | ✅ Required | N/A |
| Linux (GNOME + libsecret) | ✅ Required | ✅ Required | — |
| Linux (headless, no libsecret) | — | ✅ Required | ✅ Required |
| Linux (locked keychain) | ✅ Verified | ✅ Required | — |

2. **CI jobs per platform:**
   - Full test suite on macOS, Windows, Linux
   - Dedicated job: unlock via keychain (all three platforms)
   - Dedicated job: unlock via passphrase (all three platforms)
   - Dedicated job: unlock with no Secret Service on Linux (headless container)
   - Dedicated job: keychain sentinel round-trip (from Phase 1, still enforced)

3. **Platform-specific quirks documented:**
   - macOS: code-signing identity changes may invalidate keychain access
   - Windows: Credential Manager entry naming and collisions
   - Linux: `libsecret` availability varies by distribution and desktop environment

4. **Installation and uninstallation behavior verified:**
   - Fresh install on each platform
   - Upgrade over an existing install
   - Uninstall leaves no orphaned files (or documents what remains)

### Critical Implementation Notes

1. **The Linux no-Secret-Service case is the most fragile.** Headless servers, minimal desktop environments, and containerized setups may lack `libsecret`. The passphrase fallback must work fully in this environment. This is already a CI job from Phase 1; Phase 5 verifies it end-to-end.
2. **The macOS code-signing issue is real.** If the application is re-signed with a different identity, the Keychain may treat it as a different application and deny access. The passphrase fallback handles this — but only if the user set one during onboarding, which is why that step is non-skippable.
3. **Test with a real user profile, not a test profile.** A test profile is often already unlocked and lacks the realistic permission prompts. Use a fresh VM or a dedicated user account.
4. **Verify uninstall completeness.** A cleanup step that misses `dek.wrapped.json` leaves key material on disk. The uninstaller must remove it or clearly document that it is retained.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Full test suite passes on all three platforms | CI green |
| Keychain unlock works on macOS, Windows, GNOME Linux | CI per platform |
| Passphrase unlock works on all platforms | CI per platform |
| No-Secret-Service unlock works on Linux headless | CI job |
| Fresh install, upgrade, and uninstall verified on each platform | Manual + scripted test |
| Platform quirks documented in help content | Documentation check |

---

## Workstream 8 — Performance Tuning

**Priority:** After features are complete, before visual polish.
**Estimated duration:** 3–5 days

### Deliverables

1. **Startup performance targets:**
   - Cold start to unlock screen: **< 1 second**
   - Unlock to Ready state (warm keychain): **< 500 ms**
   - Unlock to Ready state (passphrase + Argon2id): **< 1 second** (Argon2id is deliberately slow; the target is 250–500 ms for the KDF itself)
2. **Database performance:**
   - Pipeline table with 1,000 applications: smooth scroll, no lag
   - Job description fetch and store: < 200 ms for a typical ATS response
   - Evaluation report query for a single application: < 50 ms
3. **UI responsiveness:**
   - Streaming evaluation text appears without visible lag
   - PDF render does not block the UI
   - Browser fetch does not block the UI
4. **Memory profile:**
   - Steady-state memory usage documented (target: < 200 MB without browser backend)
   - Memory usage with browser backend documented separately
5. **A short performance baseline document** recording the measured numbers with the hardware and build configuration.

### Critical Implementation Notes

1. **Measure, don't guess.** Every target is verified against a measurement, not an assumption. Where a target is missed, the cause is documented, not hand-waved.
2. **Argon2id tuning is a security/UX tradeoff.** The KDF must be slow enough to resist brute force and fast enough not to frustrate the user. 250–500 ms on target hardware is the standard range. Do not tune for speed at the expense of security.
3. **Browser operations are the dominant runtime cost.** This is expected and acceptable. The point of the performance work is not to make browser operations fast — it is to ensure they do not block the UI.
4. **The pipeline table is the most likely performance bottleneck.** GPUI's virtualized rows help, but poor query design can still cause lag. Profile the query, not just the rendering.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Cold start to unlock meets the target | Benchmark |
| Unlock to Ready meets the target | Benchmark |
| Pipeline table with 1,000 applications is smooth | Manual test |
| Evaluation streaming has no visible lag | Manual test |
| PDF render does not block the UI | UI responsiveness test |
| Performance baseline document exists | Documentation check |

---

## Workstream 9 — Backup, Export & Recovery Polish

**Priority:** Parallel with platform testing.
**Estimated duration:** 3–5 days

### Deliverables

1. **Encrypted export bundle:**
   - Exports `career-ops.db` and `dek.wrapped.json` together as a zip or directory
   - Optionally includes a manifest with version, timestamp, and record counts
   - Does not export the keychain entry (that is machine-specific)
   - Does not export API keys

2. **Import flow:**
   - Restores from an encrypted export bundle
   - Requires the passphrase that was set on the source machine
   - Validates the bundle before overwriting any existing data
   - Clear warning if an existing database will be replaced

3. **Backup reminders (opt-in):**
   - Optional reminder if no backup export has been performed in N days
   - Dismissable and configurable
   - Default off (do not nag)

4. **Data directory actions:**
   - "Open data directory" from Settings
   - "Open backup directory" from Settings (if a separate location is configured)

5. **Help content:**
   - A "Backup and recovery" help page explaining:
     - What `dek.wrapped.json` is
     - Why backing up only the database is insufficient
     - What happens if the passphrase is lost
     - How to restore from a backup bundle

### Critical Implementation Notes

1. **The export bundle is the user's insurance policy.** It must be easy to create, easy to verify, and easy to restore. Any friction here translates directly to a user who does not back up and later loses data.
2. **The bundle must be self-contained.** Restoring on a new machine requires only the bundle and the passphrase. The keychain entry does not transfer (and should not).
3. **Do not encrypt the bundle separately.** The bundle contains an encrypted database and a wrapped key file. Adding another encryption layer adds complexity without meaningful benefit.
4. **Validate before overwrite.** Import must check that the bundle is well-formed and that the passphrase works before touching the existing database. A failed import must leave the existing data untouched.
5. **Backup reminders default off.** Security features that nag get disabled. A reminder is a help, not a mandate.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Export produces a bundle containing the DB and key file | Integration test |
| Import restores the database on a fresh install | Integration test |
| Import validates before overwriting | Fault injection test |
| Import requires the correct passphrase | Integration test |
| Backup reminder is off by default | UI test |
| Help content explains the backup model | Documentation check |

---

## Workstream 10 — Documentation & Help Content

**Priority:** Throughout the phase, finalized in W4.
**Estimated duration:** 3–5 days

### Deliverables

1. **User guide** covering:
   - Installation and first-run
   - Adding portals and fetching jobs
   - Evaluating applications
   - Generating cover letters
   - Rendering PDFs
   - Backing up and restoring data
   - Troubleshooting common issues

2. **In-app help content:**
   - "Backup and recovery" page (referenced from Workstream 9)
   - "Understanding encryption" page (referenced from onboarding)
   - "Adding portals" page (referenced from the Portals Configuration screen)
   - "Configuring AI providers" page (referenced from Settings)
   - Contextual help links from relevant screens

3. **Troubleshooting guide:**
   - "The application will not unlock" — passphrase and keychain issues
   - "Job fetching fails" — tier routing and JS-required pages
   - "PDF rendering produces wrong output" — extraction-order issues
   - "Evaluation fails" — API key and provider issues

4. **README** for the repository:
   - What the application is
   - Build instructions (including feature flags)
   - Platform-specific build notes
   - Contribution guidelines

5. **Build and feature documentation:**
   - Which Cargo features exist and what they enable
   - How to build the default (lean) and full (browser-enabled) variants
   - Which features are recommended for which use cases

### Critical Implementation Notes

1. **Documentation is a deliverable, not an afterthought.** Phase 5 does not exit without it.
2. **In-app help is preferred over external documentation.** Users do not read READMEs. They read contextual help when they get stuck.
3. **The troubleshooting guide should be searchable.** A simple text search across help content is more valuable than a perfect index.
4. **Do not document features that do not exist.** If a feature is deferred, do not describe it as "coming soon." Omit it entirely.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| User guide covers all major workflows | Documentation review |
| In-app help pages exist and are linked from relevant screens | UI test |
| Troubleshooting guide covers the four listed scenarios | Documentation review |
| README explains feature flags and build variants | Documentation review |
| Documentation is accurate as of the release | Manual review |

---

## Workstream 11 — Visual Polish & Empty States

**Priority:** Final week.
**Estimated duration:** 3–5 days

### Deliverables

1. **Empty states for every screen:**
   - Dashboard with no applications: "No applications yet. Add a portal to get started."
   - Pipeline with no jobs: "No jobs match the current filter."
   - Portals with no configuration: "No portals configured. Add one to begin fetching jobs."
   - Prompts with no user prompts: "System prompts are available. Duplicate one to customize it."
2. **Loading states:**
   - Skeleton screens or spinners for async operations
   - Progress indicators for batch operations
   - Streaming indicators for evaluation
3. **Error states:**
   - Inline error messages for form validation
   - Non-blocking notifications for transient failures
   - Full-screen error states for unrecoverable conditions (with recovery actions)
4. **Transitions and animations:**
   - Minimal, purposeful animations
   - Respect reduced-motion preferences
   - No gratuitous animation
5. **Visual consistency pass:**
   - Consistent spacing, typography, and color usage
   - Consistent button and input styles
   - Consistent iconography

### Critical Implementation Notes

1. **Empty states are onboarding opportunities.** A user seeing "No applications yet" for the first time should know exactly what to do next. The empty state is not a placeholder — it is a call to action.
2. **Loading states prevent anxiety.** A spinner for an operation that takes 200ms is visual noise. A spinner for an operation that takes 20 seconds is essential. Match the indicator to the operation duration.
3. **Error states are the most under-designed part of most applications.** Every error path should have a design, not just an implementation. The user should never see a raw error or a blank screen.
4. **Polish is not redesign.** Do not change the visual language in Phase 5. Do refine it.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Every screen has an empty state | UI test |
| Async operations have appropriate loading states | Manual review |
| Every error path has a designed error state | Manual review |
| Reduced-motion preference is respected | Manual test |
| Visual consistency across screens | Manual review |

---

## Test Strategy

### Coverage Completion

Phase 5 closes coverage gaps from prior phases. The goal is not 100% line coverage — that is a vanity metric. The goal is that every **user-facing workflow** has an integration test and every **safety-critical code path** has a fault-injection test.

| Category | Target |
| :--- | :--- |
| Keystore safety paths | 100% (from Phase 1) |
| Acquisition routing and classification | 95% |
| Evaluation pipeline | 90% (excluding LLM-specific paths covered by cassettes) |
| PDF rendering and ATS verification | 95% |
| UI workflows | Every screen has at least one integration test |
| Settings persistence | Every setting has a round-trip test |

### New Test Categories in Phase 5

| Test type | Purpose |
| :--- | :--- |
| **Fault injection** | Deliberately break rotation, import, and unlock to verify recovery |
| **Platform matrix** | Same tests on macOS, Windows, Linux with/without keychain |
| **Keyboard navigation** | Every workflow reachable without a mouse |
| **Error path** | Every classified error produces a user-visible message |
| **Panic audit** | No `unwrap()` in designated modules |
| **Reduced motion** | Animations are disabled when the preference is set |

### CI Jobs Added in Phase 5

| Job | Purpose |
| :--- | :--- |
| `platform-matrix` | Full test suite on macOS, Windows, Linux |
| `no-secret-service` | Unlock via passphrase on headless Linux |
| `panic-ban` | Clippy lints for `unwrap_used`, `expect_used` in designated modules |
| `contrast-check` | Automated WCAG AA contrast verification |
| `keyboard-nav` | Automated keyboard-only workflow tests |

---

## Phase 5 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P5-R1 | **Scope creep into feature work under the guise of polish.** | High | Phase 5 is explicitly not a feature phase. Every addition requires an explicit decision to defer Phase 6 or extend the timeline. |
| P5-R2 | **GPUI accessibility support is limited.** Screen reader support cannot be delivered. | Medium | Document the limitation honestly. Keyboard navigation and contrast compliance are achievable and required. |
| P5-R3 | **Platform-specific quirks surface late.** macOS code-signing, Windows Credential Manager naming, Linux libsecret variability. | Medium | Test on real hardware from the start of W3, not at the end. Document each quirk as it is found. |
| P5-R4 | **Key rotation failure causes data loss.** `PRAGMA rekey` is not interruptible. | High | Prompt for a backup before rotation. Preserve the old key file until the new one is verified. Fault-inject the failure path. |
| P5-R5 | **Onboarding is skipped or rushed.** Users do not complete the passphrase step. | High | Non-skippable passphrase step. Explicit acknowledgement of data-loss risk. This is the single most important safety communication in the app. |
| P5-R6 | **Performance tuning expands indefinitely.** | Medium | Set explicit targets. Measure. Stop when targets are met. Do not chase micro-optimizations. |
| P5-R7 | **Documentation lags behind implementation.** | Medium | Documentation is a Phase 5 deliverable, not a Phase 6 afterthought. In-app help is written alongside the corresponding feature work. |
| P5-R8 | **Export/import introduces a new data-loss vector.** A buggy import could destroy existing data. | High | Validate before overwrite. Fault-inject import failures. Never partially write. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | W1 complete Settings screen, W2 key rotation UI, W3 backend availability UI | Settings is complete and testable; rotation works end-to-end |
| **2** | W4 onboarding wizard, W5 error handling audit, panic audit, secret-redaction audit | First-run experience works; every error path has a message |
| **3** | W7 platform-specific testing on all three platforms, W8 performance tuning, W6 accessibility | Platform matrix is green; performance targets are measured |
| **4** | W9 backup/export polish, W10 documentation, W11 visual polish; coverage completion | Release candidate ready for Phase 6 packaging |

**Total:** 3–4 weeks. The lower end (3 weeks) assumes platform testing reveals no surprises. The upper end (4 weeks) accounts for platform-specific issues that surface late.

---

## Exit Criteria

Phase 5 is complete when **all** of the following are true:

1. The Settings screen exposes every user-configurable option and every compile-time capability.
2. Key rotation (passphrase change and full DEK rotation) works end-to-end with fault-injected failure testing.
3. Backend selection UI reflects compile-time availability accurately, with explanatory tooltips for unavailable backends.
4. The onboarding wizard is complete, non-skippable for the passphrase step, and keyboard-navigable.
5. Every user-facing error path has a designed message and, where applicable, an actionable next step.
6. The panic audit is complete, and the CI panic-ban job is green.
7. No secret material appears in `Display`, `Debug`, or log output.
8. Keyboard navigation reaches every action on all three platforms.
9. Contrast ratios meet WCAG AA. Reduced-motion preference is respected.
10. The platform test matrix is green on macOS, Windows, Linux (with and without libsecret).
11. Startup, unlock, DB, and UI performance targets are measured and met (or the gap is documented).
12. Encrypted export and import work end-to-end, with validation before overwrite.
13. User guide, in-app help, troubleshooting guide, and README exist and are accurate.
14. Every screen has a designed empty state.
15. A short Phase 5 retrospective memo exists, recording platform-specific findings and any targets not met.

---

## What Phase 5 Is Not

Phase 5 hardens and polishes. It does not add features, and it does not package the application. Specifically:

- There is no new feature work. If a feature is not complete by Phase 4, it is deferred to a future release.
- There is no file-based data migration. That is Phase 6.
- There is no packaging or distribution. That is Phase 6.
- There is no localization or internationalization.
- There is no telemetry or analytics.
- There is no full screen reader support (GPUI limitation).
- There is no visual redesign — only refinement.
- There is no micro-optimization without a measured target.

The temptation to add "just one small feature" during polish is the most common way Phase 5 overruns. Every addition is a subtraction from hardening. Resist it.

---

The single most important thing to take from this plan: **the onboarding wizard is a safety feature, not a UX feature.** The passphrase acknowledgement step is the only thing standing between a user and permanent data loss if their keychain entry becomes unavailable. It is deliberately friction-full, and it should stay that way. Every other polish item in Phase 5 is in service of a user who has successfully reached the Ready state. The onboarding wizard is what gets them there without a trapdoor underneath.