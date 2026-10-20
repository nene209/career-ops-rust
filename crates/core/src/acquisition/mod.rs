//! Job acquisition pipeline.
//!
//! Phase 2 implements two tiers:
//!
//! - **Tier 1a** (`ats`) — public ATS JSON APIs (Greenhouse, Lever,
//!   Ashby, SmartRecruiters). One request per board, structured data.
//! - **Tier 1b** (`html`) — static HTML extraction via JSON-LD
//!   `JobPosting` schema or CSS selectors. Used for career pages that
//!   are not on a recognized ATS.
//!
//! Tier 2 (headless browser via the Obscura sidecar) lives in
//! `crate::backend` and is wired in by the router in a later workstream.
//!
//! The router module — which reads `portals.render_mode`, dispatches to
//! the appropriate tier, and records `fetch_tier` — is not part of the
//! Week 2 deliverable. It lands in Week 3.

pub mod ats;
pub mod html;
pub mod http;