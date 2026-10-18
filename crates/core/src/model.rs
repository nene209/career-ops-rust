//! Domain types. One type per table in `migrations/0001_initial.sql`.
//!
//! These are deliberately simple — Phase 1 establishes the shapes; later
//! phases add behaviour via repositories and services.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub location: Option<String>,
    pub summary: Option<String>,
    pub archetypes: Option<String>,  // JSON
    pub preferences: Option<String>, // JSON
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CvEntry {
    pub id: i64,
    pub profile_id: i64,
    pub kind: String, // "experience" | "education" | "skill"
    pub title: String,
    pub organization: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<String>, // JSON
    pub sort_order: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationStatus {
    Discovered,
    Evaluating,
    Ready,
    Applied,
    Interviewing,
    Rejected,
    Offer,
}

impl ApplicationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Discovered => "discovered",
            Self::Evaluating => "evaluating",
            Self::Ready => "ready",
            Self::Applied => "applied",
            Self::Interviewing => "interviewing",
            Self::Rejected => "rejected",
            Self::Offer => "offer",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "discovered" => Self::Discovered,
            "evaluating" => Self::Evaluating,
            "ready" => Self::Ready,
            "applied" => Self::Applied,
            "interviewing" => Self::Interviewing,
            "rejected" => Self::Rejected,
            "offer" => Self::Offer,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FetchTier {
    Tier1aAts,
    Tier1bHtml,
    Tier2Obscura,
    Tier2HeadlessChrome,
    Manual,
}

impl FetchTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tier1aAts => "tier1a:ats",
            Self::Tier1bHtml => "tier1b:html",
            Self::Tier2Obscura => "tier2:obscura",
            Self::Tier2HeadlessChrome => "tier2:rust-headless-chrome",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct JobDescription {
    pub id: i64,
    pub title: String,
    pub company: String,
    pub source_url: String,
    pub raw_text: String,
    pub fetched_at: DateTime<Utc>,
    pub fetch_tier: String,
    pub external_id: Option<String>,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Application {
    pub id: i64,
    pub job_description_id: i64,
    pub status: String,
    pub applied_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub match_score: Option<f64>,
    pub evaluation_report_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub id: i64,
    pub application_id: i64,
    pub content: String,
    pub generated_at: DateTime<Utc>,
    pub model_used: String,
    pub prompt_version: Option<i64>,
    pub prompt_hash: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CoverLetter {
    pub id: i64,
    pub application_id: i64,
    pub content: String,
    pub generated_at: DateTime<Utc>,
    pub model_used: String,
    pub prompt_version: Option<i64>,
    pub prompt_hash: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub kind: String, // "cv" | "cover_letter"
    pub content: String,
    pub version: i64,
    pub content_hash: String,
    pub is_system: bool,
    pub supersedes_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DocumentRender {
    pub id: i64,
    pub application_id: Option<i64>,
    pub kind: String,
    pub backend_used: String,
    pub template_id: Option<i64>,
    pub file_path: String,
    pub rendered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Portal {
    pub id: i64,
    pub company_name: String,
    pub url: String,
    pub render_mode: String, // "static" | "browser"
    pub scraper_config: Option<String>, // JSON
    pub last_verified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Prompt {
    pub id: i64,
    pub name: String,
    pub category: String, // "evaluate" | "cover_letter" | "scan"
    pub content: String,
    pub is_system: bool,
    pub version: i64,
    pub content_hash: String,
    pub supersedes_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub value: String, // JSON
}