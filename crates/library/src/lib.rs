//! The clip library: SQLite storage for matches, players, events and clips,
//! auto-clip planning, retention planning and the ffmpeg helper.

pub mod autoclip;
pub mod db;
pub mod ffmpeg;
pub mod retention;
pub mod settings;

pub use autoclip::{plan_clips, ClipPlan};
pub use db::{ClipRow, EventRow, Library, MatchRow, MatchSummary, NewClip, NewEvent, NewMatch, PlayerRow};
pub use ffmpeg::Ffmpeg;
pub use settings::{AppSettings, AutoClipRules};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ffmpeg failed: {0}")]
    Ffmpeg(String),
    #[error("not found")]
    NotFound,
}

pub type Result<T> = std::result::Result<T, Error>;
