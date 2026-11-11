//! The match session engine: decides when a match starts and ends, drives the
//! recorder, stores events on the recording's timeline, and post-processes the
//! recording into a seekable file plus automatic clips.
//!
//! The engine is driven by `tick` and talks to the recorder through traits, so
//! the whole flow can be tested without Windows or a running game.

pub mod manage;
mod naming;
pub mod overlay;
mod post;
mod session;

pub mod real;
pub mod runner;

pub use post::{PostJob, PostReport};
pub use session::{ActiveRecording, Engine, Notice, RecorderBackend, Status};
