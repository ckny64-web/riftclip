use crate::naming::{matches_dir, path_string, timestamp};
use crate::post::PostJob;
use league::{classify, AllGameData, EventTracker, EventType, GameEvent, Involvement, Roster, TimeMap};
use library::{AppSettings, Library, NewEvent, NewMatch, PlayerRow};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Starts recordings. Implemented by the real recorder and by test fakes.
pub trait RecorderBackend {
    fn start(&mut self, output: &Path, settings: &AppSettings) -> Result<Box<dyn ActiveRecording>, String>;
}

pub trait ActiveRecording {
    /// Seconds since the recording began.
    fn elapsed(&self) -> f64;
    /// False once the captured window is gone.
    fn is_running(&self) -> bool;
    /// Finish the file; returns its length in seconds.
    fn stop(self: Box<Self>) -> Result<f64, String>;
}

/// Stand-in clock used when recording is off or failed to start, so events
/// still get a consistent timeline.
struct NoRecording(Instant);

impl ActiveRecording for NoRecording {
    fn elapsed(&self) -> f64 {
        self.0.elapsed().as_secs_f64()
    }
    fn is_running(&self) -> bool {
        true
    }
    fn stop(self: Box<Self>) -> Result<f64, String> {
        Ok(self.0.elapsed().as_secs_f64())
    }
}

/// Things the engine reports back from `tick`.
#[derive(Debug)]
pub enum Notice {
    MatchStarted { match_id: i64, champion: String, recording: bool },
    /// Recording was wanted but could not start; the match is still tracked.
    RecordingFailed(String),
    Event { rec_time: f64, event: GameEvent },
    /// The match is over. Run the job (off the polling thread) to finish the files.
    MatchFinished(PostJob),
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    InGame { match_id: i64, recording: bool, elapsed: f64 },
}

struct Session {
    match_id: i64,
    started_at: i64,
    stem: String,
    recording: Box<dyn ActiveRecording>,
    /// The raw file being written, if recording is on.
    raw_path: Option<PathBuf>,
    timemap: TimeMap,
    tracker: EventTracker,
    last_data: AllGameData,
    result: &'static str,
    /// Recording time at which the game reported it was over.
    game_end_at: Option<f64>,
}

/// How long to keep recording after the game reports its end (nexus explosion).
const END_GRACE_SECS: f64 = 8.0;

pub struct Engine {
    lib: Library,
    backend: Box<dyn RecorderBackend>,
    base_dir: PathBuf,
    settings: AppSettings,
    session: Option<Session>,
    /// After a match ends, wait for the game process to exit before starting
    /// another one (the API keeps answering on the end-of-game screen).
    await_exit: bool,
}

impl Engine {
    pub fn new(lib: Library, backend: Box<dyn RecorderBackend>, base_dir: PathBuf) -> library::Result<Self> {
        let settings = lib.settings()?;
        Ok(Engine { lib, backend, base_dir, settings, session: None, await_exit: false })
    }

    pub fn library(&self) -> &Library {
        &self.lib
    }

    pub fn library_mut(&mut self) -> &mut Library {
        &mut self.lib
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// Re-read settings from the database (they apply from the next match).
    pub fn reload_settings(&mut self) -> library::Result<()> {
        self.settings = self.lib.settings()?;
        Ok(())
    }

    pub fn status(&self) -> Status {
        match &self.session {
            None => Status::Idle,
            Some(s) => Status::InGame {
                match_id: s.match_id,
                recording: s.raw_path.is_some(),
                elapsed: s.recording.elapsed(),
            },
        }
    }

    /// The most recent game state, for the overlay.
    pub fn live(&self) -> Option<&AllGameData> {
        self.session.as_ref().map(|s| &s.last_data)
    }

    /// Advance the engine. Call about once a second with whether the game
    /// process exists and, if the live API answered, its data.
    pub fn tick(&mut self, game_running: bool, live: Option<&AllGameData>, now_unix: i64) -> Vec<Notice> {
        let mut notices = Vec::new();
        if !game_running {
            self.await_exit = false;
        }

        if self.session.is_none() {
            if let (true, false, Some(data)) = (game_running, self.await_exit, live) {
                if data.me().is_some() {
                    if let Err(e) = self.begin(data, now_unix, &mut notices) {
                        notices.push(Notice::Error(format!("could not start tracking the match: {e}")));
                    }
                }
            }
        }

        if let Some(session) = self.session.as_mut() {
            if let Some(data) = live {
                if let Err(e) = Self::ingest(&self.lib, session, data, &mut notices) {
                    notices.push(Notice::Error(format!("could not store events: {e}")));
                }
            }
            let now = session.recording.elapsed();
            let over = !game_running
                || !session.recording.is_running()
                || session.game_end_at.is_some_and(|t| now >= t + END_GRACE_SECS);
            if over {
                self.await_exit = game_running;
                match self.finish(now_unix) {
                    Ok(job) => notices.push(Notice::MatchFinished(job)),
                    Err(e) => notices.push(Notice::Error(format!("could not finish the match: {e}"))),
                }
            }
        }
        notices
    }

    fn begin(&mut self, data: &AllGameData, now_unix: i64, notices: &mut Vec<Notice>) -> library::Result<()> {
        let champion = data.me().map(|p| p.champion_name.clone()).unwrap_or_default();
        let stem = format!("{}_{}", timestamp(now_unix), crate::naming::slug(&champion));

        let mut raw_path = None;
        let mut recording: Box<dyn ActiveRecording> = Box::new(NoRecording(Instant::now()));
        if self.settings.record_enabled {
            let dir = matches_dir(&self.base_dir);
            let path = dir.join(format!("{stem}.rec.mp4"));
            let started = std::fs::create_dir_all(&dir)
                .map_err(|e| format!("create {}: {e}", dir.display()))
                .and_then(|_| self.backend.start(&path, &self.settings));
            match started {
                Ok(r) => {
                    recording = r;
                    raw_path = Some(path);
                }
                Err(e) => notices.push(Notice::RecordingFailed(e)),
            }
        }

        let match_id = self.lib.insert_match(&NewMatch {
            started_at: now_unix,
            champion: champion.clone(),
            game_mode: data.game_data.game_mode.clone(),
            recording_path: raw_path.as_deref().map(path_string),
        })?;
        self.lib.set_players(match_id, &player_rows(data))?;

        notices.push(Notice::MatchStarted { match_id, champion, recording: raw_path.is_some() });
        self.session = Some(Session {
            match_id,
            started_at: now_unix,
            stem,
            recording,
            raw_path,
            timemap: TimeMap::new(),
            tracker: EventTracker::new(),
            last_data: data.clone(),
            result: "",
            game_end_at: None,
        });
        Ok(())
    }

    fn ingest(lib: &Library, s: &mut Session, data: &AllGameData, notices: &mut Vec<Notice>) -> library::Result<()> {
        let now = s.recording.elapsed();
        s.timemap.observe(now, data.game_data.game_time);
        let roster = Roster::from_game(data);
        for raw in s.tracker.take_new(&data.events.events) {
            let Some(event) = classify(raw, &roster) else { continue };
            // Events from before the recording began (joining late) clamp to 0.
            let rec_time = s.timemap.to_recording(event.game_time).unwrap_or(now).min(now);
            if event.ty == EventType::GameEnd {
                s.game_end_at = Some(now);
                s.result = match raw.result.as_str() {
                    "Win" => "win",
                    "Lose" => "loss",
                    _ => "",
                };
            }
            lib.insert_event(
                s.match_id,
                &NewEvent {
                    game_event_id: Some(event.id as i64),
                    ty: event.ty,
                    involvement: event.involvement,
                    rec_time,
                    game_time: event.game_time,
                    detail: event.detail.clone(),
                },
            )?;
            notices.push(Notice::Event { rec_time, event });
        }
        s.last_data = data.clone();
        Ok(())
    }

    /// Add a manual marker at the current moment. Returns its recording time,
    /// or `None` when no match is being tracked.
    pub fn mark(&mut self) -> Option<f64> {
        let s = self.session.as_ref()?;
        let rec_time = s.recording.elapsed();
        let game_time = s.last_data.game_data.game_time;
        self.lib
            .insert_event(
                s.match_id,
                &NewEvent {
                    game_event_id: None,
                    ty: EventType::Manual,
                    involvement: Involvement::None,
                    rec_time,
                    game_time,
                    detail: "Manual mark".into(),
                },
            )
            .ok()?;
        Some(rec_time)
    }

    /// Stop the current session now (used on shutdown). Returns the job to run.
    pub fn stop(&mut self, now_unix: i64) -> Option<PostJob> {
        self.session.as_ref()?;
        self.finish(now_unix).ok()
    }

    fn finish(&mut self, now_unix: i64) -> library::Result<PostJob> {
        let s = self.session.take().expect("finish is only called with a session");
        let fallback = s.recording.elapsed();
        let (duration, stop_error) = match s.recording.stop() {
            Ok(d) => (d, None),
            Err(e) => (fallback, Some(e)),
        };
        // Final scoreboard as last seen.
        self.lib.set_players(s.match_id, &player_rows(&s.last_data))?;
        self.lib.finish_match(s.match_id, duration, s.result)?;
        Ok(PostJob {
            match_id: s.match_id,
            started_at: s.started_at,
            stem: s.stem,
            raw_path: s.raw_path,
            duration,
            base_dir: self.base_dir.clone(),
            rules: self.settings.auto_clips.clone(),
            retention_days: self.settings.retention_days,
            max_recordings_gb: self.settings.max_recordings_gb,
            now_unix,
            stop_error,
        })
    }
}

fn player_rows(data: &AllGameData) -> Vec<PlayerRow> {
    let me = data.active_player.names();
    data.all_players
        .iter()
        .map(|p| PlayerRow {
            team: p.team.clone(),
            is_me: [&p.riot_id_game_name, &p.summoner_name, &p.riot_id].into_iter().any(|n| me.matches(n)),
            name: p.display_name().to_string(),
            champion: p.champion_name.clone(),
            position: p.position.clone(),
            kills: p.scores.kills as i64,
            deaths: p.scores.deaths as i64,
            assists: p.scores.assists as i64,
            cs: p.scores.creep_score as i64,
            ..Default::default()
        })
        .collect()
}
