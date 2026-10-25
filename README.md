# Career-Ops (Rust Desktop)

A desktop application for managing a job search pipeline. Jobs are
acquired from configured portals, evaluated against your CV, and used to
generate tailored cover letters and ATS-friendly PDFs. The application
never submits applications on your behalf — every status change requires
explicit confirmation.

This is a port of the original
[career-ops](https://github.com/career-ops-hq/career-ops) Node.js tool
from a file-based CLI to a stateful, encrypted desktop application.

**Status:** Phase 1 complete. Phase 2 (acquisition) in progress — Week 1
spike complete, Weeks 2–4 remaining.

---

## Building

### Prerequisites

- **Rust 1.95.0 or newer.** Run `rustc --version` to check, and
  `rustup update stable` to upgrade. The 1.95.0 floor was originally
  set by GPUI's use of `std::hint::cold_path`, and GPUI is no longer a
  dependency — the constraint is now a project preference rather than
  a technical requirement. The workspace's `rust-version` field in the
  root `Cargo.toml` is the value `cargo` actually enforces; if the two
  disagree, the field wins.
  Run `rustc --version` to check. Upgrade with `rustup update stable`.
- **Linux only:** `libsecret-1-dev` and `pkg-config` for the keychain
  backend. On Debian/Ubuntu:
  ```bash
  sudo apt-get install -y libsecret-1-dev pkg-config libssl-dev
  ```
- **SQLCipher** is vendored via `libsqlite3-sys` with
  `bundled-sqlcipher-vendored-openssl`, so no system SQLCipher is
  required.
- **Obscura** is required for the acquisition test suite and for
  browser-mode (Tier 2) portal fetches in the running application.
  Static (Tier 1) fetches work without it. Put `obscura` on `PATH` or
  point `ObscuraConfig::binary` at it. The vendored binary described
  under Testing is for the test suite only.

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
(see Prerequisites) and the Obscura binary (see below).

### Core crate, no keychain

```bash
cargo test -p career-ops-core --no-default-features
```

This exercises the `keychain` stubs. It confirms that the codebase
compiles and the tests that do not touch the keychain still pass in a
build without a keychain backend. This is what runs in the
`test-no-keychain-feature` CI job.

### Vendored Obscura binary (test-only)

The acquisition tests drive the [Obscura](https://github.com/h4ckf0r0day/obscura)
headless browser over CDP. Obscura is not part of the application
binary — it is a separate executable that the backend spawns as a
sidecar process. For tests, the executable must be discoverable.

The test harness resolves the Obscura binary in this order:

1. The `OBSCURA_BIN` environment variable, if set
2. `vendor/obscura` at the workspace root
3. `obscura` on `PATH`

The fallback to `vendor/obscura` is resolved from
`env!("CARGO_MANIFEST_DIR")`, so it works regardless of the shell's
current working directory. This is the recommended local-development
setup.

#### Download the release binary

Download the archive for your platform from the Obscura
[releases page](https://github.com/h4ckf0r0day/obscura/releases), then:

```bash
mkdir -p vendor
cp path/to/extracted/obscura vendor/obscura
chmod +x vendor/obscura
```

#### Build from source

Building from source takes about five minutes on first build (V8 is
compiled). The `render` feature is required if you want to test
`Page.printToPDF` (the Phase 4 PDF evaluation will depend on it):

```bash
git clone https://github.com/h4ckf0r0day/obscura
cd obscura
cargo build --release -p obscura-cli --bins --features render
mkdir -p /path/to/career-ops-rust/vendor
cp target/release/obscura /path/to/career-ops-rust/vendor/obscura
```

#### Version requirements

- **v0.2.5 or later.** Earlier versions have a CDP bug where
  `TargetInfo` omits the `canAccessOpener` field, which strict CDP
  clients (including `chromiumoxide`) reject. The symptom is a panic
  reading `"Created target not present"` from `Browser::connect`.
- **Built with `--features render`** if building from source, for
  `Page.printToPDF` support.

#### Override the path

To use a different binary without moving it:

```bash
OBSCURA_BIN=/usr/local/bin/obscura cargo test -p career-ops-core --test obscura_spike
```

#### Repository policy

`vendor/obscura` is a binary (~70 MB) and is not committed to the repository:

- **Gitignore it.** Add `vendor/obscura` to `.gitignore` and rely on
  this section for setup. Keeps the repo small; every contributor
  downloads once.

CI should use a download step rather than a committed binary, so that
CI and developer machines run the same version and the repository
stays portable. The binary is only needed for tests, not for the
application itself — the production distribution model for Obscura is
a Phase 6 concern.

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
# BackendUnsupported path that Phase 2 leaves on ObscuraBrowserBackend
cargo test -p career-ops-core --test backend_traits

# Prompt seeding — first seed, idempotent re-seed, content-change
# versioning, user-prompt collision
cargo test -p career-ops-core --test prompts_seed

# Obscura sidecar spike — CDP connection, post-JS DOM, printToPDF,
# sidecar death detection. Requires the obscura binary.
cargo test -p career-ops-core --test obscura_spike -- --nocapture

# App state flow — full first-launch → unlock → ready lifecycle, plus
# the recovery path for wrong passphrases
cargo test -p career-ops-app --test state_flow

# Keychain failure injection — only the tests that exercise a broken
# platform keychain. This is the filter used by the
# `keychain-failure-injection` CI job.
cargo test -p career-ops-app --test state_flow keychain
```

The last command is a filtered run: `keychain` is a substring filter that
matches only the tests whose names contain that word. The unfiltered
`cargo test -p career-ops-app --test state_flow` runs the entire state-flow
suite, which is what you want when working on the state machine generally.
The filtered run is what you want when you are specifically touching the
keychain failure paths, or when you want to reproduce exactly what the
`keychain-failure-injection` CI job sees.

### Test notes

- **The `state_flow.rs` tests serialize on a `tokio::sync::Mutex`.**
  They mutate the process-global test keychain and cannot safely run in
  parallel. The mutex handles this transparently; you do not need to
  pass `--test-threads=1`.
- **The keychain-path test is not `#[ignore]`d.** It works with the
  persistent test backend in `crates/test-support/`. The built-in
  `keyring::mock` backend is entry-scoped and cannot support this test;
  the test-support crate exists to work around that limitation.
- **The `obscura_spike` test requires the Obscura binary.** See
  [Vendored Obscura binary](#vendored-obscura-binary-test-only). If
  it fails with `No such file or directory`, the binary is not at
  `vendor/obscura` and `OBSCURA_BIN` is not set.
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

The keystore module carries an explicit
`#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]`.
`clippy::unwrap_used` and `clippy::expect_used` are restriction lints
and are **not** enabled by `-D warnings` alone. This attribute is what
makes the "no unwrap in keystore code" invariant real.

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
      backend_traits.rs
      db_roundtrip.rs
      keystore_safety.rs
      obscura_spike.rs      Obscura sidecar spike (Week 1 Phase 2)
      prompts_seed.rs
      common/mod.rs         Shared test harness
    prompts/      System prompt .md files copied from the original repo

  app/            Binary + library. The library target exists so
                  integration tests can import the state machine.
    src/
      state.rs    Application state machine, unlock gating
      screens/    egui view layer (Phase 2.1)
      lib.rs      Library entry point (pub mod state, pub mod screens)
      main.rs     Binary entry point
    tests/
      state_flow.rs

  test-support/   Shared test infrastructure
    src/lib.rs    Persistent in-memory keychain backend

vendor/
  obscura         Test-only headless browser binary (see Testing)

docs/
  phase-1-retrospective.md    What happened during Phase 1
  phase-2-acquisition.md      Phase 2 plan
  decisions-addendum.md       Decisions made after the main plan was
                              written, superseding specific sections
```

---

## Configuration

### Data directory

The application stores its data in the OS-specific app data directory
(`~/Library/Application Support/career-ops` on macOS,
`$XDG_DATA_HOME/career-ops` or `~/.local/share/career-ops` on Linux, and
`%APPDATA%\career-ops\data` on Windows). Phase 2.1 replaced the Phase 1
placeholder that required an environment variable.

`CAREER_OPS_DATA_DIR` overrides that location, which is useful for
development and isolated test runs:

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

### "`obscura_spike` fails with `No such file or directory`"

The test could not find the Obscura binary. Either:

- Place the binary at `vendor/obscura` (resolved relative to the
  workspace root, independent of shell cwd), or
- Set `OBSCURA_BIN` to an absolute path:

```bash
OBSCURA_BIN=/absolute/path/to/obscura cargo test -p career-ops-core --test obscura_spike
```

A relative path like `./vendor/obscura` will **not** work. Cargo runs
test binaries with the working directory set to the crate's manifest
directory (`crates/core/`), not the shell's cwd.

### "`obscura_spike` panics with `Created target not present`"

The Obscura binary is older than v0.2.5. This is the
`canAccessOpener` CDP bug fixed in
[PR #165](https://github.com/h4ckf0r0day/obscura/pull/165). Upgrade the
binary; do not modify the Rust code.

### "`obscura_spike` passes but `printToPDF` reports UNSUPPORTED"

The Obscura binary was built without the `render` feature. Rebuild with:

```bash
cargo build --release -p obscura-cli --bins --features render
```

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
- **No `unwrap()` in keystore code paths.** Enforced by the clippy
  attribute in `keystore/mod.rs`. If a new keystore function needs to
  unwrap, use `?` with a typed error instead.

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
- **Browser backend** → the `ObscuraBrowserBackend` implementation in
  `crates/core/src/backend/` (Phase 2). Changes here should be validated
  by running `cargo test -p career-ops-core --test obscura_spike`.

---

## License

MIT OR Apache-2.0.
