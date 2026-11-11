//! Headless runner: watches for League, records each match, stores events and
//! cuts clips. Press Ctrl+C to quit (a match in progress is finished cleanly).
//!
//!   cargo run -p engine --release --bin riftclip-cli -- [--dir FOLDER]
//!
//! Files go to FOLDER (default: Videos\Riftclip in your user folder).

use engine::overlay::Phase;
use engine::real::GameRecorder;
use engine::runner::{default_dir, Command, Runner, Update};
use std::path::PathBuf;
use std::sync::mpsc::channel;

fn clock(secs: f64) -> String {
    format!("{:02}:{:02}", secs as u64 / 60, secs as u64 % 60)
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
    let runner = Runner::new(dir.clone(), Box::new(GameRecorder)).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    println!("Riftclip is watching for League of Legends. Files go to {}", dir.display());
    if !runner.ffmpeg_available() {
        println!("Warning: ffmpeg was not found. Matches will still be recorded, but not finalized or cut into clips.");
    }
    println!("Press Ctrl+C to quit.");

    let (tx, rx) = channel();
    let _ = ctrlc::set_handler(move || {
        println!("Stopping ...");
        let _ = tx.send(Command::Quit);
    });

    runner.run(rx, &mut |update| match update {
        Update::Phase(Phase::Loading) => println!("Game detected, waiting for it to load ..."),
        Update::Phase(Phase::Idle) => println!("No game running."),
        Update::Phase(Phase::InGame) => {}
        Update::Overlay(_) | Update::Marked(_) => {}
        Update::MatchStarted { champion, recording, .. } => {
            println!("Match started as {champion}. Recording: {}", if recording { "on" } else { "off" })
        }
        Update::RecordingFailed(e) => println!("Recording could not start: {e}"),
        Update::Event { rec_time, detail, involvement } => println!("  [{}] {detail} ({})", clock(rec_time), involvement.as_str()),
        Update::PostStarted { duration } => println!("Match over. Finishing the recording ({} long) ...", clock(duration)),
        Update::PostFinished(Ok(r)) => {
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
        Update::PostFinished(Err(e)) => println!("Could not finish the match files: {e}"),
        Update::Info(s) => println!("{s}"),
        Update::Error(e) => println!("Error: {e}"),
    });
}
