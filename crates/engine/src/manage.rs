//! Library actions started by the user: cutting a clip by hand and deleting
//! clips, recordings and matches together with their files.

use crate::naming::{clips_dir, path_string, slug, timestamp};
use library::{ClipRow, Ffmpeg, Library, NewClip};
use std::path::Path;

/// Shortest clip that can be saved, in seconds.
pub const MIN_CLIP_SECS: f64 = 1.0;

fn remove(path: &str) {
    let _ = std::fs::remove_file(path);
}

/// Cut `[start, end]` seconds out of a match's recording and add it to the
/// library. The range is clamped to the recording.
#[allow(clippy::too_many_arguments)] // one call site; a request struct would only rename the arguments
pub fn create_clip(
    lib: &Library,
    ffmpeg: &Ffmpeg,
    base_dir: &Path,
    match_id: i64,
    start: f64,
    end: f64,
    title: &str,
    now_unix: i64,
) -> Result<ClipRow, String> {
    let m = lib.get_match(match_id).map_err(|_| "That match no longer exists.".to_string())?;
    let recording = m.recording_path.filter(|p| Path::new(p).is_file()).ok_or("This match has no recording to cut from.")?;

    let too_short = || format!("A clip must be at least {MIN_CLIP_SECS:.0} second long.");
    if !(start.is_finite() && end.is_finite()) {
        return Err(too_short());
    }
    let start = start.max(0.0);
    let end = if m.duration > 0.0 { end.min(m.duration) } else { end };
    if end - start < MIN_CLIP_SECS {
        return Err(too_short());
    }

    let title = if title.trim().is_empty() { "Clip" } else { title.trim() };
    let stem = Path::new(&recording).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "match".into());
    let dir = clips_dir(base_dir, &stem);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create the clips folder: {e}"))?;

    // Unique file name even when two clips are saved in the same second.
    let base = format!("{}_{}", timestamp(now_unix), slug(title));
    let mut name = base.clone();
    let mut n = 2;
    while dir.join(format!("{name}.mp4")).exists() {
        name = format!("{base}-{n}");
        n += 1;
    }
    let clip = dir.join(format!("{name}.mp4"));
    ffmpeg.cut(Path::new(&recording), start, end, &clip).map_err(|e| format!("Could not cut the clip: {e}"))?;
    let thumb = dir.join(format!("{name}.jpg"));
    let thumb_ok = ffmpeg.thumbnail(&clip, (end - start) * 0.5, &thumb).is_ok();

    let id = lib
        .insert_clip(&NewClip {
            match_id: Some(match_id),
            start,
            end,
            path: path_string(&clip),
            thumbnail_path: thumb_ok.then(|| path_string(&thumb)),
            title: title.to_string(),
            auto: false,
            created_at: now_unix,
        })
        .map_err(|e| e.to_string())?;
    lib.get_clip(id).map_err(|e| e.to_string())
}

/// Remove a clip from the library and delete its files.
pub fn delete_clip(lib: &Library, clip_id: i64) -> Result<(), String> {
    let clip = lib.delete_clip(clip_id).map_err(|_| "That clip no longer exists.".to_string())?;
    remove(&clip.path);
    if let Some(t) = &clip.thumbnail_path {
        remove(t);
    }
    Ok(())
}

/// Delete a match's full recording to free space. The match, its timeline and
/// its clips stay.
pub fn delete_recording(lib: &Library, match_id: i64) -> Result<(), String> {
    let m = lib.get_match(match_id).map_err(|_| "That match no longer exists.".to_string())?;
    if let Some(p) = &m.recording_path {
        remove(p);
    }
    lib.set_recording(match_id, None, 0).map_err(|e| e.to_string())
}

/// Delete a match with its recording and timeline. Its clips are kept.
pub fn delete_match(lib: &Library, match_id: i64) -> Result<(), String> {
    let m = lib.get_match(match_id).map_err(|_| "That match no longer exists.".to_string())?;
    if let Some(p) = &m.recording_path {
        remove(p);
    }
    if let Some(p) = &m.thumbnail_path {
        remove(p);
    }
    lib.delete_match(match_id).map_err(|e| e.to_string())
}
