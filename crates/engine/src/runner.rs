//! The polling loop shared by the command-line runner and the tray app:
//! watches for the game, drives the engine, fetches ranks from the League
//! client, feeds the overlay, and finishes match files in the background.

use crate::overlay::{OverlayState, Phase};
use crate::post::{PostJob, PostReport};
use crate::session::{Engine, Notice, RecorderBackend, Status};
use league::client::{LcuClient, LiveClient};
use league::ddragon::StaticData;
use league::process::ProcessWatcher;
use league::ranks::{self, ClientPlayer};
use league::{gold_report, Involvement};
use library::{Ffmpeg, Library};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Requests from the user interface.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Add a manual marker to the current match.
    Mark,
    /// Settings were changed in the database.
    ReloadSettings,
    /// The show-ranks key went down (`true`) or up (`false`).
    HoldRanks(bool),
    ToggleGold,
    /// Finish any match in progress and return from `run`.
    Quit,
}

/// What the runner reports back.
#[derive(Debug)]
pub enum Update {
    Phase(Phase),
    /// Forward to the overlay process, if one is running.
    Overlay(proto::Message),
    MatchStarted { match_id: i64, champion: String, recording: bool },
    RecordingFailed(String),
    Event { rec_time: f64, detail: String, involvement: Involvement },
    /// Result of a `Mark` command: the recording time, or `None` outside a match.
    Marked(Option<f64>),
    /// A match ended and its files are being finished.
    PostStarted { duration: f64 },
    PostFinished(Result<PostReport, String>),
    Info(String),
    Error(String),
}

enum Internal {
    Static(Box<StaticData>),
    Client { players: Vec<ClientPlayer>, my_puuid: String, queue: String },
    Post(Result<PostReport, String>),
}

pub struct Runner {
    engine: Engine,
    base_dir: PathBuf,
    db_path: PathBuf,
    ffmpeg: Ffmpeg,
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// `Videos\Riftclip` in the user's profile.
pub fn default_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    home.unwrap_or_else(|| PathBuf::from(".")).join("Videos").join("Riftclip")
}

pub fn db_path(base_dir: &Path) -> PathBuf {
    base_dir.join("riftclip.db")
}

/// Ask the League client who is in the game. Retries for a while because the
/// client fills the session in shortly after the game launches.
fn fetch_client_info(tx: Sender<Internal>) {
    std::thread::spawn(move || {
        let mut watcher = ProcessWatcher::new();
        for _ in 0..20 {
            if let Some(creds) = watcher.client_credentials() {
                let client = LcuClient::new(&creds);
                if let Ok((session, players)) = client.game_players() {
                    if !players.is_empty() {
                        let queue = if session.queue_description.is_empty() {
                            league::lcu::queue_name(session.queue_id).to_string()
                        } else {
                            session.queue_description
                        };
                        let my_puuid = client.my_puuid().unwrap_or_default();
                        let _ = tx.send(Internal::Client { players, my_puuid, queue });
                        return;
                    }
                }
            }
            if watcher.game_pid().is_none() {
                return; // game closed while we were waiting
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

impl Runner {
    pub fn new(base_dir: PathBuf, backend: Box<dyn RecorderBackend>) -> Result<Self, String> {
        std::fs::create_dir_all(&base_dir).map_err(|e| format!("cannot create {}: {e}", base_dir.display()))?;
        let db_path = db_path(&base_dir);
        let lib = Library::open(&db_path).map_err(|e| format!("cannot open {}: {e}", db_path.display()))?;
        let engine = Engine::new(lib, backend, base_dir.clone()).map_err(|e| format!("cannot load settings: {e}"))?;
        let ffmpeg = Ffmpeg::locate(engine.settings().ffmpeg_path.as_deref());
        Ok(Runner { engine, base_dir, db_path, ffmpeg })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn ffmpeg_available(&self) -> bool {
        self.ffmpeg.is_available()
    }

    fn post(&self, job: PostJob, tx: &Sender<Internal>) -> JoinHandle<()> {
        let (db, ffmpeg, tx) = (self.db_path.clone(), self.ffmpeg.clone(), tx.clone());
        std::thread::spawn(move || {
            let result = Library::open(&db).and_then(|mut lib| job.run(&mut lib, &ffmpeg)).map_err(|e| e.to_string());
            let _ = tx.send(Internal::Post(result));
        })
    }

    /// Run until `Quit` arrives or the command channel closes.
    pub fn run(mut self, commands: Receiver<Command>, on_update: &mut dyn FnMut(Update)) {
        let (tx, internal) = channel::<Internal>();
        let started = Instant::now();
        let now = || started.elapsed().as_secs_f64();

        let mut watcher = ProcessWatcher::new();
        let live_client = LiveClient::new();
        let mut overlay = OverlayState::new(self.engine.settings().overlay.clone());
        let mut statics = StaticData::default();
        let mut client_players: Vec<ClientPlayer> = Vec::new();
        let mut my_puuid = String::new();
        let mut queue = String::new();
        let mut client_requested = false;
        let mut match_over = false; // match finished but the game is still open
        let mut game_running = false;
        let mut jobs: Vec<JoinHandle<()>> = Vec::new();
        let mut pending_posts = 0usize;
        let mut last_poll = Instant::now() - Duration::from_secs(10);
        let mut polls = 0u64;

        // Champion names and item prices; the app works without them.
        {
            let (tx, cache) = (tx.clone(), self.base_dir.join("ddragon"));
            std::thread::spawn(move || {
                if let Some(data) = league::ddragon::load(&cache) {
                    let _ = tx.send(Internal::Static(Box::new(data)));
                }
            });
        }

        macro_rules! overlay_out {
            ($msgs:expr) => {
                for m in $msgs {
                    on_update(Update::Overlay(m));
                }
            };
        }

        'main: loop {
            // ---- commands (also paces the loop: wakes at least every 200 ms)
            match commands.recv_timeout(Duration::from_millis(200)) {
                Ok(Command::Quit) | Err(RecvTimeoutError::Disconnected) => break 'main,
                Ok(Command::Mark) => on_update(Update::Marked(self.engine.mark())),
                Ok(Command::HoldRanks(held)) => overlay_out!(overlay.hold_ranks(held, now())),
                Ok(Command::ToggleGold) => overlay_out!(overlay.toggle_gold(now())),
                Ok(Command::ReloadSettings) => {
                    if let Err(e) = self.engine.reload_settings() {
                        on_update(Update::Error(format!("could not reload settings: {e}")));
                    }
                    self.ffmpeg = Ffmpeg::locate(self.engine.settings().ffmpeg_path.as_deref());
                    overlay_out!(overlay.set_config(self.engine.settings().overlay.clone(), now()));
                }
                Err(RecvTimeoutError::Timeout) => {}
            }

            // ---- results from background threads
            while let Ok(msg) = internal.try_recv() {
                match msg {
                    Internal::Static(data) => {
                        on_update(Update::Info(format!("Loaded game data for patch {}", data.version)));
                        statics = *data;
                    }
                    Internal::Client { players, my_puuid: me, queue: q } => {
                        client_players = players;
                        my_puuid = me;
                        queue = q;
                        let rows = ranks::join(&client_players, self.engine.live(), &statics.champions, &my_puuid);
                        overlay_out!(overlay.set_ranks(&rows, now()));
                        if let Err(e) = self.engine.set_client_info(rows, queue.clone()) {
                            on_update(Update::Error(format!("could not store ranks: {e}")));
                        }
                    }
                    Internal::Post(result) => {
                        pending_posts = pending_posts.saturating_sub(1);
                        on_update(Update::PostFinished(result));
                    }
                }
            }

            // ---- game polling, once a second
            if last_poll.elapsed() >= Duration::from_secs(1) {
                last_poll = Instant::now();
                // Listing processes is the expensive part; every other second is enough.
                if polls.is_multiple_of(2) {
                    game_running = watcher.game_pid().is_some();
                }
                polls += 1;
                if !game_running {
                    match_over = false;
                }

                let data = if game_running && !match_over { live_client.all_game_data().ok() } else { None };
                let mut started_now = false;
                for notice in self.engine.tick(game_running, data.as_ref(), now_unix()) {
                    match notice {
                        Notice::MatchStarted { match_id, champion, recording } => {
                            started_now = true;
                            on_update(Update::MatchStarted { match_id, champion, recording });
                        }
                        Notice::RecordingFailed(e) => on_update(Update::RecordingFailed(e)),
                        Notice::Event { rec_time, event } => {
                            on_update(Update::Event { rec_time, detail: event.detail, involvement: event.involvement })
                        }
                        Notice::MatchFinished(job) => {
                            match_over = game_running;
                            on_update(Update::PostStarted { duration: job.duration });
                            pending_posts += 1;
                            jobs.push(self.post(job, &tx));
                        }
                        Notice::Error(e) => on_update(Update::Error(e)),
                    }
                }

                // ---- phase
                let phase = match (game_running, self.engine.status()) {
                    (_, Status::InGame { .. }) => Phase::InGame,
                    (true, Status::Idle) if !match_over => Phase::Loading,
                    _ => Phase::Idle,
                };
                if phase != overlay.phase() {
                    on_update(Update::Phase(phase));
                    overlay_out!(overlay.set_phase(phase, now()));
                    if phase == Phase::Idle {
                        client_players.clear();
                        client_requested = false;
                    } else if !client_requested {
                        client_requested = true;
                        fetch_client_info(tx.clone());
                    }
                }

                // ---- overlay data
                if started_now && !client_players.is_empty() {
                    // Now that the game reports champions and positions, rebuild the table.
                    let rows = ranks::join(&client_players, self.engine.live(), &statics.champions, &my_puuid);
                    overlay_out!(overlay.set_ranks(&rows, now()));
                    if let Err(e) = self.engine.set_client_info(rows, queue.clone()) {
                        on_update(Update::Error(format!("could not store ranks: {e}")));
                    }
                }
                if phase == Phase::InGame {
                    let prices = (!statics.prices.is_empty()).then_some(&statics.prices);
                    if let Some(report) = data.as_ref().and_then(|d| gold_report(d, prices)) {
                        overlay_out!(overlay.set_gold(&report, now()));
                    }
                }
            }
            overlay_out!(overlay.tick(now()));
        }

        // ---- shutdown: finish the current match and wait for file work
        if let Some(job) = self.engine.stop(now_unix()) {
            on_update(Update::PostStarted { duration: job.duration });
            pending_posts += 1;
            jobs.push(self.post(job, &tx));
        }
        overlay_out!(overlay.set_phase(Phase::Idle, now()));
        on_update(Update::Phase(Phase::Idle));
        for j in jobs {
            let _ = j.join();
        }
        while pending_posts > 0 {
            match internal.try_recv() {
                Ok(Internal::Post(result)) => {
                    pending_posts -= 1;
                    on_update(Update::PostFinished(result));
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
}
