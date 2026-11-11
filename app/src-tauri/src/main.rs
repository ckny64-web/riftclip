// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Riftclip tray app: runs the match runner in the background, shows status
//! in the tray, owns the hotkeys and the overlay process, and opens the
//! library window on demand. No window (and no webview) exists until asked for.

mod commands;
mod overlay_proc;

use engine::overlay::Phase;
use engine::real::GameRecorder;
use engine::runner::{db_path, default_dir, Command, Runner, Update};
use library::Library;
use overlay_proc::OverlayProcess;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Shared with commands and hotkey handlers.
pub struct AppState {
    pub base_dir: PathBuf,
    commands: Mutex<Sender<Command>>,
    hotkeys: Mutex<Vec<(Shortcut, Hotkey)>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Hotkey {
    Mark,
    Ranks,
    Gold,
}

impl AppState {
    fn send(&self, command: Command) {
        let _ = self.commands.lock().unwrap().send(command);
    }
}

/// Appends a line to `riftclip.log` in the data folder (the app has no console).
fn log(base_dir: &Path, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(base_dir.join("riftclip.log")) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "[{secs}] {line}");
    }
}

fn beep() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows::Win32::UI::WindowsAndMessaging::MB_OK;
        let _ = MessageBeep(MB_OK);
    }
}

fn open_library(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("library") {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, "library", WebviewUrl::App("index.html".into()))
        .title("Riftclip")
        .inner_size(1100.0, 720.0)
        .build();
    if let Err(e) = built {
        log(&app.state::<AppState>().base_dir, &format!("could not open the library window: {e}"));
    }
}

fn clock(secs: f64) -> String {
    format!("{:02}:{:02}", secs as u64 / 60, secs as u64 % 60)
}

/// Runs the match runner until Quit, mirroring its updates to the tray, the
/// log file and the overlay process. Exits the app when the runner returns.
fn runner_thread(app: AppHandle, base_dir: PathBuf, rx: std::sync::mpsc::Receiver<Command>, status: MenuItem<tauri::Wry>) {
    std::thread::spawn(move || {
        let set_status = |text: &str| {
            let _ = status.set_text(text);
            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_tooltip(Some(format!("Riftclip — {text}")));
            }
        };
        let runner = match Runner::new(base_dir.clone(), Box::new(GameRecorder)) {
            Ok(r) => r,
            Err(e) => {
                log(&base_dir, &format!("startup failed: {e}"));
                set_status("Startup failed (see riftclip.log)");
                return;
            }
        };
        if !runner.ffmpeg_available() {
            log(&base_dir, "ffmpeg was not found: recordings will not be finalized or cut into clips");
        }
        set_status("Idle");

        let mut overlay: Option<OverlayProcess> = None;
        let mut in_match = false;
        let mut finishing = 0u32;
        runner.run(rx, &mut |update| match update {
            Update::Phase(phase) => {
                match phase {
                    Phase::Idle => {
                        overlay = None; // dropping it closes the overlay
                        in_match = false;
                        if finishing == 0 {
                            set_status("Idle");
                        }
                    }
                    Phase::Loading | Phase::InGame => {
                        if overlay.is_none() {
                            match OverlayProcess::spawn(&base_dir.join("overlay.log")) {
                                Ok(p) => {
                                    log(&base_dir, "overlay started");
                                    overlay = Some(p)
                                }
                                Err(e) => log(&base_dir, &format!("overlay could not start: {e}")),
                            }
                        }
                        if phase == Phase::Loading {
                            set_status("Game loading");
                        }
                    }
                }
                log(&base_dir, &format!("phase: {phase:?}"));
            }
            Update::Overlay(message) => {
                match &message {
                    proto::Message::Show { ranks, gold } => log(&base_dir, &format!("overlay show: ranks {ranks}, gold {gold}")),
                    proto::Message::Ranks { rows } => log(&base_dir, &format!("overlay ranks: {} rows", rows.len())),
                    _ => {}
                }
                if let Some(p) = overlay.as_mut() {
                    if !p.send(&message) {
                        log(&base_dir, "overlay stopped responding");
                        overlay = None;
                    }
                }
            }
            Update::MatchStarted { champion, recording, .. } => {
                in_match = true;
                set_status(&if recording { format!("Recording — {champion}") } else { format!("In game (not recording) — {champion}") });
                log(&base_dir, &format!("match started as {champion}, recording: {recording}"));
            }
            Update::RecordingFailed(e) => log(&base_dir, &format!("recording could not start: {e}")),
            Update::Event { rec_time, detail, involvement } => {
                log(&base_dir, &format!("event [{}] {detail} ({})", clock(rec_time), involvement.as_str()))
            }
            Update::Marked(at) => {
                if let Some(t) = at {
                    beep();
                    log(&base_dir, &format!("manual mark at {}", clock(t)));
                }
            }
            Update::PostStarted { duration } => {
                finishing += 1;
                set_status("Finishing recording …");
                log(&base_dir, &format!("match over after {}, finishing files", clock(duration)));
            }
            Update::PostFinished(result) => {
                finishing = finishing.saturating_sub(1);
                match result {
                    Ok(r) => {
                        log(&base_dir, &format!("saved recording: {:?}, clips: {}, old recordings deleted: {}", r.recording, r.clips.len(), r.recordings_deleted));
                        for w in r.warnings {
                            log(&base_dir, &format!("warning: {w}"));
                        }
                        if finishing == 0 && !in_match {
                            set_status(&format!("Idle — last match: {} clip(s)", r.clips.len()));
                        }
                    }
                    Err(e) => {
                        log(&base_dir, &format!("could not finish match files: {e}"));
                        if finishing == 0 && !in_match {
                            set_status("Idle — last match failed to save (see riftclip.log)");
                        }
                    }
                }
            }
            Update::Info(s) => log(&base_dir, &s),
            Update::Error(e) => log(&base_dir, &format!("error: {e}")),
        });
        drop(overlay);
        app.exit(0);
    });
}

fn register_hotkeys(app: &AppHandle, state: &AppState) {
    let settings = Library::open(&db_path(&state.base_dir)).and_then(|l| l.settings()).unwrap_or_default();
    let wanted = [
        (settings.hotkeys.mark, Hotkey::Mark),
        (settings.hotkeys.show_ranks, Hotkey::Ranks),
        (settings.hotkeys.toggle_gold, Hotkey::Gold),
    ];
    let mut registered = Vec::new();
    for (text, action) in wanted {
        match text.parse::<Shortcut>() {
            Ok(shortcut) => match app.global_shortcut().register(shortcut) {
                Ok(()) => registered.push((shortcut, action)),
                Err(e) => log(&state.base_dir, &format!("hotkey {text} could not be registered: {e}")),
            },
            Err(e) => log(&state.base_dir, &format!("hotkey '{text}' is not valid: {e}")),
        }
    }
    *state.hotkeys.lock().unwrap() = registered;
}

fn main() {
    let base_dir = default_dir();
    let _ = std::fs::create_dir_all(&base_dir);
    let (tx, rx) = channel::<Command>();
    let state = AppState { base_dir: base_dir.clone(), commands: Mutex::new(tx), hotkeys: Mutex::new(Vec::new()) };

    let app = tauri::Builder::default()
        // A second launch just brings up the library window of the first.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| open_library(app)))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    let state = app.state::<AppState>();
                    let action = state.hotkeys.lock().unwrap().iter().find(|(s, _)| s == shortcut).map(|(_, a)| *a);
                    let pressed = event.state() == ShortcutState::Pressed;
                    match action {
                        Some(Hotkey::Mark) if pressed => state.send(Command::Mark),
                        Some(Hotkey::Gold) if pressed => state.send(Command::ToggleGold),
                        Some(Hotkey::Ranks) => state.send(Command::HoldRanks(pressed)),
                        _ => {}
                    }
                })
                .build(),
        )
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::list_matches,
            commands::match_details,
            commands::list_clips,
            commands::open_path,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let status = MenuItem::with_id(app, "status", "Starting …", false, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open library", true, None::<&str>)?;
            let folder = MenuItem::with_id(app, "folder", "Open recordings folder", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&status, &PredefinedMenuItem::separator(app)?, &open, &folder, &quit])?;

            let mut tray = TrayIconBuilder::with_id("main").tooltip("Riftclip").menu(&menu).show_menu_on_left_click(true);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            let quit_status = status.clone();
            tray.on_menu_event(move |app, event| {
                let state = app.state::<AppState>();
                match event.id().as_ref() {
                    "open" => open_library(app),
                    "folder" => commands::reveal(&state.base_dir),
                    "quit" => {
                        // The runner finishes any recording, then exits the app.
                        let _ = quit_status.set_text("Quitting …");
                        state.send(Command::Quit);
                    }
                    _ => {}
                }
            })
            .build(app)?;

            register_hotkeys(&handle, &app.state::<AppState>());
            runner_thread(handle, base_dir.clone(), rx, status);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while starting Riftclip");

    app.run(|_app, event| {
        // Closing the library window must not quit the tray app; only an
        // explicit exit (code set) does.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
