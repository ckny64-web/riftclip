//! Commands the library window can call.

use crate::AppState;
use engine::runner::db_path;
use library::{ClipRow, EventRow, Library, MatchRow, MatchSummary, PlayerRow};
use serde::Serialize;
use std::path::Path;
use tauri::State;

fn lib(state: &AppState) -> Result<Library, String> {
    Library::open(&db_path(&state.base_dir)).map_err(|e| e.to_string())
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

/// Open a file or folder with its default program.
pub fn reveal(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Open a recording or clip in the default player. Only paths inside the
/// data folder are accepted.
#[tauri::command]
pub fn open_path(state: State<AppState>, path: String) -> Result<(), String> {
    let target = Path::new(&path).canonicalize().map_err(|_| "That file no longer exists.".to_string())?;
    let base = state.base_dir.canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&base) {
        return Err("That file is outside the Riftclip folder.".into());
    }
    reveal(&target);
    Ok(())
}
