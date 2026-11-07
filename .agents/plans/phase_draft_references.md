# Career-Ops Rust Desktop Application — Search References by Phase

**Purpose:** A consolidated index of the external references consulted or cited while drafting the phase execution plans. Organized by phase, with a cross-phase reference section for reusable topics.
**Last updated:** 2026-10-24

**Note:** the GPUI references in this document were used during Phase 2.01 and superseded by the egui choice

**Important caveat:** These are pointers for further investigation, not verified endorsements. Package versions, API surfaces, and project status change rapidly. Verify anything load-bearing against the current source before depending on it — especially crate versions, `rig` API shape, GPUI internals, and any browser backend named here.

---

## Cross-Phase References

These apply to multiple phases and are listed once.

### Rust Desktop Frameworks

| Topic | Reference | Used in |
| :--- | :--- | :--- |
| GPUI overview and current status | crates.io GPUI package page[reference:0] | All phases |
| GPUI component ecosystem | `gpui-component` GitHub[reference:1] | UI workstreams |
| Community GPUI guide | GPUI Framework Guide (hippietrail)[reference:2] | UI workstreams |
| Curated GPUI resources | `awesome-gpui`[reference:3] | UI workstreams |
| GPUI Kit (unified facade) | GPUI Kit repository[reference:4] | UI workstreams |
| GPUI pre-1.0 breaking-change reality | Using GPUI in Gauss notes[reference:5] | P1, P5 |
| GPUI service architecture discussion | "The 315-Day Vacuum"[reference:6] | P1, P5 |
| GPUI vs Tauri vs others | Tencent Cloud comparison (2026)[reference:7] | P1 |
| GPUI vs Tauri podcast discussion | Syntax episode #1043[reference:8] | P1 |
| GPUI vs other Rust GUIs | Rust Brasil comparison (2026)[reference:9] | P1 |
| Text field complexity in GPUI | "The Mail Client Problem"[reference:10] | P1, UI workstreams |

### GPUI-Tokio Integration

| Topic | Reference | Used in |
| :--- | :--- | :--- |
| `gpui-tokio-bridge` repo | GitHub (AprilNEA)[reference:11] | P2, P3, P4 |
| `gpui-tokio-bridge` docs | docs.rs crate page[reference:12] | P2, P3, P4 |
| Source and runtime details | `gpui_tokio_bridge/lib.rs`[reference:13] | P2, P3, P4 |
| Bridge alternatives | finch-xu/GetCat DeepWiki[reference:14] | P2, P3, P4 |

### GPUI Accessibility

| Topic | Reference | Used in |
| :--- | :--- | :--- |
| AccessKit integration proposal | Gauss design proposal[reference:15] | P5 |
| Focus trap utility | GPUI Kit focus trap docs[reference:16] | P5 |
| Accessibility guide | GPUI Kit accessibility guide[reference:17] | P5 |
| GPUI accessibility internals | Hex.pm GPUI guide[reference:18] | P5 |
| GPUI AccessKit primer | Gauss primer[reference:19] | P5 |
| Focus handle helper | `gpui-focus-handle` skill[reference:20] | P5 |

---

## Phase 0 — Validation Spikes

### Spike A — `printpdf` Viability

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| `printpdf` crate | crates.io page[reference:21] | Confirms manual ops API as the pixel-exact path |
| `printpdf` repo | GitHub (fschutt)[reference:22] | Confirms HTML feature is "still evolving" |
| `printpdf` docs | docs.rs 0.12.4[reference:23] | Manual positioning confirmed |
| `printpdf` + `glyph_brush_layout` integration | StackOverflow discussion[reference:24] | Known friction point: mm vs px coordinate conversion |
| `printpdf` release history | crates.io release list[reference:25] | 50 releases, 11 breaking |

### Spike B — Tier 1 Acquisition

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| `reqwest` + `scraper` guide | Scrapeless guide (2026)[reference:26] | Explicitly notes neither executes JavaScript |
| `reqwest` + `scraper` tutorial | Decodo tutorial (2026)[reference:27] | Confirms raw HTML only |
| ATS job board APIs | Apify ATS Job Board API[reference:28] | Greenhouse, Lever, Ashby, Personio |
| ATS scraper package | `@mambalabsdev/ats-scrapers`[reference:29] | Lists public JSON API endpoints per platform |
| Direct ATS endpoint references | huntkit security-stack script[reference:30] | Lever and Ashby endpoint patterns |
| ATS adapter contracts | captatum contracts doc[reference:31] | Notes Greenhouse metadata-only response omits departments |

### Spike C — Browser Backend Comparison

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| `rust-headless-chrome` repo | GitHub[reference:32] | Rust equivalent of Puppeteer |
| `rust-headless-chrome` printToPDF struct | docs.rs `PrintToPDFReturnObject`[reference:33] | Base64-decoded CDP result |
| PDF generation internals | DeepWiki writeup[reference:34] | `Tab::print_to_pdf` makes the CDP call |
| `chromiumoxide` printToPdf params | docs.rs `PrintToPdfParams`[reference:35] | Mirror of CDP protocol type |
| `chromiumoxide` alternative docs | docs.rs struct page[reference:36] | Same struct, different page |

---

## Phase 1 — Foundation

### Keystore and Encryption

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| `keyring` crate | docs.rs[reference:37] | Cross-platform credential storage |
| `keyring` platform backends hazard | agentic-engineering-patterns doc[reference:38] | **Critical:** v3.6 without features has no backend |
| `keyring_core` architecture | dashpay PR #3672[reference:39] | Per-platform store crates |
| Argon2id Rust crate | MojoAuth guide[reference:40] | `argon2` crate usage |
| Argon2id parameters | RustCrypto Book[reference:41] | Minimum 19 MiB memory, 2 iterations, 1 parallelism |
| Argon2id OWASP defaults | salting crate[reference:42] | Opinionated defaults |

### SQLCipher + sqlx

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| SQLCipher + sqlx in Tauri | Tauri plugin issue #2528[reference:43] | `.pragma("key", ...)` pattern |
| SQLCipher config in sqlx | sqlx issue #2009[reference:44] | PRAGMAs must be set before any DB action |
| Hidden PRAGMAs issue | sqlx issue #621[reference:45] | **Critical:** sqlx issues PRAGMAs before key can be set |
| SQLCipher PRAGMA key API | Zetetic SQLCipher docs[reference:46] | Official reference |
| SQLCipher key derivation | DeepWiki[reference:47] | PBKDF2-based; raw key format |
| `sqlx-sqlite-cipher` crate | GitHub (HaHa421)[reference:48] | Feature flag: `sqlcipher = ["libsqlite3-sys/sqlcipher"]` |
| SQLCipher + sqlx Windows guide | Medium (mhmtsr)[reference:49] | Step-by-step integration |
| Local-first password manager example | Medium (Tauri/SQLCipher)[reference:50] | Real-world integration pattern |
| sqlx SQLite driver internals | DeepWiki[reference:51] | FFI access details |
| Encrypted SQLite repository pattern | `flare-db` crate[reference:52] | `connect_encrypted_sqlite` helper |
| Holochain sqlx encryption wrapper | docs.rs `holochain_data`[reference:53] | Full DB encryption + key management |

### sqlx Core

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| sqlx connection pooling | docs.rs 0.3.0[reference:54] | `sqlx::Pool` |
| sqlx pool module | risingwavelabs docs[reference:55] | Canonical pool implementation |
| sqlx-sqlite-conn-mgr | lib.rs[reference:56] | Pragmatic SQLite connection policies |
| Async SQLite in Rust intro | Naurt article (2024)[reference:57] | Beginner's guide |
| sqlx migrations CLI | crates.io `sqlx-cli`[reference:58] | `sqlx migrate add`, `sqlx migrate run` |
| sqlx migrations feature config | GitHub discussion #3826[reference:59] | Feature flag setup |
| sqlx migrate macro | docs.rs `migrate!`[reference:60] | Embed migrations in binary |

---

## Phase 2 — Acquisition

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| Rust web scraping architecture | CapSolver dev guide[reference:61] | Recommends keeping fast path simple |
| reqwest proxy use | ScrapingAnt guide[reference:62] | Rate limit bypass |
| ATS Jobs Scraper (Workable) | Apify[reference:63] | Additional ATS coverage |
| Company Jobs Aggregator | Apify[reference:64] | Greenhouse, Lever, Ashby JSON |
| ATS adapter contract patterns | captatum docs[reference:65] | Tier-2 optional; adapter design |

---

## Phase 3 — AI Integration

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| Rig overview | docs.rig.rs[reference:66] | Unified API across providers |
| Rig introduction | rig.rs[reference:67] | 20+ providers, RAG, agents |
| Rig marketing site | rig.rs[reference:68] | Type-safe tools, structured output |
| Rig JetBrains guide | JetBrains blog (2026)[reference:69] | Streaming and local models |
| Rig GitHub repo | 0xPlaygrounds/rig[reference:70] | Agentic workflows, multi-turn streaming |
| Rig abstraction level comparison | trusty-tools research[reference:71] | "Actix/Axum for LLM applications" |
| Rig Extractor system | docs.rig.rs[reference:72] | Tool-based structured extraction |
| Rig Structured Output | rig.rs[reference:73] | Private "submit" tool; JSON schema via schemars |
| Rig Extractor internals | DeepWiki[reference:74] | Retry logic, hidden submit tool |
| Rig data extraction patterns | DeepWiki[reference:75] | Extractor vs TypedPromptRequest |
| Rig structured outputs / JSON schema | DeepWiki[reference:76] | Native provider schema support |

---

## Phase 4 — Output Generation

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| ATS reads embedded text layer | ai-job-search CV templates doc[reference:77] | **Key fact:** extraction order is the contract |
| career-ops ATS fix commit | GitHub commit 91e523d[reference:78] | Real fix for scrambled extraction order |
| ATS parsing order explainer | LinkedIn post (Ryan Gleason)[reference:79] | Text order ≠ visual order |
| career-ops extraction fix details | GitHub commit 202691e[reference:80] | Flex `space-between` scrambled order; `pdftotext` verification |
| ATS reads top-to-bottom not columns | LinkedIn post (Kathy Driggers)[reference:81] | Two-column PDFs break parsing |
| resumark layout module | docs.rs[reference:82] | Explicit order index per text run |
| ATS-friendly PDF checker | restruct-cv (PyPI)[reference:83] | `--ats` flag for overlay verification |

---

## Phase 5 — Polish & Hardening

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| GPUI accessibility design | Gauss AccessKit proposal[reference:84] | Screen reader + keyboard-only goals |
| GPUI focus trap | GPUI Kit docs[reference:85] | Modal focus containment |
| GPUI accessibility guide | GPUI Kit[reference:86] | Keyboard targets, focus tracking |
| GPUI internals | Hex.pm guide[reference:87] | Snapshot semantics to platform tree |
| GPUI + AccessKit | Gauss primer[reference:88] | Full screen-reader support path |
| Argon2id tuning | RustCrypto Book[reference:89] | KDF parameter floor |
| GPUI process architecture | "315-Day Vacuum"[reference:90] | Relevant to packaging and process model |

---

## Phase 6 — Migration & Release

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| SLSA framework | slsa.dev terminology[reference:91] | Supply chain integrity |
| SLSA specification | slsa.dev spec v1.1[reference:92] | Levels of security guarantees |
| SLSA overview | slsa.dev about[reference:93] | Incrementally adoptable |
| SLSA v1.2 Source Track | safeguard.sh[reference:94] | Latest version approval |
| SLSA v1.1 Build Track | safeguard.sh[reference:95] | Build track stabilized |
| SLSA and Chainloop | chainloop.dev[reference:96] | Tooling guidance |
| Sigstore introduction | blog.sigstore.dev[reference:97] | `cosign` key pair generation |
| Sigstore bundle verification | blog.sigstore.dev[reference:98] | Fulcio + Rekor |
| Cosign signing patterns | pkg.go.dev signer[reference:99] | Key-based vs keyless |
| Cosign education | Chainguard docs[reference:100] | Signing, verifying, OCI registry |
| Cosign verify-blob | Chainguard docs[reference:101] | `--bundle` flag |
| Cosign KMS plugins | blog.sigstore.dev[reference:102] | KMS-managed keys |
| Cosign keyless signing | safeguard.sh[reference:103] | Provenance and enforcement |
| Sigstore build pipeline | safeguard.sh[reference:104] | Cosign, Fulcio, Rekor, policy-controller |
| EU Cyber Resilience Act overview | EUR-Lex Regulation 2024/2847[reference:105] | Security updates during support period |
| CRA obligations | DLA Piper overview[reference:106] | Products with digital elements |
| CRA compliance lifecycle | Freshfields Part 2[reference:107] | Annex I essential requirements |
| CRA implementation guidance | DLA Piper (March 2026)[reference:108] | Commission draft guidance |
| CRA legislative summary | EU Digital Strategy[reference:109] | Horizontal regulatory framework |
| CRA and SaaS boundary | DLA Piper[reference:110] | Scope of "products with digital digital elements" |
| Post-quantum TLS migration | IETF draft[reference:111] | Hybrid key exchange (X25519MLKEM768) |
| Hybrid certificates for PQC | IEEE paper (2026)[reference:112] | Transition mechanisms |
| Replacing vulnerable algorithms | Taylor & Francis chapter[reference:113] | RSA, ECC, DH migration |
| Dual algorithm certificates | ACM paper (2026)[reference:114] | Backward-compatible PQC transition |
| Zero-downtime PQC migration | ICISC 2025 paper[reference:115] | Bridge-server approach |
| PQC benchmarking and crypto-agility | IEEE paper (2026)[reference:116] | Phased deployment strategy |

---

## Phase 7 — Post-v1

### WebAssembly Component Model

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| Wasm Components goals | Rust lang goals[reference:117] | WASI 0.3 expected early 2026; async support |
| `wit-component` crate | crates.io[reference:118] | Component binary creation |
| `wasm-component-ld` | Rust source repo[reference:119] | Native component output for `wasm32-wasip2` |
| Component model in Rust | Bytecode Alliance docs[reference:120] | `cargo component` tool |
| Component model linker README | Rust source repo[reference:121] | Component vs core wasm |
| WARP toolkit | GitHub topics[reference:122] | Rust-first modular Wasm components |
| Wasm tutorial | mytechnotalent/Wasm[reference:123] | `#![no_std]` guests |

### sqlite-vec Vector Search

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| sqlite-vec (Go) | pkg.go.dev[reference:124] | Embeddings in a single SQLite file |
| sqlite-vec (C, multi-platform) | PyPI (win-arm64 pre-release)[reference:125] | Pure C, no dependencies, runs anywhere |
| Go package integration | gosqlite.org/vec[reference:126] | First-class vector search |
| Building a RAG on SQLite | blog.sqlite.ai[reference:127] | SQLite-Vector + SQLite-AI extensions |
| Hybrid search with sqlite-vec | Simon Willison's weblog[reference:128] | Vector + traditional search |
| sqlite-vec vector types | npm package page[reference:129] | Float, int8, binary trade-offs |
| vec1 virtual tables | SQLite official docs[reference:130] | Nearest-neighbor queries, ANN mode |

### Local LLM Inference

| Topic | Reference | Notes |
| :--- | :--- | :--- |
| ollama-rs crate | crates.io[reference:131] | Interacts with Ollama API |
| corell local-first inference kernel | docs.rs[reference:132] | 0-network async framework |
| Rust mentor GGUF model | Hugging Face[reference:133] | Local inference via Ollama/llama.cpp |
| Cross-process model sharing | Rustyclaw docs[reference:134] | Ollama server vs per-process loading |
| ragrig trait-driven RAG | docs.rs[reference:135] | Runtime hot-swapping, zero native deps |
| Local summarizer agent | crates.io[reference:136] | 100% local, async, Ollama-based |

---

## Appendix: Reference Quality Notes

Not all references in this document are equally reliable. Use this guide when deciding how much weight to give a source:

| Source type | Examples | Reliability |
| :--- | :--- | :--- |
| **Official docs** | docs.rs, crates.io, slsa.dev, EUR-Lex | High — but check the version |
| **Primary source repos** | GitHub repos for `printpdf`, `rig`, `rust-headless-chrome` | High — check commit recency |
| **DeepWiki summaries** | DeepWiki entries for sqlx, rig | Medium — auto-generated, verify against source |
| **Technical blogs** | Scrapeless, Decodo, JetBrains, Chainguard | Medium — usually accurate, occasionally stale |
| **Community guides** | Medium articles, StackOverflow answers | Medium to low — verify against official docs |
| **Social posts** | LinkedIn, X/Twitter, forum threads | Low — useful as signal, not as authority |
| **This document** | Self-referential | Low — a pointer index, not a source |

**Highest-value references for each phase:**

- **Phase 1:** sqlx issue #621 (SQLCipher PRAGMA ordering) and the `keyring` platform-features warning. Both encode failure modes that are easy to miss.
- **Phase 4:** career-ops commit 91e523d and the ai-job-search CV template doc. Both document real-world ATS extraction failures.
- **Phase 6:** SLSA spec and the CRA legislative text. Both are authoritative and directly relevant to release obligations.

---

## How to Use This Document

1. **Before starting a phase**, skim its references to refresh the technical landscape. Several of these topics move quickly.
2. **During a phase**, use the references as starting points for deeper investigation — not as final answers.
3. **When a reference is stale**, replace it rather than adding alongside. This document is a current index, not an archive.
4. **When adding a new reference**, note which phase and workstream it supports. An unlabeled reference is not useful.

The most important habit this document encourages: **verify before depending**. Every crate named here is a dependency decision, and every framework named here is an architectural decision. Neither should rest on a search result alone.