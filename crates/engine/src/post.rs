//! Work done after a match ends: make the recording seekable, cut the
//! automatic clips, and apply retention. Runs off the polling thread.

use crate::naming::{clips_dir, matches_dir, path_string, slug, thumbs_dir};
use library::{plan_clips, AutoClipRules, Ffmpeg, Library, NewClip};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PostJob {
    pub match_id: i64,
    pub started_at: i64,
    /// Base file name shared by the match's recording, thumbnail and clips.
    pub stem: String,
    /// The fragmented file the recorder wrote; `None` if nothing was recorded.
    pub raw_path: Option<PathBuf>,
    pub duration: f64,
    pub base_dir: PathBuf,
    pub rules: AutoClipRules,
    pub retention_days: u32,
    pub max_recordings_gb: u32,
    pub now_unix: i64,
    /// Set when the recorder reported a problem closing the file.
    pub stop_error: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct PostReport {
    pub recording: Option<PathBuf>,
    pub clips: Vec<PathBuf>,
    pub recordings_deleted: usize,
    /// Non-fatal problems, in plain words.
    pub warnings: Vec<String>,
}

impl PostJob {
    pub fn run(&self, lib: &mut Library, ffmpeg: &Ffmpeg) -> library::Result<PostReport> {
        let mut report = PostReport::default();
        if let Some(e) = &self.stop_error {
            report.warnings.push(format!("the recorder reported a problem closing the file: {e}"));
        }

        if let Some(raw) = self.raw_path.as_ref().filter(|p| p.is_file()) {
            // 1. Fragmented -> normal MP4 so players can seek instantly.
            let final_path = matches_dir(&self.base_dir).join(format!("{}.mp4", self.stem));
            let recording = match ffmpeg.remux(raw, &final_path) {
                Ok(()) => {
                    let _ = std::fs::remove_file(raw);
                    final_path
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&final_path);
                    report.warnings.push(format!("could not finalize the recording, keeping the raw file: {e}"));
                    raw.clone()
                }
            };
            let bytes = std::fs::metadata(&recording).map(|m| m.len()).unwrap_or(0);
            lib.set_recording(self.match_id, Some(&path_string(&recording)), bytes)?;

            // 2. Thumbnail for the match list.
            let thumbs = thumbs_dir(&self.base_dir);
            std::fs::create_dir_all(&thumbs)?;
            let thumb = thumbs.join(format!("{}.jpg", self.stem));
            match ffmpeg.thumbnail(&recording, (self.duration * 0.5).min(120.0), &thumb) {
                Ok(()) => lib.set_match_thumbnail(self.match_id, &path_string(&thumb))?,
                Err(e) => report.warnings.push(format!("no thumbnail: {e}")),
            }

            // 3. Automatic clips.
            let plans = plan_clips(&lib.events(self.match_id)?, &self.rules, self.duration);
            if !plans.is_empty() {
                let dir = clips_dir(&self.base_dir, &self.stem);
                std::fs::create_dir_all(&dir)?;
                for (i, plan) in plans.iter().enumerate() {
                    let name = format!("{:02}_{}", i + 1, slug(&plan.title));
                    let clip = dir.join(format!("{name}.mp4"));
                    if let Err(e) = ffmpeg.cut(&recording, plan.start, plan.end, &clip) {
                        report.warnings.push(format!("could not cut clip '{}': {e}", plan.title));
                        continue;
                    }
                    let clip_thumb = dir.join(format!("{name}.jpg"));
                    let thumb_ok = ffmpeg.thumbnail(&clip, (plan.end - plan.start) * 0.5, &clip_thumb).is_ok();
                    lib.insert_clip(&NewClip {
                        match_id: Some(self.match_id),
                        start: plan.start,
                        end: plan.end,
                        path: path_string(&clip),
                        thumbnail_path: thumb_ok.then(|| path_string(&clip_thumb)),
                        title: plan.title.clone(),
                        auto: true,
                        created_at: self.now_unix,
                    })?;
                    report.clips.push(clip);
                }
            }
            report.recording = Some(recording);
        } else if self.raw_path.is_some() {
            report.warnings.push("the recording file is missing".into());
            lib.set_recording(self.match_id, None, 0)?;
        }

        // 4. Retention: old or over-quota full recordings go; clips stay.
        let cap = self.max_recordings_gb as u64 * (1 << 30);
        for id in library::retention::plan(&lib.recordings()?, self.now_unix, self.retention_days, cap) {
            if id == self.match_id {
                continue; // never delete the match that just finished
            }
            if let Some(path) = lib.get_match(id)?.recording_path {
                let _ = std::fs::remove_file(&path);
            }
            lib.set_recording(id, None, 0)?;
            report.recordings_deleted += 1;
        }
        Ok(report)
    }
}
