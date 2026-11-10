mod audio;
mod capture;
mod encoder;
mod window;

use crate::{Error, Result, Settings, Stats, Target};
use encoder::Encoder;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

pub(crate) trait Ctx<T> {
    fn ctx(self, context: &'static str) -> Result<T>;
}

impl<T> Ctx<T> for windows::core::Result<T> {
    fn ctx(self, context: &'static str) -> Result<T> {
        self.map_err(|source| Error::Windows { context, source })
    }
}

/// Current time in 100 ns units on the same clock Windows Graphics Capture
/// stamps frames with (`QueryPerformanceCounter`).
pub(crate) fn qpc_100ns() -> i64 {
    let (mut count, mut freq) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut count);
        let _ = QueryPerformanceFrequency(&mut freq);
    }
    if freq <= 0 {
        return 0;
    }
    ((count as i128) * 10_000_000 / (freq as i128)) as i64
}

/// State shared by the capture callback, the audio thread and the handle.
pub(crate) struct Shared {
    pub encoder: Encoder,
    /// Recording start on the QPC clock, 100 ns units.
    pub start: i64,
    pub stop: AtomicBool,
    pub window_closed: AtomicBool,
    pub frames_written: AtomicU64,
    pub frames_paced_out: AtomicU64,
    pub frames_dropped: AtomicU64,
}

pub(crate) struct Recording {
    shared: Arc<Shared>,
    capture: capture::Capture,
    audio: Option<audio::AudioThread>,
    audio_error: Option<String>,
    width: u32,
    height: u32,
}

pub(crate) fn start(target: &Target, output: &Path, settings: &Settings) -> Result<Recording> {
    // Fails harmlessly if the thread already has a COM apartment.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    let found = window::find(target)?;
    let device = capture::create_device()?;
    let source = capture::Source::open(&device, found.hwnd)?;
    let (width, height) = source.even_size();

    // Open the audio device before the encoder so we know whether the file
    // gets an audio track.
    let (pending_audio, audio_error) = if settings.audio {
        match audio::AudioThread::open(found.pid) {
            Ok(a) => (Some(a), None),
            Err(e) => (None, Some(e)),
        }
    } else {
        (None, None)
    };

    let encoder = Encoder::new(&device, output, width, height, settings, pending_audio.is_some())?;
    let shared = Arc::new(Shared {
        encoder,
        start: qpc_100ns(),
        stop: AtomicBool::new(false),
        window_closed: AtomicBool::new(false),
        frames_written: AtomicU64::new(0),
        frames_paced_out: AtomicU64::new(0),
        frames_dropped: AtomicU64::new(0),
    });

    let audio = pending_audio.map(|a| a.begin(shared.clone()));
    let capture = source.start(shared.clone(), settings)?;

    Ok(Recording { shared, capture, audio, audio_error, width, height })
}

impl Recording {
    pub fn elapsed(&self) -> Duration {
        Duration::from_nanos(((qpc_100ns() - self.shared.start).max(0) as u64) * 100)
    }

    pub fn is_running(&self) -> bool {
        !self.shared.window_closed.load(Ordering::Relaxed)
    }

    pub fn stats(&self) -> Stats {
        Stats {
            width: self.width,
            height: self.height,
            frames_written: self.shared.frames_written.load(Ordering::Relaxed),
            frames_paced_out: self.shared.frames_paced_out.load(Ordering::Relaxed),
            frames_dropped: self.shared.frames_dropped.load(Ordering::Relaxed),
            audio: self.audio.is_some(),
            audio_error: self.audio_error.clone(),
            duration: self.elapsed(),
        }
    }

    pub fn stop(self) -> Result<Stats> {
        let stats = self.stats();
        self.shared.stop.store(true, Ordering::SeqCst);
        self.capture.close();
        if let Some(a) = self.audio {
            a.join();
        }
        self.shared.encoder.finish()?;
        Ok(stats)
    }
}
