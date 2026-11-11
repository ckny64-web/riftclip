//! The real recorder, wired to the engine's traits.

use crate::session::{ActiveRecording, RecorderBackend};
use library::AppSettings;
use recorder::{Recorder, RecordingHandle, Settings, Target};
use std::path::Path;

/// Records the League game window.
#[derive(Default)]
pub struct GameRecorder;

impl RecorderBackend for GameRecorder {
    fn start(&mut self, output: &Path, settings: &AppSettings) -> Result<Box<dyn ActiveRecording>, String> {
        let rec = Settings { fps: settings.fps, bitrate_kbps: settings.bitrate_kbps, ..Settings::default() };
        Recorder::start(&Target::ProcessName(league::GAME_PROCESS.to_string()), output, &rec)
            .map(|h| Box::new(Active(h)) as Box<dyn ActiveRecording>)
            .map_err(|e| e.to_string())
    }
}

struct Active(RecordingHandle);

impl ActiveRecording for Active {
    fn elapsed(&self) -> f64 {
        self.0.elapsed().as_secs_f64()
    }
    fn is_running(&self) -> bool {
        self.0.is_running()
    }
    fn stop(self: Box<Self>) -> Result<f64, String> {
        self.0.stop().map(|s| s.duration.as_secs_f64()).map_err(|e| e.to_string())
    }
}
