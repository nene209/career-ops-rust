career-ops/
├── Cargo.toml
├── .gitignore
├── IMPLEMENTATION_NOTES.md
├── .github/workflows/ci.yml
├── crates/
│   ├── core/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── error.rs
│   │   │   ├── model.rs
│   │   │   ├── keystore/{mod.rs, wrap.rs, keychain.rs, argon.rs}
│   │   │   ├── db/mod.rs
│   │   │   ├── repo/{mod.rs, profile.rs, prompts.rs}
│   │   │   └── backend.rs
│   │   ├── migrations/0001_initial.sql
│   │   └── tests/keystore_safety.rs
│   └── app/
│       ├── Cargo.toml
│       └── src/{main.rs, state.rs, screens.rs}