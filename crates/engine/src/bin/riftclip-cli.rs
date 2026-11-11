//! Headless runner: watches for League, records each match, stores events and
//! cuts clips. Press Ctrl+C to quit (a match in progress is finished cleanly).
//!
//!   cargo run -p engine --release --bin riftclip-cli -- [--dir FOLDER]
//!
//! Files go to FOLDER (default: Videos\Riftclip in your user folder).

use engine::{real::GameRecorder, Engine, Notice, PostJob};
use league::client::LiveClient;
use league::process::ProcessWatcher;
use library::{Ffmpeg, Library};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn default_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    home.unwrap_or_else(|| PathBuf::from(".")).join("Videos").join("Riftclip")
}

fn clock(secs: f64) -> String {
    format!("{:02}:{:02}", secs as u64 / 60, secs as u64 % 60)
}

/// Finish a match's files on a separate thread with its own database connection.
fn post_process(job: PostJob, db: &Path, ffmpeg: &Ffmpeg) -> JoinHandle<()> {
    let (db, ffmpeg) = (db.to_path_buf(), ffmpeg.clone());
    std::thread::spawn(move || {
        println!("Finishing the recording ({} long) ...", clock(job.duration));
        let report = Library::open(&db).and_then(|mut lib| job.run(&mut lib, &ffmpeg));
        match report {
            Ok(r) => {
                match &r.recording {
                    Some(p) => println!("Recording saved: {}", p.display()),
                    None => println!("No recording for this match."),
                }
                for c in &r.clips {
                    println!("Clip: {}", c.display());
                }
                if r.recordings_deleted > 0 {
                    println!("Deleted {} old recording(s).", r.recordings_deleted);
                }
                for w in &r.warnings {
                    println!("Warning: {w}");
                }
            }
            Err(e) => println!("Could not finish the match files: {e}"),
        }
    })
}

fn main() {
    let mut dir = default_dir();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dir" => dir = PathBuf::from(args.next().expect("--dir needs a folder")),
            other => {
                eprintln!("unknown option {other}");
                std::process::exit(1);
            }
        }
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {}: {e}", dir.display());
        std::process::exit(1);
    }
    let db = dir.join("riftclip.db");
    let lib = Library::open(&db).unwrap_or_else(|e| {
        eprintln!("cannot open {}: {e}", db.display());
        std::process::exit(1);
    });
    let mut engine = Engine::new(lib, Box::new(GameRecorder), dir.clone()).expect("load settings");
    let ffmpeg = Ffmpeg::locate(engine.settings().ffmpeg_path.as_deref());

    println!("Riftclip is watching for League of Legends. Files go to {}", dir.display());
    if !ffmpeg.is_available() {
        println!("Warning: ffmpeg was not found. Matches will still be recorded, but not finalized or cut into clips.");
    }
    println!("Press Ctrl+C to quit.");

    let quit = Arc::new(AtomicBool::new(false));
    {
        let quit = quit.clone();
        let _ = ctrlc::set_handler(move || quit.store(true, Ordering::SeqCst));
    }

    let mut watcher = ProcessWatcher::new();
    let live = LiveClient::new();
    let mut jobs: Vec<JoinHandle<()>> = Vec::new();
    let mut game_running = false;
    let mut tick = 0u64;

    while !quit.load(Ordering::SeqCst) {
        // Looking for the process is the expensive part; do it every other second.
        if tick.is_multiple_of(2) {
            let running = watcher.game_pid().is_some();
            if running != game_running {
                println!("{}", if running { "Game detected, waiting for it to load ..." } else { "Game closed." });
            }
            game_running = running;
        }
        tick += 1;

        let data = if game_running { live.all_game_data().ok() } else { None };
        for notice in engine.tick(game_running, data.as_ref(), now_unix()) {
            match notice {
                Notice::MatchStarted { champion, recording, .. } => {
                    println!("Match started as {champion}. Recording: {}", if recording { "on" } else { "off" })
                }
                Notice::RecordingFailed(e) => println!("Recording could not start: {e}"),
                Notice::Event { rec_time, event } => {
                    println!("  [{}] {} ({})", clock(rec_time), event.detail, event.involvement.as_str())
                }
                Notice::MatchFinished(job) => jobs.push(post_process(job, &db, &ffmpeg)),
                Notice::Error(e) => println!("Error: {e}"),
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    println!("Stopping ...");
    if let Some(job) = engine.stop(now_unix()) {
        jobs.push(post_process(job, &db, &ffmpeg));
    }
    for j in jobs {
        let _ = j.join();
    }
}
