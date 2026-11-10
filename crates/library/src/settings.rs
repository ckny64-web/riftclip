//! User settings, stored as one JSON document in the database.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Which events become clips automatically, and how much footage surrounds them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoClipRules {
    pub enabled: bool,
    /// My kills (multikills are merged into one clip by the padding).
    pub kills: bool,
    pub deaths: bool,
    pub assists: bool,
    /// Dragons, Baron, Herald, turrets and inhibitors my team takes.
    pub objectives: bool,
    /// Seconds of footage before the event.
    pub pad_before: f64,
    /// Seconds of footage after the event.
    pub pad_after: f64,
}

impl Default for AutoClipRules {
    fn default() -> Self {
        AutoClipRules {
            enabled: true,
            kills: true,
            deaths: true,
            assists: true,
            objectives: true,
            pad_before: 12.0,
            pad_after: 6.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub ranks_enabled: bool,
    pub gold_enabled: bool,
    /// Top-left corner of each panel as a fraction of the screen (0.0–1.0).
    pub ranks_pos: (f32, f32),
    pub gold_pos: (f32, f32),
    pub scale: f32,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        OverlaySettings {
            ranks_enabled: true,
            gold_enabled: true,
            ranks_pos: (0.35, 0.2),
            gold_pos: (0.01, 0.3),
            scale: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub mark: String,
    pub show_ranks: String,
    pub toggle_gold: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Hotkeys { mark: "F9".into(), show_ranks: "F10".into(), toggle_gold: "F11".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Where recordings and clips are written. `None` means the default
    /// (`Videos/Riftclip`), resolved by the app.
    pub recordings_dir: Option<PathBuf>,
    /// Path to `ffmpeg.exe`. `None` means "next to the app, else on PATH".
    pub ffmpeg_path: Option<PathBuf>,
    pub record_enabled: bool,
    pub bitrate_kbps: u32,
    pub fps: u32,
    pub auto_clips: AutoClipRules,
    /// Delete full recordings older than this many days (0 = never).
    pub retention_days: u32,
    /// Delete the oldest full recordings above this total size (0 = no cap).
    pub max_recordings_gb: u32,
    pub hotkeys: Hotkeys,
    pub overlay: OverlaySettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            recordings_dir: None,
            ffmpeg_path: None,
            record_enabled: true,
            bitrate_kbps: 20_000,
            fps: 60,
            auto_clips: AutoClipRules::default(),
            retention_days: 14,
            max_recordings_gb: 100,
            hotkeys: Hotkeys::default(),
            overlay: OverlaySettings::default(),
        }
    }
}
