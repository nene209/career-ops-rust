//! Portal suggestions (Phase 3.2, WS4).
//!
//! One compact call proposes companies whose careers page uses an embedded
//! engine. The model names a company, an engine, and a board slug; it never
//! names a URL. [`output`] derives the URL from the engine and the slug, then
//! [`service`] resolves an embedded known site's config for it.
//!
//! This is advisory in the strictest sense: nothing is added until the user
//! approves a proposal in the Portals screen.

mod output;
mod prompt;
mod service;

pub use output::{parse, SuggestEngine, SuggestedPortal, SuggestionsOutput, MAX_SUGGESTIONS};
pub use prompt::{assemble, render_known_sites, PROMPT_CATEGORY, PROMPT_NAME};
pub use service::{suggest_portals, PortalSuggestion, SuggestError, SuggestReport};
