# Logging

Career-Ops records what it does through `tracing`, into **two sinks from one
event stream**:

| Sink | Purpose | Lifetime |
| :--- | :--- | :--- |
| `<data dir>/logs/career-ops.log` | The durable record: survives a crash and is what a bug report attaches | Rotated, five archives kept |
| In-memory ring (2,000 lines) | What the **Logs** screen and the drawer render | This session |

The UI never reads the file. It renders the structured buffer, so filtering by
level and searching a field do not have to parse text back into structure.

---

## Where the log is

The data directory is the one the app already uses for its vault. On macOS that
is `~/Library/Application Support/career-ops`, and `CAREER_OPS_DATA_DIR`
overrides it (which is how the manual script keeps a throwaway vault).

```
<data dir>/logs/career-ops.log
```

The Logs screen prints the exact path under the capture line, and **Open
folder** shows it in the file manager.

**Rotation** happens once, at startup, when the current file has reached 5 MB.
It is renamed to `career-ops.YYYYMMDD-HHMMSS.mmm.log` and the five newest
archives are kept. Rotation at startup rather than on every write is deliberate:
the app writes tens of lines a day, and stat-ing the file on a hot path to learn
that would be waste.

## Levels

| Situation | File | In-app buffer |
| :--- | :--- | :--- |
| `RUST_LOG` unset (the normal case) | `info` | `info`, plus `debug` for `career_ops_core` and `career_ops_app` |
| `RUST_LOG` set | whatever it says | whatever it says |

`info` is enough to see every fetch, page, and walk chunk, so a release build is
diagnosable without an environment variable — which is the whole point of the
phase that added this. The buffer goes one step deeper for the app's own
targets because it is bounded and cheap, and because a level chip that can
*reveal* a captured `debug` line is more useful than one that can only hide
`info`. The file stays at `info`: a `debug` line per enriched description would
bury the per-page lines in a 40-job fetch.

An **empty** `RUST_LOG` is ignored rather than silencing everything, which is
what `EnvFilter` would otherwise do.

To go deeper than the built-in policy, set `RUST_LOG` before launching:

```bash
RUST_LOG=career_ops_core::acquisition=trace ./target/debug/career-ops
```

A line that never entered the buffer cannot be revealed by the chips; the Logs
screen says which policy is in effect. There is no restart-free level switch
(see the phase plan's Q-45).

## Reading it in the app

- **Logs** in the sidebar (last, after a separator).
- **Logs drawer** in the sidebar footer, or <kbd>Cmd</kbd>/<kbd>Ctrl</kbd> +
  <kbd>L</kbd>: the same rows in a resizable strip at the bottom of the window,
  so a fetch can be watched while Portals is on screen.

Both mounts share one view, so pausing in one pauses the other.

| Control | What it does |
| :--- | :--- |
| **All** / **Error** / **Warn** / **Info** / **Debug** / **Trace** | Admits that level and anything more severe |
| **Filter** | Case-insensitive match against the message, the target, or a field value |
| **Pause** / **Resume** | Freezes the list at the current line; the paused header reports how many arrived since |
| **Follow** | Keep the newest line in view. Scrolling up turns it off; clicking it turns it back on |
| **Copy visible** | The rows currently shown, one per line, `key=value` fields included |
| **Open folder** | Disabled in a buffer-only session (tests, or a session where the file could not be opened) |
| **Delete logs** | Asks first, then deletes the current file and its archives and clears the buffer |

Rows are monospace and are not reflowed: a log line wraps badly, so the
horizontal scrollbar is the reading surface.

## What is written

| Event | Level | Fields |
| :--- | :--- | :--- |
| `career-ops starting` | info | `version`, `profile`, `data_dir`, `log` |
| `page read` (HTML and Eightfold) | info | `page`, `jobs`, `added`, plus `url`/`bytes` or `offset` |
| `fetch finished` (one per fetch call) | info | `portal`, `tier`, `first_page`, `pages`, `jobs`, `details_attempted`, `details_enriched`, `stop`, `elapsed_ms` |
| `fetch-all chunk` (one per walk chunk) | info | `portal`, `first_page`, `pages`, `deepest`, `jobs`, `stop` |
| `eightfold position skipped` | warn | `index`, `positions`, the parse error |
| `eightfold page kept without…` | warn | `positions`, `skipped` |
| `sidecar stderr` | debug | `line` |
| Panic (`logging::panic`) | error | payload and source location, written to the file |

A failure also carries its classification: the same cause and suggested action
the Portals health badge shows.

## What is never written

The database is encrypted; **this file is not**. The rule is enforced by review,
and the plan that added logging states it as a design decision:

- **Never** log passphrases, key material, CV or profile text, prompts and
  completions, job description bodies, or HTTP response bodies.
- **Always fine**: URLs, external ids, counts, byte sizes, statuses, tiers, stop
  reasons, error strings.
- Anything body-adjacent goes through `career_ops_core::logging::truncate_for_log`
  first, which cuts on a character boundary and appends `…`.

Logs are plaintext, on your disk, and outside the vault. Deleting them is a
normal action, not a recovery step.

## Attaching a log to a bug report

1. **Pause** and **Copy visible** for the lines that matter, or **Open folder**
   and take `career-ops.log`.
2. The first line names the build (`version`, `profile`), the data directory, and
   the log path — quote it, because it answers most reproduction questions.
3. Include the surrounding `page read` / `fetch-all chunk` lines, not just the
   error: the error says what broke, the chunk line says where in the walk.

A failed fetch's error line in Portals has **View logs** beside it, which opens
this screen directly.

## The terminal

`main` installs the file sink and the buffer; it does **not** echo to stderr.
`RUST_LOG=debug cargo run` therefore no longer prints to your terminal — tail the
file instead:

```bash
tail -f "$CAREER_OPS_DATA_DIR/logs/career-ops.log"
```

A failing ignored live test is the exception: it prints the captured buffer when
it fails, so `cargo test -- --ignored --nocapture` shows what the run logged.

## Deleting the file while the app is running

The writer keeps its handle open, so **Delete logs** unlinks the file rather than
emptying it: the name is gone and a fresh file appears on the next launch. The
screen says so instead of implying that file logging stopped.
