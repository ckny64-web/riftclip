//! Commands the library window can call.

use crate::AppState;
use engine::manage;
use engine::runner::{db_path, Command};
use library::{AppSettings, ClipRow, EventRow, Ffmpeg, Library, MatchRow, MatchSummary, PlayerRow};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, State};

fn lib(state: &AppState) -> Result<Library, String> {
    Library::open(&db_path(&state.base_dir)).map_err(|e| e.to_string())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[tauri::command]
pub fn list_matches(state: State<AppState>) -> Result<Vec<MatchSummary>, String> {
    lib(&state)?.list_matches().map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct MatchDetails {
    #[serde(rename = "match")]
    row: MatchRow,
    players: Vec<PlayerRow>,
    events: Vec<EventRow>,
    clips: Vec<ClipRow>,
}

#[tauri::command]
pub fn match_details(state: State<AppState>, id: i64) -> Result<MatchDetails, String> {
    let lib = lib(&state)?;
    let e = |e: library::Error| e.to_string();
    Ok(MatchDetails {
        row: lib.get_match(id).map_err(e)?,
        players: lib.players(id).map_err(e)?,
        events: lib.events(id).map_err(e)?,
        clips: lib.clips(Some(id)).map_err(e)?,
    })
}

#[tauri::command]
pub fn list_clips(state: State<AppState>) -> Result<Vec<ClipRow>, String> {
    lib(&state)?.clips(None).map_err(|e| e.to_string())
}

/// Cut a clip out of a match's recording. Runs ffmpeg, so it is async to keep
/// the window responsive.
#[tauri::command(async)]
pub fn create_clip(state: State<AppState>, match_id: i64, start: f64, end: f64, title: String) -> Result<ClipRow, String> {
    let lib = lib(&state)?;
    let ffmpeg = Ffmpeg::locate(lib.settings().map_err(|e| e.to_string())?.ffmpeg_path.as_deref());
    if !ffmpeg.is_available() {
        return Err("ffmpeg was not found. Install it (winget install Gyan.FFmpeg) or set its path in Settings.".into());
    }
    manage::create_clip(&lib, &ffmpeg, &state.base_dir, match_id, start, end, &title, now_unix())
}

#[tauri::command]
pub fn rename_clip(state: State<AppState>, id: i64, title: String) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A clip needs a name.".into());
    }
    lib(&state)?.rename_clip(id, title).map_err(|_| "That clip no longer exists.".to_string())
}

#[tauri::command]
pub fn delete_clip(state: State<AppState>, id: i64) -> Result<(), String> {
    manage::delete_clip(&lib(&state)?, id)
}

#[tauri::command]
pub fn delete_recording(state: State<AppState>, match_id: i64) -> Result<(), String> {
    if state.status.lock().unwrap().match_id == Some(match_id) {
        return Err("This match is still being recorded.".into());
    }
    manage::delete_recording(&lib(&state)?, match_id)
}

#[tauri::command]
pub fn delete_match(state: State<AppState>, match_id: i64) -> Result<(), String> {
    if state.status.lock().unwrap().match_id == Some(match_id) {
        return Err("This match is still being recorded.".into());
    }
    manage::delete_match(&lib(&state)?, match_id)
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<AppSettings, String> {
    lib(&state)?.settings().map_err(|e| e.to_string())
}

/// Save settings and apply them: the recorder picks them up for the next
/// match, and the mark hotkey is re-registered. Returns a warning to show the
/// user if the hotkey could not be registered.
#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<AppState>, settings: AppSettings) -> Result<Option<String>, String> {
    if !(1_000..=100_000).contains(&settings.bitrate_kbps) {
        return Err("Bitrate must be between 1,000 and 100,000 kbps.".into());
    }
    if ![30, 60].contains(&settings.fps) {
        return Err("Frame rate must be 30 or 60.".into());
    }
    let pads = [settings.auto_clips.pad_before, settings.auto_clips.pad_after];
    if pads.iter().any(|p| !p.is_finite() || !(0.0..=120.0).contains(p)) {
        return Err("Clip padding must be between 0 and 120 seconds.".into());
    }
    lib(&state)?.save_settings(&settings).map_err(|e| e.to_string())?;
    state.send(Command::ReloadSettings);
    Ok(crate::register_hotkeys(&app, &state).err())
}

/// What the app is doing right now, for the window's status line.
#[tauri::command]
pub fn get_status(state: State<AppState>) -> crate::StatusInfo {
    state.status.lock().unwrap().clone()
}

#[derive(Serialize)]
pub struct AppInfo {
    data_folder: String,
    ffmpeg_found: bool,
}

#[tauri::command]
pub fn app_info(state: State<AppState>) -> AppInfo {
    let ffmpeg_path = lib(&state).ok().and_then(|l| l.settings().ok()).and_then(|s| s.ffmpeg_path);
    AppInfo {
        data_folder: state.base_dir.to_string_lossy().into_owned(),
        ffmpeg_found: Ffmpeg::locate(ffmpeg_path.as_deref()).is_available(),
    }
}

/// Open a file or folder with its default program.
pub fn reveal(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

fn inside_data_folder(state: &AppState, path: &str) -> Result<std::path::PathBuf, String> {
    let target = Path::new(path).canonicalize().map_err(|_| "That file no longer exists.".to_string())?;
    let base = state.base_dir.canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&base) {
        return Err("That file is outside the Riftclip folder.".into());
    }
    Ok(target)
}

/// Open a recording or clip in the default player. Only paths inside the
/// data folder are accepted.
#[tauri::command]
pub fn open_path(state: State<AppState>, path: String) -> Result<(), String> {
    reveal(&inside_data_folder(&state, &path)?);
    Ok(())
}

/// Show a file in File Explorer with it selected.
#[tauri::command]
pub fn show_in_folder(state: State<AppState>, path: String) -> Result<(), String> {
    let target = inside_data_folder(&state, &path)?;
    #[cfg(windows)]
    {
        // Explorer wants a plain path, not the \\?\ form canonicalize returns.
        let plain = target.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let _ = std::process::Command::new("explorer").arg(format!("/select,{plain}")).spawn();
    }
    #[cfg(not(windows))]
    reveal(target.parent().unwrap_or(&target));
    Ok(())
}

#[tauri::command]
pub fn open_data_folder(state: State<AppState>) {
    reveal(&state.base_dir);
}
