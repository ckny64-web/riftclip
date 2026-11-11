//! Drives the engine through whole matches with a fake recorder and (where a
//! recording is involved) the real ffmpeg.

use engine::{ActiveRecording, Engine, Notice, PostJob, RecorderBackend, Status};
use league::{AllGameData, EventType, Involvement, RawEvent};
use library::{AppSettings, AutoClipRules, Ffmpeg, Library};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

#[derive(Clone, Default)]
struct Fake {
    /// File copied to the output path when a recording starts.
    template: Option<PathBuf>,
    clock: Rc<Cell<f64>>,
    window_open: Rc<Cell<bool>>,
    fail: bool,
    starts: Rc<Cell<u32>>,
}

struct FakeRec(Fake);

impl RecorderBackend for Fake {
    fn start(&mut self, output: &Path, _: &AppSettings) -> Result<Box<dyn ActiveRecording>, String> {
        if self.fail {
            return Err("no hardware encoder".into());
        }
        if let Some(t) = &self.template {
            std::fs::copy(t, output).map_err(|e| e.to_string())?;
        }
        self.starts.set(self.starts.get() + 1);
        self.clock.set(0.0);
        self.window_open.set(true);
        Ok(Box::new(FakeRec(self.clone())))
    }
}

impl ActiveRecording for FakeRec {
    fn elapsed(&self) -> f64 {
        self.0.clock.get()
    }
    fn is_running(&self) -> bool {
        self.0.window_open.get()
    }
    fn stop(self: Box<Self>) -> Result<f64, String> {
        Ok(self.0.clock.get())
    }
}

fn ev(id: u32, name: &str, time: f64) -> RawEvent {
    RawEvent { event_id: id, event_name: name.into(), event_time: time, ..Default::default() }
}

/// The sample roster with a short scripted game: a double kill, a death, a win.
fn game_at(game_time: f64) -> AllGameData {
    let mut data = AllGameData::parse(include_str!("../../league/tests/fixtures/allgamedata_sample.json")).unwrap();
    let script = vec![
        ev(0, "GameStart", 0.0),
        RawEvent { killer_name: "Mid Me".into(), victim_name: "MidFoe".into(), ..ev(1, "ChampionKill", 10.0) },
        RawEvent { killer_name: "Mid Me".into(), victim_name: "JungleFoe".into(), ..ev(2, "ChampionKill", 12.0) },
        RawEvent { killer_name: "Mid Me".into(), kill_streak: 2, ..ev(3, "Multikill", 12.0) },
        RawEvent { killer_name: "JungleFoe".into(), victim_name: "Mid Me".into(), ..ev(4, "ChampionKill", 24.0) },
        RawEvent { result: "Win".into(), ..ev(5, "GameEnd", 27.0) },
    ];
    data.events.events = script.into_iter().filter(|e| e.event_time <= game_time).collect();
    data.game_data.game_time = game_time;
    data
}

fn have_ffmpeg() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|t| Command::new(t).arg("-version").output().is_ok_and(|o| o.status.success()))
}

/// A fragmented H.264/AAC file like the recorder writes: 60 fps, 1 s keyframes.
fn make_raw(path: &Path, seconds: u32) {
    let ok = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=60", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-t", &seconds.to_string(), "-c:v", "libx264", "-preset", "ultrafast", "-g", "60", "-keyint_min", "60", "-sc_threshold", "0", "-bf", "0", "-pix_fmt", "yuv420p"])
        .args(["-c:a", "aac", "-movflags", "frag_keyframe+empty_moov+default_base_moof"])
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

fn duration(path: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn engine_with(fake: &Fake, base: &Path, settings: &AppSettings) -> Engine {
    let lib = Library::open_in_memory().unwrap();
    lib.save_settings(settings).unwrap();
    Engine::new(lib, Box::new(fake.clone()), base.to_path_buf()).unwrap()
}

fn finished(notices: Vec<Notice>) -> Option<PostJob> {
    notices.into_iter().find_map(|n| match n {
        Notice::MatchFinished(job) => Some(job),
        _ => None,
    })
}

const NOW: i64 = 1_794_421_805; // 2026-11-11 18:30:05 UTC

#[test]
fn full_match_records_marks_events_and_cuts_clips() {
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg/ffprobe not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let template = dir.path().join("template.mp4");
    make_raw(&template, 34);
    let fake = Fake { template: Some(template), ..Default::default() };
    let settings = AppSettings {
        auto_clips: AutoClipRules { pad_before: 4.0, pad_after: 3.0, ..Default::default() },
        ..Default::default()
    };
    let base = dir.path().join("lib");
    let mut engine = engine_with(&fake, &base, &settings);

    // Game process exists but the API is not answering yet (loading screen).
    assert!(engine.tick(true, None, NOW).is_empty());
    assert_eq!(engine.status(), Status::Idle);
    assert_eq!(engine.mark(), None);

    // The API answers 4 s into the game: recording starts, so recording time = game time - 4.
    let mut job = None;
    let mut events_seen = Vec::new();
    for t in 0..=40 {
        fake.clock.set(t as f64);
        let notices = engine.tick(true, Some(&game_at(t as f64 + 4.0)), NOW + t);
        if t == 0 {
            assert!(matches!(&notices[0], Notice::MatchStarted { champion, recording: true, .. } if champion == "Ahri"));
        }
        if t == 15 {
            assert_eq!(engine.mark(), Some(15.0));
        }
        for n in &notices {
            if let Notice::Event { rec_time, event } = n {
                events_seen.push((event.ty, event.involvement, *rec_time));
            }
        }
        if let Some(j) = finished(notices) {
            assert_eq!(t, 31, "ends 8 s after GameEnd was seen at recording time 23");
            job = Some(j);
            break;
        }
    }
    let job = job.expect("the match finished");
    assert_eq!(engine.status(), Status::Idle);
    assert_eq!(
        events_seen,
        vec![
            (EventType::GameStart, Involvement::None, 0.0), // before the recording: clamped to its start
            (EventType::ChampionKill, Involvement::Killer, 6.0),
            (EventType::ChampionKill, Involvement::Killer, 8.0),
            (EventType::Multikill, Involvement::Killer, 8.0),
            (EventType::ChampionKill, Involvement::Victim, 20.0),
            (EventType::GameEnd, Involvement::None, 23.0),
        ]
    );
    assert_eq!(job.stem, "2026-11-11_18-30-05_Ahri");

    // Still on the end-of-game screen: must not start a second match.
    assert!(engine.tick(true, Some(&game_at(40.0)), NOW + 41).is_empty());
    assert_eq!(fake.starts.get(), 1);

    let report = job.run(engine.library_mut(), &Ffmpeg::locate(None)).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    let recording = report.recording.clone().unwrap();
    assert_eq!(recording, base.join("matches").join("2026-11-11_18-30-05_Ahri.mp4"));
    assert!(recording.is_file());
    assert!(!base.join("matches").join("2026-11-11_18-30-05_Ahri.rec.mp4").exists(), "raw file removed after remux");

    let lib = engine.library();
    let m = &lib.list_matches().unwrap()[0];
    assert_eq!((m.kills, m.deaths, m.assists, m.clip_count), (2, 1, 0, 2));
    assert_eq!(m.row.result, "win");
    assert_eq!(m.row.duration, 31.0);
    assert_eq!(m.row.champion, "Ahri");
    assert_eq!(m.row.recording_path.as_deref(), Some(recording.to_str().unwrap()));
    assert!(m.row.recording_bytes > 10_000);
    assert!(Path::new(m.row.thumbnail_path.as_ref().unwrap()).is_file());
    assert_eq!(lib.players(m.row.id).unwrap().len(), 10);
    assert_eq!(lib.players(m.row.id).unwrap().iter().filter(|p| p.is_me).count(), 1);
    let marks: Vec<f64> =
        lib.events(m.row.id).unwrap().iter().filter(|e| e.ty == EventType::Manual).map(|e| e.rec_time).collect();
    assert_eq!(marks, vec![15.0]);

    let clips = lib.clips(Some(m.row.id)).unwrap();
    assert_eq!(clips.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["Double kill by Ahri", "Lee Sin killed Ahri"]);
    assert_eq!((clips[0].start, clips[0].end), (2.0, 11.0));
    assert_eq!((clips[1].start, clips[1].end), (16.0, 23.0));
    assert!(clips.iter().all(|c| c.auto));
    for (clip, want) in clips.iter().zip([9.0, 7.0]) {
        let d = duration(Path::new(&clip.path));
        assert!((want - 0.2..=want + 1.1).contains(&d), "{}: {d} s, wanted about {want}", clip.title);
        assert!(Path::new(clip.thumbnail_path.as_ref().unwrap()).is_file());
    }
    assert!(clips[0].path.ends_with("01_Double-kill-by-Ahri.mp4"));

    // The game closes; the next match starts a new recording, and closing the
    // game window mid-match ends it.
    assert!(engine.tick(false, None, NOW + 50).is_empty());
    let n = engine.tick(true, Some(&game_at(5.0)), NOW + 100);
    assert!(matches!(n[0], Notice::MatchStarted { recording: true, .. }));
    assert_eq!(fake.starts.get(), 2);
    fake.clock.set(3.0);
    fake.window_open.set(false);
    let job2 = finished(engine.tick(true, Some(&game_at(8.0)), NOW + 103)).expect("ends when the window closes");
    assert_eq!(job2.duration, 3.0);
}

#[test]
fn match_is_tracked_when_recording_cannot_start() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake { fail: true, ..Default::default() };
    let mut engine = engine_with(&fake, dir.path(), &AppSettings::default());

    let notices = engine.tick(true, Some(&game_at(11.0)), NOW);
    assert!(matches!(&notices[0], Notice::RecordingFailed(e) if e == "no hardware encoder"));
    assert!(matches!(notices[1], Notice::MatchStarted { recording: false, .. }));
    assert!(matches!(engine.status(), Status::InGame { recording: false, .. }));
    assert!(engine.live().is_some());

    // Game process exits.
    let job = finished(engine.tick(false, None, NOW + 60)).unwrap();
    assert!(job.raw_path.is_none());
    let report = job.run(engine.library_mut(), &Ffmpeg::new("ffmpeg-not-needed")).unwrap();
    assert_eq!(report.recording, None);
    assert!(report.clips.is_empty() && report.warnings.is_empty());

    let m = &engine.library().list_matches().unwrap()[0];
    assert_eq!(m.kills, 1); // the kill at game time 10 was still stored
    assert_eq!(m.row.recording_path, None);
}

#[test]
fn recording_can_be_switched_off() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let settings = AppSettings { record_enabled: false, ..Default::default() };
    let mut engine = engine_with(&fake, dir.path(), &settings);
    let notices = engine.tick(true, Some(&game_at(1.0)), NOW);
    assert!(matches!(notices[0], Notice::MatchStarted { recording: false, .. }));
    assert_eq!(fake.starts.get(), 0);
    assert!(engine.stop(NOW + 5).is_some());
    assert!(engine.stop(NOW + 6).is_none());
}

#[test]
fn spectating_without_a_local_player_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine_with(&Fake::default(), dir.path(), &AppSettings::default());
    let mut data = game_at(5.0);
    data.active_player = Default::default();
    assert!(engine.tick(true, Some(&data), NOW).is_empty());
    assert_eq!(engine.status(), Status::Idle);
}

#[test]
fn retention_removes_old_recordings_but_not_the_new_one() {
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg/ffprobe not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let template = dir.path().join("template.mp4");
    make_raw(&template, 4);
    let fake = Fake { template: Some(template), ..Default::default() };
    let settings = AppSettings { retention_days: 14, ..Default::default() };
    let mut engine = engine_with(&fake, &dir.path().join("lib"), &settings);

    // A match from 30 days ago with a recording still on disk.
    let old_file = dir.path().join("old.mp4");
    std::fs::write(&old_file, b"old recording").unwrap();
    let old = engine
        .library()
        .insert_match(&library::NewMatch { started_at: NOW - 30 * 86_400, recording_path: Some(old_file.to_string_lossy().into()), ..Default::default() })
        .unwrap();
    engine.library().set_recording(old, Some(old_file.to_str().unwrap()), 13).unwrap();

    engine.tick(true, Some(&game_at(1.0)), NOW);
    fake.clock.set(4.0);
    let job = finished(engine.tick(false, None, NOW + 4)).unwrap();
    let report = job.run(engine.library_mut(), &Ffmpeg::locate(None)).unwrap();

    assert_eq!(report.recordings_deleted, 1);
    assert!(!old_file.exists());
    assert_eq!(engine.library().get_match(old).unwrap().recording_path, None);
    assert!(report.recording.unwrap().is_file());
}
