//! Milestone 1 capture spike: record a window (and that process's audio) to a file.
//!
//!   cargo run -p recorder --release --example spike -- [options]
//!
//!   --process NAME   executable to record (default "League of Legends.exe")
//!   --out PATH       output file (default "spike.mp4")
//!   --seconds N      stop after N seconds (default: until Enter is pressed)
//!   --fps N          output frame rate (default 60)
//!   --bitrate KBPS   video bitrate (default 20000)
//!   --no-audio       record video only

use recorder::{Recorder, Settings, Target};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let mut process = "League of Legends.exe".to_string();
    let mut out = PathBuf::from("spike.mp4");
    let mut seconds: Option<u64> = None;
    let mut settings = Settings::default();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().unwrap_or_else(|| fail(&format!("{name} needs a value")));
        match arg.as_str() {
            "--process" => process = value("--process"),
            "--out" => out = PathBuf::from(value("--out")),
            "--seconds" => seconds = Some(parse(&value("--seconds"), "--seconds")),
            "--fps" => settings.fps = parse(&value("--fps"), "--fps"),
            "--bitrate" => settings.bitrate_kbps = parse(&value("--bitrate"), "--bitrate"),
            "--no-audio" => settings.audio = false,
            other => fail(&format!("unknown option {other}")),
        }
    }

    println!("Looking for a window of {process} ...");
    let handle = match Recorder::start(&Target::ProcessName(process), &out, &settings) {
        Ok(h) => h,
        Err(e) => fail(&format!("could not start recording: {e}")),
    };
    let s = handle.stats();
    println!("Recording {}x{} at {} fps, {} kbps -> {}", s.width, s.height, settings.fps, settings.bitrate_kbps, out.display());
    match (&s.audio, &s.audio_error) {
        (true, _) => println!("Audio: process audio only"),
        (false, Some(e)) => println!("Audio: OFF, capture failed: {e}"),
        (false, None) => println!("Audio: off"),
    }

    let stop = Arc::new(AtomicBool::new(false));
    if seconds.is_none() {
        println!("Press Enter to stop.");
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            stop.store(true, Ordering::SeqCst);
        });
    }

    let started = Instant::now();
    let mut last_report = Instant::now();
    let mut last_frames = 0u64;
    loop {
        std::thread::sleep(Duration::from_millis(200));
        if stop.load(Ordering::SeqCst) || seconds.is_some_and(|s| started.elapsed().as_secs() >= s) {
            break;
        }
        if !handle.is_running() {
            println!("The window closed.");
            break;
        }
        if last_report.elapsed() >= Duration::from_secs(5) {
            let s = handle.stats();
            let fps = (s.frames_written - last_frames) as f64 / last_report.elapsed().as_secs_f64();
            println!(
                "{:>5.0}s  {:.1} fps written  (total {} written, {} over-rate skipped, {} dropped)",
                s.duration.as_secs_f64(),
                fps,
                s.frames_written,
                s.frames_paced_out,
                s.frames_dropped
            );
            last_frames = s.frames_written;
            last_report = Instant::now();
        }
    }

    match handle.stop() {
        Ok(s) => {
            let secs = s.duration.as_secs_f64().max(0.001);
            println!(
                "Done: {:.1}s, {} frames ({:.1} fps average), {} dropped by the encoder.",
                secs,
                s.frames_written,
                s.frames_written as f64 / secs,
                s.frames_dropped
            );
            println!("Saved {}", out.display());
        }
        Err(e) => fail(&format!("could not finish the file: {e}")),
    }
}

fn parse<T: std::str::FromStr>(v: &str, name: &str) -> T {
    v.parse().unwrap_or_else(|_| fail(&format!("{name}: '{v}' is not a number")))
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1)
}
