# Phase 6: Migration & Release — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.4), §12
**Status:** Draft
**Last updated:** 2026-10-12
**Duration:** 2–4 weeks
**Depends on:** Phase 5 completion (see Entry Conditions)

---

## Purpose

Phase 6 ships the application. It delivers three things: a **migration path** for users of the original file-based `career-ops`, **packaged installers** for macOS, Windows, and Linux, and a **release process** that produces signed, verifiable artifacts.

The phase is deliberately last. Migration depends on a stable schema (frozen since Phase 1, but not finalized until Phase 5 stopped changing things). Packaging depends on a feature-complete binary. Release documentation depends on finalized behavior. Attempting any of these earlier means redoing them.

Phase 6 is **not** the phase to add features or fix design issues. If a feature is missing or a design is wrong, that is a signal to defer the release, not to squeeze the work into this phase. The exit criteria are what define "shipped."

---

## Entry Conditions

Phase 6 begins after Phase 5 exit criteria are met. The following prior outputs are consumed:

| Prior Output | How Phase 6 Uses It |
| :--- | :--- |
| Frozen schema migrations (Phase 1) | Destination for migration imports |
| Prompt seeding from embedded resources (Phase 3) | Migrated prompts follow the same model |
| Template seeding (Phase 4) | Migrated templates (if any) follow the same model |
| Complete Settings and export/import (Phase 5) | Export bundle format is reused for the migration snapshot |
| Feature-flag build matrix (Phase 1–5) | Defines the artifact variants to produce |
| Documentation draft (Phase 5) | Finalized and supplemented with install and upgrade guides |
| Test suite green on all three platforms (Phase 5) | Release candidate testing baseline |

**Phase 6 assumes feature freeze.** Between the start of Phase 6 and the release, no new features are added, no schema changes are made, and no new dependencies are introduced unless a critical security issue demands it.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Migration tool: read original files, populate SQLite | Runtime migration (migration is a one-shot tool, not a live sync) |
| Migration preview and confirmation UI | Bidirectional sync between files and DB |
| Migration provenance recording | Migrating from other job-tracking tools |
| Packaged installers for macOS, Windows, Linux | Mobile or web builds |
| Code signing and notarization (where applicable) | App Store / Microsoft Store distribution |
| Feature-variant builds (lean / full) | Dynamic backend loading |
| Auto-update check (notify-only) | Full auto-update with silent install |
| Release process documentation | Continuous deployment automation |
| Install, upgrade, and uninstall guides | Localization of installers |
| Release candidate testing matrix | Long-term support commitments |
| v1.0 release artifacts | v2 feature planning |

---

## Sequencing Rationale

The build order is driven by the release pipeline:

```
W1: Migration tool (parsing, preview, import)
    └── W2: Packaging infrastructure (build scripts, code signing)
        └── W3: Release candidate testing, upgrade/install testing
            └── W4: Release process, documentation finalization, v1.0 tag

Parallel: Auto-update check infrastructure
```

Migration comes first because it is the highest-risk technical work in the phase. Parsing freeform markdown into structured data has many failure modes, and finding those failure modes early is cheaper than finding them after packaging.

Packaging comes second because it is a known quantity but has long lead times for signing certificates and notarization approvals. Start the certificate acquisition process **before** Phase 6 begins — Apple Developer Program enrollment and Windows code-signing certificate issuance can take days to weeks.

---

## Workstream 1 — Migration Tool

**Priority:** First. Highest technical risk in the phase.
**Estimated duration:** 1–1.5 weeks

### Deliverables

1. **A standalone migration binary** (`career-ops-migrate`) that:
   - Reads an original `career-ops` directory
   - Parses all supported file types
   - Populates a fresh encrypted database
   - Produces a migration report

   The migration is a **separate binary** rather than a mode of the main application. Rationale: the main application cannot open a database without a keystore, and the migration tool needs to create a keystore for a fresh database. Keeping them separate avoids coupling migration logic to the main app's unlock flow, and makes the migration tool testable in isolation.

2. **Supported input files:**

| Source file | Destination table | Parsing notes |
| :--- | :--- | :--- |
| `config/profile.yml` | `profile` | YAML → typed struct → insert |
| `cv.md` | `cv_entries` | Markdown → heuristic section detection. Lossy; document limitations. |
| `data/applications.md` | `applications`, `job_descriptions` | Markdown table or list. Detect format by inspection. |
| `data/pipeline.md` | `applications` (status) | Merge with applications if both exist. |
| `portals.yml` | `portals` | YAML → typed struct. Infer `render_mode` from prior behavior. |
| `modes/*.md` | `prompts` | System prompts; mark `is_system = true` if content matches a known seed. |
| `jds/*.md` | `job_descriptions` | One file per job. Extract title/company from filename or content. |
| `reports/*.md` | `evaluation_reports` | Link to applications via filename convention or content match. |
| `output/cover_*.md` | `cover_letters` | Link to applications via filename convention or content match. |

3. **Migration preview.** Before writing to the database, the tool prints or displays:
   - Record counts per table
   - Warnings for ambiguous or unparseable entries
   - A summary of relationships that could not be resolved
   - A preview of the first few records in each table

   Preview is available in two modes: CLI text output (for scripted use) and a GUI flow within the main application (for typical users, gated behind a command-line flag or a hidden menu).

4. **Migration report.** After migration, write a `migration-report.md` alongside the new database recording:
   - Source directory and timestamp
   - Record counts per table
   - Warnings and skipped entries
   - Provenance markers (see below)

5. **Provenance.** Migrated records are marked so their origin is traceable:
   - `evaluation_reports.prompt_version` is set to `NULL` and `prompt_hash` to `"migrated:<sha256-of-file>"` since the original tool did not version prompts
   - `document_renders` is not populated (no PDFs are migrated by default)
   - A `settings` row records `migrated_from = "<source-dir>"` and `migrated_at = "<timestamp>"`

6. **Non-destructive by default.** The migration tool never modifies the source directory. It creates a new database in a specified location. The user chooses whether to adopt it as their primary database.

7. **Idempotency.** Running the migration twice against the same source produces the same database. Running it against a modified source produces a new database, not a merged one. Merge is explicitly out of scope.

### Critical Implementation Notes

1. **Markdown CV parsing is inherently lossy.** The original `cv.md` is freeform. There is no schema. Heuristics will fail on some inputs. The migration report must clearly flag low-confidence parses, and the user should be prompted to review the imported `cv_entries` in the editor. Do not pretend the import is perfect.

2. **Filename conventions are the primary relationship signal.** `reports/*.md` and `output/cover_*.md` link to applications via filename patterns (company name, job ID, or date). If the original tool used a consistent convention, the migration can follow it. If not, the migration links what it can and leaves the rest unlinked with a warning.

3. **Do not migrate `modes/*.md` as user prompts if they match a system seed.** Compare content hashes. Matching files are recorded as system prompts (already seeded). Non-matching files are imported as user prompts with `is_system = false`.

4. **The migration tool uses the same keystore code as the main application.** Do not reimplement key management. The migration creates a fresh database with a fresh DEK, using the same `core::keystore` module. This ensures the migration tool and main app are compatible.

5. **Warn loudly about what is not migrated.** The original tool may have artifacts the new application does not support: HTML templates, custom scripts, workflow configurations, or per-mode configurations that do not map cleanly. Enumerate these explicitly in the report. Users should not discover after migration that something is missing.

6. **Sample migration fixture.** Create a synthetic original-`career-ops` directory for testing that exercises every supported file type and several edge cases (empty files, malformed YAML, ambiguous reports). This is the primary migration test fixture.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| All supported input files are parsed | Integration test against fixture |
| Record counts match source file contents | Integration test |
| Relationships between reports/cover letters and applications are resolved where possible | Integration test |
| Unresolvable relationships produce warnings, not errors | Integration test |
| Migration is idempotent | Integration test (run twice, compare) |
| Source directory is unmodified after migration | Filesystem diff test |
| Migration report is generated with provenance markers | Integration test |
| Malformed input produces a classified warning, not a crash | Fault injection test |
| Empty source directory produces an empty database with a clear report | Edge case test |

---

## Workstream 2 — Packaging Infrastructure

**Priority:** Second. Long lead times for signing.
**Estimated duration:** 1–1.5 weeks

### Deliverables

1. **Build script** (`xtask` or `just`) that produces release artifacts for all supported platforms and feature variants.

2. **Feature-variant matrix:**

| Variant | Cargo features | Artifact suffix | Use case |
| :--- | :--- | :--- | :--- |
| **Lean** (default) | (none) | `-lean` | Users who only need ATS API acquisition and no PDF rendering |
| **Full** | `browser-backend` | `-full` | Users who need JS-heavy site scraping and/or PDF rendering |

   The lean build has no browser binary, ships at ~25 MB, and cannot render PDFs. The full build bundles a browser, ships at ~150 MB, and supports both JS-heavy acquisition and PDF rendering. Both are officially supported.

3. **Platform-specific packaging:**

| Platform | Format | Tooling |
| :--- | :--- | :--- |
| macOS | `.dmg` containing `.app` bundle | `cargo-bundle` or custom bundling; `codesign`; `xcrun notarytool` |
| Windows | `.msi` (WiX) or `.exe` (NSIS) | `cargo-wix` or `cargo-msi`; `signtool` |
| Linux | `.AppImage` and `.deb` | `linuxdeploy` / `appimagetool`; `cargo-deb` or `nfpm` |

   If `cargo-bundle` or equivalent does not support GPUI's runtime requirements (fonts, GPU access, platform libraries), a custom bundling step may be required. Verify this during the first day of this workstream.

4. **Code signing:**
   - **macOS:** Apple Developer ID certificate, hardened runtime, `codesign`, then `xcrun notarytool submit --wait`. The signed and notarized `.dmg` will pass Gatekeeper on first launch.
   - **Windows:** Authenticode signing with an EV or OV certificate via `signtool`. Unsigned builds will trigger SmartScreen warnings; document this for users if signing is not obtained.
   - **Linux:** No OS-level signing. Provide SHA-256 checksums and GPG-signed checksums for verification.

5. **Artifact naming convention:**
   ```
   career-ops-{version}-{variant}-{os}-{arch}.{ext}
   # e.g. career-ops-1.0.0-full-macos-aarch64.dmg
   ```

6. **Reproducible builds.** Document the toolchain versions (`rustc`, `cargo`, target-specific tools) and produce a `BUILD.md` recording how each artifact was built. Reproducibility is aspirational, not guaranteed, but the record matters for security.

### Critical Implementation Notes

1. **Signing certificates have lead times.** Apple Developer Program enrollment takes up to 48 hours. Windows EV certificates can take weeks to issue. Begin certificate acquisition **before** Phase 6 begins. If this has not been started, flag it immediately and proceed with unsigned builds while certificates are pending.

2. **macOS notarization requires a hardened runtime.** GPUI applications may need entitlements for GPU access, file access, and network. Test notarization early — a rejection late in the phase is a schedule risk.

3. **The browser binary bundling needs a decision.** For the `full` variant, the browser binary must be bundled inside the `.app` / `.msi` / `.deb`, or downloaded on first use. Bundling is simpler but grows the artifact. Downloading on first use requires a network call and a trust story for the download source. **Recommendation:** bundle by default, with an option in Settings to point at a system browser. Document both paths.

4. **GPUI packaging is less trodden than Tauri or Electron.** Zed ships as a desktop application, so packaging is possible, but the tooling may be less mature. Verify the full pipeline for one platform (macOS is the best-documented) before committing to all three. If a platform proves intractable, defer it explicitly and document the deferral.

5. **Feature variants mean two builds per platform.** Combined with architecture variants (macOS aarch64/x86_64, Linux x86_64/aarch64), this multiplies quickly. Start with the minimum viable matrix: macOS aarch64, Windows x86_64, Linux x86_64 — one architecture per platform, two variants each, six artifacts total. Add architectures later if demand exists.

6. **CI must produce all artifacts from a single tag.** Manual packaging is a source of errors. Automate the release build so that pushing a git tag triggers a CI workflow that produces all artifacts, computes checksums, and drafts a release.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Lean and full builds produce artifacts on macOS | Manual + CI |
| Lean and full builds produce artifacts on Windows | Manual + CI |
| Lean and full builds produce artifacts on Linux | Manual + CI |
| macOS `.app` launches from a `.dmg` on a clean system | Manual test |
| Windows installer installs and uninstalls cleanly | Manual test |
| Linux `.AppImage` runs on a clean system | Manual test |
| Linux `.deb` installs and removes cleanly | Manual test |
| macOS build passes notarization | `spctl --assess` passes |
| Windows build is signed (if certificate available) | `signtool verify` passes |
| SHA-256 checksums are published with each artifact | Release artifacts |
| CI produces all artifacts from a git tag | CI green |

---

## Workstream 3 — Release Candidate Testing

**Priority:** After packaging works. Before release.
**Estimated duration:** 3–5 days

### Deliverables

1. **Install, upgrade, and uninstall testing on each platform:**

| Test | macOS | Windows | Linux |
| :--- | :--- | :--- | :--- |
| Fresh install | Required | Required | Required (.deb + AppImage) |
| First launch from a clean user profile | Required | Required | Required |
| Onboarding completes | Required | Required | Required |
| Data persists across restart | Required | Required | Required |
| Upgrade over a prior install | Required | Required | Required |
| Settings preserved across upgrade | Required | Required | Required |
| Uninstall removes the application | Required | Required | Required |
| Uninstall leaves user data (by design) | Required | Required | Required |
| Uninstall removes `dek.wrapped.json` on request | Required | Required | Required |

2. **Clean-machine testing.** Test on a machine (or VM) that has never had the application installed, never had a browser installed by the application, and has no prior Rust toolchain. This surfaces missing runtime dependencies.

3. **Antivirus / SmartScreen behavior.** Note what happens on first launch on Windows if the build is unsigned or newly signed. Document the expected user experience.

4. **Gatekeeper behavior on macOS.** Verify that the notarized `.dmg` opens without the "unidentified developer" warning.

5. **Full workflow test on a clean install:**
   - Complete onboarding with a fresh passphrase
   - Add a portal, fetch jobs
   - Evaluate an application
   - Generate a cover letter
   - Render a PDF
   - Export a backup bundle
   - Restore from the bundle on a second machine

6. **Migration test on a clean install:**
   - Run `career-ops-migrate` against a real (or fixture) original directory
   - Verify imported data appears correctly
   - Verify the migration report matches expectations
   - Verify the source directory is unmodified

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| All install/upgrade/uninstall tests pass | Manual test per platform |
| Clean-machine install succeeds on all three platforms | Manual test |
| Full workflow completes on a clean install | Manual test |
| Migration completes and produces expected data | Manual test |
| No unexpected network calls during onboarding | Network capture |
| No unexpected files written outside the data directory | Filesystem inspection |

---

## Workstream 4 — Auto-Update Check

**Priority:** Parallel with RC testing.
**Estimated duration:** 2–3 days

### Deliverables

1. **Update check on startup** (opt-in, default on):
   - Query a static JSON manifest hosted at a known URL
   - Compare the current version against the latest released version
   - If a newer version exists, show a non-blocking notification with release notes link
2. **Update manifest format:**
   ```json
   {
     "latest_version": "1.0.1",
     "release_date": "2026-11-01",
     "release_notes_url": "https://...",
     "downloads": {
       "macos-aarch64": "https://...",
       "windows-x86_64": "https://...",
       "linux-x86_64": "https://..."
     }
   }
   ```
3. **Privacy:** The update check sends no identifying information. It is a plain GET for a public file. Document this in the privacy section of the user guide.
4. **Opt-out** in Settings. When disabled, no network request is made.
5. **Signature verification of the manifest.** Sign the manifest with a GPG key whose public key is embedded in the application. A tampered manifest (pointing to a malicious download) is rejected.

### Critical Implementation Notes

1. **Notify-only, not auto-install.** Full auto-update requires a code-signed installer downloaded and executed by the application, which introduces a significant attack surface. Notify-and-download is the right trade-off for v1.
2. **The manifest is a single point of failure.** If the hosting URL goes away, the update check fails silently. The application must not surface update-check failures as errors. They are a background convenience.
3. **Sign the manifest.** An unsigned manifest can be replaced by an attacker to redirect users to a malicious binary. A GPG-signed manifest with an embedded public key prevents this.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Update check retrieves and parses the manifest | Integration test |
| Notification appears when a newer version is available | UI test |
| No notification when up to date | UI test |
| Opt-out prevents any network request | Network capture test |
| Tampered manifest is rejected | Signature verification test |
| Update-check failure does not surface as an error | UI test with a failed request |

---

## Workstream 5 — Release Process

**Priority:** Final week.
**Estimated duration:** 2–3 days

### Deliverables

1. **Versioning.** Semantic versioning: `MAJOR.MINOR.PATCH`. v1.0.0 is the initial release. Patch releases for bug fixes, minor releases for backwards-compatible additions, major releases for breaking changes.

2. **Git tag convention.** `v1.0.0`, `v1.0.1`, `v1.1.0`. Tags trigger the release CI workflow.

3. **Release artifacts per tag:**
   - Six primary artifacts (2 variants × 3 platforms)
   - `SHA256SUMS` file listing all artifact hashes
   - `SHA256SUMS.asc` GPG signature of the checksums file
   - Release notes (`CHANGELOG.md` entry)

4. **Release CI workflow:**
   - Triggered by pushing a `v*` tag
   - Builds all artifacts on the appropriate runners
   - Runs the full test suite on each platform
   - Signs and notarizes macOS builds
   - Signs Windows builds (if certificate available)
   - Computes checksums and signs them
   - Creates a draft GitHub release with all artifacts attached
   - Waits for manual approval before publishing

5. **GitHub release structure:**
   - Release title: `v1.0.0`
   - Body: release notes from `CHANGELOG.md`
   - Assets: all artifacts + `SHA256SUMS` + `SHA256SUMS.asc` + `BUILD.md` + migration guide
   - Marked as latest if it is the newest non-prerelease

6. **Release checklist** (a `RELEASE.md` documenting the process):
   - [ ] All tests green on all platforms
   - [ ] Phase 6 exit criteria met
   - [ ] `CHANGELOG.md` updated
   - [ ] Version bumped in `Cargo.toml` and any other manifests
   - [ ] Signing certificates valid
   - [ ] Update manifest prepared for the previous release
   - [ ] Documentation updated for any user-facing changes
   - [ ] Draft release reviewed by a second person
   - [ ] Tag pushed, CI completed, artifacts verified
   - [ ] Draft release published
   - [ ] Update manifest deployed

7. **Post-release:**
   - Deploy the update manifest so existing users see the new version
   - Monitor for crash reports (if telemetry is enabled — currently out of scope, so skip)
   - Triage issues reported in the first week

### Critical Implementation Notes

1. **Tag-push-triggers-everything.** Manual release processes drift and produce inconsistent artifacts. Everything is triggered by the tag.

2. **Draft releases, not auto-published.** A human reviews the draft before publishing. This is a safety check, not a bureaucratic step.

3. **The update manifest is deployed separately.** It is not part of the GitHub release. Deploying it is a deliberate action that announces the release to existing users.

4. **Sign the checksums.** The `SHA256SUMS` file must be signed. Without it, a user who downloads the artifacts from an alternative source has no way to verify authenticity.

5. **Document the process for future releases.** The release checklist is not just for v1.0.0. It is the process for every release. Write it once, follow it every time.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Tag push triggers the release CI | CI workflow verified |
| All artifacts are produced from a single tag | CI output |
| macOS artifacts are notarized | `spctl --assess` on the released artifact |
| Checksums are correct and signed | Manual verification |
| Draft release contains all artifacts | GitHub release inspection |
| Update manifest is deployed post-release | Manifest URL responds correctly |
| Release checklist is documented | `RELEASE.md` exists |

---

## Workstream 6 — Documentation Finalization

**Priority:** Throughout, finalized in W4.
**Estimated duration:** 3–5 days

### Deliverables

1. **Install guide** per platform:
   - macOS: `.dmg` open, drag-to-Applications, first-launch flow
   - Windows: `.msi` or `.exe` run, SmartScreen behavior, installation directory
   - Linux: `.AppImage` permissions, `.deb` install, data directory location

2. **Upgrade guide:**
   - How upgrades work per platform
   - What is preserved (database, keystore, settings)
   - How to roll back if needed

3. **Uninstall guide:**
   - How to uninstall per platform
   - What is removed and what is retained
   - How to remove `dek.wrapped.json` if the user wants a clean removal

4. **Migration guide:**
   - What the migration tool imports and what it does not
   - Known limitations of markdown CV parsing
   - How to run the migration tool
   - How to review the imported data
   - How to keep the source directory as a backup

5. **Privacy and security page:**
   - What data is stored locally (everything except API calls)
   - What data is sent to LLM providers
   - What the update check sends (nothing identifying)
   - The encryption model in plain language
   - The backup model and the consequences of key loss

6. **FAQ:**
   - "Can I use this without an AI provider?"
   - "What happens if I forget my passphrase?"
   - "Can I use a local LLM?"
   - "Can I use this offline?"
   - "Where is my data stored?"
   - "How do I contribute?"

7. **Updated README:**
   - Links to the install guide, migration guide, and help content
   - Badges: build status, latest release, license
   - Feature list with the lean/full distinction
   - Screenshot of the main screen

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Install guide covers all three platforms | Documentation review |
| Upgrade and uninstall guides exist | Documentation review |
| Migration guide documents limitations honestly | Documentation review |
| Privacy page accurately describes data flows | Documentation review |
| FAQ answers the six listed questions | Documentation review |
| README links to all relevant docs | Documentation review |

---

## Test Strategy

### New Test Categories in Phase 6

| Test type | Purpose |
| :--- | :--- |
| **Migration fixture test** | Full migration from a synthetic original directory |
| **Migration edge cases** | Empty files, malformed YAML, ambiguous relationships |
| **Install/uninstall test** | Clean install, upgrade, uninstall on each platform |
| **Clean-machine test** | Fresh VM, no prior toolchain |
| **Update check test** | Manifest fetch, version compare, signature verification |
| **Artifact verification** | Checksums, signatures, notarization |

### CI Jobs Added in Phase 6

| Job | Purpose |
| :--- | :--- |
| `migration-fixture` | Full migration test against the fixture directory |
| `release-build` | Triggered by tag; builds all artifacts |
| `artifact-verify` | Post-build verification of checksums and signatures |
| `install-test-macos` | Install and uninstall on a clean macOS VM |
| `install-test-windows` | Install and uninstall on a clean Windows VM |
| `install-test-linux` | Install and uninstall on a clean Linux container |

---

## Phase 6 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P6-R1 | **Signing certificates not obtained in time.** Apple and Windows certificate issuance can take days to weeks. | High | Begin acquisition **before** Phase 6 starts. Ship unsigned builds with documented warnings if certificates are pending. Do not delay the release for signing alone. |
| P6-R2 | **macOS notarization fails.** GPUI may require entitlements that are not immediately obvious. | High | Test notarization early in W2. Iterate on entitlements. If notarization is blocked, ship as an unsigned build with a "right-click to open" instruction. |
| P6-R3 | **GPUI packaging tooling is immature.** Desktop bundling for GPUI is less trodden than Tauri or Electron. | Medium | Prototype packaging for macOS (best-documented) first. If a platform proves intractable, defer it explicitly and document the deferral. |
| P6-R4 | **Markdown CV parsing is unreliable.** Freeform input resists heuristics. | High | Migration report clearly flags low-confidence parses. User is prompted to review imported data. Do not pretend the import is complete. |
| P6-R5 | **Migration loses data silently.** Relationships between reports and applications cannot be inferred. | High | Enumerate what cannot be migrated. Preserve the source directory. Make the migration non-destructive. |
| P6-R6 | **Feature variants multiply artifacts.** 2 variants × 3 platforms × 2 architectures = 12 artifacts. | Medium | Start with the minimum matrix: 1 architecture per platform, 2 variants, 6 artifacts. Add architectures on demand. |
| P6-R7 | **Update manifest is compromised.** A tampered manifest redirects users to a malicious download. | High | Sign the manifest. Embed the public key in the application. Verify before displaying. |
| P6-R8 | **Release process is not followed for future releases.** The v1.0.0 process is careful but v1.0.1 is rushed. | Medium | `RELEASE.md` is the process for all releases. Treat it as part of the codebase. Update it when the process changes. |
| P6-R9 | **Feature freeze slips.** "Just one more fix" before release. | High | Explicit feature freeze at the start of Phase 6. Only critical security fixes are allowed. Any other change defers to the next release. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | W1 migration tool, parsing all source file types, preview, report | Migration tool works end-to-end against fixture |
| **2** | W2 packaging infrastructure, code signing, artifact production, W4 auto-update | All six artifacts build; signing and notarization work |
| **3** | W3 release candidate testing, clean-machine installs, full workflow, migration on clean install | RC passes on all three platforms |
| **4** | W5 release process, W6 documentation finalization, v1.0.0 tag | v1.0.0 published; update manifest deployed |

**Total:** 2–4 weeks. The lower end (2 weeks) assumes signing certificates are ready, packaging tooling works, and no platform surprises. The upper end (4 weeks) accounts for certificate lead time, notarization iteration, and platform-specific packaging issues.

---

## Exit Criteria

Phase 6 is complete when **all** of the following are true:

1. The migration tool parses all supported source file types and produces a migration report.
2. Migration is non-destructive, idempotent, and warns about unparseable or ambiguous input.
3. Lean and full variants build successfully for macOS, Windows, and Linux (minimum matrix).
4. Installers for all three platforms install, upgrade, and uninstall cleanly.
5. macOS artifacts are signed and notarized (or the reason they are not is documented).
6. Windows artifacts are signed (or the reason they are not is documented).
7. Linux artifacts ship with signed SHA-256 checksums.
8. Auto-update check works, is opt-outable, and rejects tampered manifests.
9. Release CI produces all artifacts from a single tag.
10. Release checklist (`RELEASE.md`) is documented and followed.
11. Install, upgrade, uninstall, migration, privacy, and FAQ documentation exists.
12. The v1.0.0 release is published on GitHub with all artifacts and signatures.
13. The update manifest is deployed.
14. Clean-machine testing passes on all three platforms.
15. A short Phase 6 retrospective memo exists, recording packaging issues, migration findings, and any deferred platforms or variants.

---

## What Phase 6 Is Not

Phase 6 ships the application. It does not extend it. Specifically:

- There is no new feature work. Feature freeze is in effect.
- There is no schema change. Migrations from Phase 1 are frozen.
- There is no localization or internationalization.
- There is no App Store or Microsoft Store distribution.
- There is no full auto-update with silent install.
- There is no telemetry or crash reporting.
- There is no long-term support commitment.
- There is no v2 feature planning.

The temptation to slip "just one fix" past feature freeze is the most common way releases are delayed. Every post-freeze change is a re-test of the entire release candidate. Resist it.

---

## Post-v1 Backlog

The following items are explicitly deferred from v1 and are candidates for v2 or later:

- `printpdf` manual-ops `PdfBackend` for browser-free PDF rendering
- HTTP-level stealth layer (Tier 1.5) using browser impersonation
- Additional ATS adapters beyond the initial four
- Additional browser backends behind `BrowserBackend`
- Dynamic backend loading (post-install backend selection)
- File-based data migration from the new application back to files
- Prompt templating language (conditionals, loops, partials)
- Prompt A/B testing
- Local LLM support via Ollama or similar
- Full auto-update with signed installer download
- Telemetry and crash reporting
- Additional architecture variants (macOS x86_64, Linux aarch64)
- Additional platforms (BSDs, ARM Linux)
- Localization

---

The single most important thing to take from this plan: **start the signing certificate acquisition before Phase 6 begins.** Everything else in the phase can be compressed or parallelized. Certificate issuance cannot. Apple Developer Program enrollment takes up to 48 hours; Windows EV certificates can take weeks. If those are not already in motion, they are the critical path, and no amount of engineering effort will accelerate them. Everything else in Phase 6 is work. Certificates are waiting.
