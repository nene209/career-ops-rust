# Career-Ops (Rust Desktop)

A desktop application for managing a job search pipeline. Jobs are
acquired from configured portals, evaluated against your CV, and used to
generate tailored cover letters and ATS-friendly PDFs. The application
never submits applications on your behalf — every status change requires
explicit confirmation.

This is a port of the original
[career-ops](https://github.com/career-ops-hq/career-ops) Node.js tool
from a file-based CLI to a stateful, encrypted desktop application.

**Status:** Phase 1 complete. Acquisition (Phase 2) is next.

---

## Building

### Prerequisites

- **Rust 1.95.0 or newer.** The GPUI dependency uses
  `std::hint::cold_path`, which was stabilized in 1.95.0. Older
  toolchains will fail to compile `career-ops-app`.
  Run `rustc --version` to check. Upgrade with `rustup update stable`.
- **Linux only:** `libsecret-1-dev` and `pkg-config` for the keychain
  backend. On Debian/Ubuntu:
  ```bash
  sudo apt-get install -y libsecret-1-dev pkg-config libssl-dev
  ```
- **SQLCipher** is vendored via `libsqlite3-sys` with
  `bundled-sqlcipher-vendored-openssl`, so no system SQLCipher is
  required.

### Build

```bash
cargo build --workspace
```

### Build the binary

```bash
cargo build -p career-ops-app --release
```

The binary is at `target/release/career-ops`.

---

## Testing

The test suite has two configurations that matter: **with** and
**without** the `keychain` feature. Both must pass.

### Full test suite (default features)

```bash
cargo test --workspace
```

This is the primary command. It runs every test in every crate,
including the keystore tests, the app state-flow tests, and the prompt
seeding tests. It requires the platform keychain to be available
(see Prerequisites).

### Core crate, no keychain

```bash
cargo test -p career-ops-core --no-default-features
```

This exercises the `keychain` stubs. It confirms that the codebase
compiles and the tests that do not touch the keychain still pass in a
build without a keychain backend. This is what runs in the `test-no-keychain-feature` CI job.

### Individual test binaries

If you are working on a specific subsystem:

```bash
# Keystore safety — the "never re-initialize" rule, wrong-passphrase
# rejection, corrupted-wrap detection
cargo test -p career-ops-core --test keystore_safety

# Database round-trip — SQLCipher open, reopen, wrong-key rejection,
# migration idempotency
cargo test -p career-ops-core --test db_roundtrip

# Backend traits — mock browser and PDF backends, including the
# BackendUnsupported path that Obscura will use
cargo test -p career-ops-core --test backend_traits

# Prompt seeding — first seed, idempotent re-seed, content-change
# versioning, user-prompt collision
cargo test -p career-ops-core --test prompts_seed

# App state flow — full first-launch → unlock → ready lifecycle, plus
# the recovery path for wrong passphrases
cargo test -p career-ops-app --test state_flow

# Keychain failure injection — only the tests that exercise a broken
# platform keychain. This is the filter used by the
# `keychain-failure-injection` CI job.
cargo test -p career-ops-app --test state_flow keychain
```

### Test notes

- **The `state_flow.rs` tests serialize on a `tokio::sync::Mutex`.**
  They mutate the process-global test keychain and cannot safely run in
  parallel. The mutex handles this transparently; you do not need to
  pass `--test-threads=1`.
- **The keychain-path test is not `#[ignore]`d.** It works with the
  persistent test backend in `crates/test-support/`. The built-in
  `keyring::mock` backend is entry-scoped and cannot support this test;
  the test-support crate exists to work around that limitation.
- **GPUI is compiled as part of `cargo test --workspace`.** The first
  run will be slow (several minutes). Subsequent runs use the cached
  build.

---

## Clippy and formatting

Both are enforced in CI and should be clean before opening a pull
request.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

The clippy command is **not** advisory. The CI job fails on any
warning. Two recurring patterns are worth knowing:

- **`dead_code` on feature-gated items.** When a build variant disables
  a subsystem, wrap the subsystem in a `mod inner { ... }` and gate the
  module, not the individual items. This is the pattern used in
  `keystore/keychain.rs` and `keystore/argon.rs`.
- **`unused_imports` on feature-gated imports.** Put the `use` inside
  the `#[cfg]` block that needs it. An import at an outer scope that is
  only used by inner-scope code will be flagged.

---

## Project layout

```
crates/
  core/           Library. No GPUI, no UI. Testable headlessly.
    src/
      keystore/   Envelope encryption: DEK + keychain KEK + passphrase KEK
      db/         SQLCipher + sqlx setup, migrations, consistency check
      model.rs    One type per table
      repo/       Repository layer, one module per table
      backend.rs  BrowserBackend and PdfBackend traits + mocks
      prompts_seed.rs  Embedded system prompts, update-aware seeding
    migrations/
      0001_initial.sql  All eleven tables
    tests/        Integration tests (see Testing section)
    prompts/      System prompt .md files copied from the original repo

  app/            Binary + library. The library target exists so
                  integration tests can import the state machine.
    src/
      state.rs    Application state machine, unlock gating
      screens.rs  GPUI view placeholders
      lib.rs      Library entry point (pub mod state, pub mod screens)
      main.rs     Binary entry point

  test-support/   Shared test infrastructure
    src/lib.rs    Persistent in-memory keychain backend

docs/
  phase-1-retrospective.md    What happened during Phase 1
  phase-2-acquisition.md      Phase 2 plan (when written)
  decisions-addendum.md       Decisions made after the main plan was
                              written, superseding specific sections
```

---

## Configuration

### Data directory

The application stores its data in a directory determined at runtime.
For Phase 1, `career_ops_core::data_dir()` reads `CAREER_OPS_DATA_DIR`
from the environment. In production this will be replaced with the
OS-specific app data directory.

Set it explicitly to control where the database and key file live:

```bash
export CAREER_OPS_DATA_DIR="$HOME/.local/share/career-ops"
```

Two files live there:

- `career-ops.db` — the SQLCipher-encrypted database.
- `dek.wrapped.json` — the wrapped Data Encryption Key. **Back this up
  alongside the database.** Losing it means losing access to the
  database.

### API keys

API keys for LLM providers are stored in the OS keychain, not in the
database and not in any config file. The `settings` table records the
chosen provider and model, never the key itself.

### Encryption

Encryption at rest is mandatory. There is no unencrypted mode.

The database key (DEK) is random. It is wrapped twice: once by a
keychain-stored KEK and once by a passphrase-derived KEK. Either wrap
can unlock the database. Both must fail for the data to become
unreadable.

On first launch the application will:

1. Generate a random DEK.
2. Generate a random keychain KEK and write it to the OS keychain.
3. Ask you to set a passphrase, and wrap the DEK with it.
4. Write `dek.wrapped.json` and create the encrypted database.

The passphrase step is not optional, because it is the only recovery
path if the keychain entry becomes unavailable.

---

## Troubleshooting

### "GPUI fails to compile with E0658 `cold_path`"

Your Rust toolchain is older than 1.95.0. Run `rustup update stable`
and rebuild.

### "Dependency `keyring` has feature `mock` but it does not exist"

You have `features = ["mock"]` on `keyring` in a `Cargo.toml`. Remove
it. The mock credential store is always compiled in; the feature was
removed in 3.x.

### "Every keychain read returns NoEntry"

The `keyring` crate's platform backends are opt-in. Check that
`apple-native`, `windows-native`, or `linux-native` are enabled in the
`keyring` dependency in `crates/core/Cargo.toml`. The
`keychain-sentinel` test job exists specifically to catch this.

### "Database opens but a query fails with a decryption error"

The DEK used to open the database does not match the DEK the database
was created with. Check `dek.wrapped.json` and the keychain entry. The
consistency check in `db::open` should have caught this before the
query; if it did not, that is a bug.

### "The application will not unlock"

The recovery screen offers five options: passphrase unlock, retry
keychain, locate the key file, export the encrypted database, and last
resort reset. Try them in that order. The last-resort reset destroys the
existing data; only use it after exporting the database and key file.

---

## Contributing

### Before opening a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p career-ops-core --no-default-features
```

All four must be clean. The CI runs the same commands on Linux, macOS,
and Windows.

### Safety rules that are not negotiable

These are encoded in tests and enforced by the repository layer. If a
change makes any of them fail, the change is wrong.

- **`applied` is reachable only through `mark_applied`.** The transition
  table does not allow any state to reach `applied` directly, and
  `mark_applied` requires the current status to be `ready`.
- **The application never submits an application.** All submission is
  manual. Any future "assisted submission" feature must respect this.
- **An existing `dek.wrapped.json` blocks fresh initialization.** The
  application must not create a new DEK if a wrapped file already
  exists, even if it cannot be unlocked. Doing so would orphan the
  existing database.
- **A wrong key fails before any write.** `db::open` runs a
  `SELECT count(*) FROM sqlite_master` immediately after `PRAGMA key`; a
  wrong key fails this check and the pool is closed before any
  application-level query runs.

### Where to make changes

- **Business logic** → `crates/core/src/repo/` and the forthcoming
  `crates/core/src/acquisition/`, `evaluation/`, `render/` modules.
- **Encryption** → `crates/core/src/keystore/`. Changes here need
  matching updates to `tests/keystore_safety.rs`.
- **Schema** → `crates/core/migrations/`. Migrations are append-only
  once released. To change an existing table, add `0002_*.sql`, not an
  edit to `0001_initial.sql`.
- **UI** → `crates/app/src/screens.rs`. The state machine in `state.rs`
  is the load-bearing part; the views are replaceable.

---

## License

MIT OR Apache-2.0.