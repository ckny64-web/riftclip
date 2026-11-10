//! Records one window plus that process's audio to a fragmented MP4 using
//! Windows Graphics Capture, a hardware H.264 encoder and WASAPI process
//! loopback. Nothing is injected into the captured process.
//!
//! The crate builds on every platform so the workspace stays testable, but
//! `Recorder::start` only works on Windows.

use std::path::Path;
use std::time::Duration;

#[cfg(windows)]
mod win;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no visible window found for {0}")]
    WindowNotFound(String),
    #[error("recording is only supported on Windows")]
    Unsupported,
    #[error("{0}")]
    Other(String),
    #[cfg(windows)]
    #[error("{context}: {source}")]
    Windows {
        context: &'static str,
        #[source]
        source: windows::core::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// What to record.
#[derive(Debug, Clone)]
pub enum Target {
    /// Executable file name, e.g. `League of Legends.exe` (case-insensitive).
    ProcessName(String),
    /// A specific process id.
    Pid(u32),
}

#[derive(Debug, Clone)]
pub struct Settings {
    /// Output frame rate; frames arriving faster than this are dropped.
    pub fps: u32,
    /// Video bitrate in kilobits per second.
    pub bitrate_kbps: u32,
    /// Seconds between keyframes. Clips are cut on keyframes, so keep this small.
    pub keyframe_interval_secs: u32,
    /// Capture the target process's audio. If audio capture cannot start the
    /// recording continues without it (see `Stats::audio`).
    pub audio: bool,
    pub capture_cursor: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { fps: 60, bitrate_kbps: 20_000, keyframe_interval_secs: 1, audio: true, capture_cursor: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub width: u32,
    pub height: u32,
    /// Frames handed to the encoder.
    pub frames_written: u64,
    /// Frames skipped because they arrived faster than the output frame rate.
    pub frames_paced_out: u64,
    /// Frames lost because the encoder could not keep up.
    pub frames_dropped: u64,
    /// Whether an audio track is being recorded.
    pub audio: bool,
    /// Why audio is off, when it was requested but could not start.
    pub audio_error: Option<String>,
    pub duration: Duration,
}

pub struct Recorder;

impl Recorder {
    /// Start recording `target` into `output` (an `.mp4` path). Recording runs
    /// on background threads until [`RecordingHandle::stop`] is called.
    pub fn start(target: &Target, output: &Path, settings: &Settings) -> Result<RecordingHandle> {
        #[cfg(windows)]
        {
            win::start(target, output, settings).map(|inner| RecordingHandle { inner })
        }
        #[cfg(not(windows))]
        {
            let _ = (target, output, settings);
            Err(Error::Unsupported)
        }
    }
}

pub struct RecordingHandle {
    #[cfg(windows)]
    inner: win::Recording,
}

impl RecordingHandle {
    /// Time since the first moment of the recording; matches video timestamps.
    pub fn elapsed(&self) -> Duration {
        #[cfg(windows)]
        {
            self.inner.elapsed()
        }
        #[cfg(not(windows))]
        {
            Duration::ZERO
        }
    }

    /// False once the captured window has closed.
    pub fn is_running(&self) -> bool {
        #[cfg(windows)]
        {
            self.inner.is_running()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    pub fn stats(&self) -> Stats {
        #[cfg(windows)]
        {
            self.inner.stats()
        }
        #[cfg(not(windows))]
        {
            Stats::default()
        }
    }

    /// Stop capturing and finish the file.
    pub fn stop(self) -> Result<Stats> {
        #[cfg(windows)]
        {
            self.inner.stop()
        }
        #[cfg(not(windows))]
        {
            Err(Error::Unsupported)
        }
    }
}
