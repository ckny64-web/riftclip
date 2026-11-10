//! Thin wrapper around the `ffmpeg` executable for the three file operations
//! the library needs. All of them copy streams without re-encoding except the
//! thumbnail.

use crate::{Error, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct Ffmpeg {
    exe: PathBuf,
}

fn secs(t: f64) -> OsString {
    format!("{:.3}", t.max(0.0)).into()
}

/// Arguments to turn a fragmented recording into a normal, instantly seekable MP4.
pub fn remux_args(src: &Path, dst: &Path) -> Vec<OsString> {
    vec![
        "-y".into(), "-v".into(), "error".into(),
        "-i".into(), src.into(),
        "-c".into(), "copy".into(),
        "-movflags".into(), "+faststart".into(),
        dst.into(),
    ]
}

/// Arguments to cut `[start, end]` seconds out of `src` without re-encoding.
/// The cut starts on the keyframe at or before `start`.
pub fn cut_args(src: &Path, start: f64, end: f64, dst: &Path) -> Vec<OsString> {
    vec![
        "-y".into(), "-v".into(), "error".into(),
        "-ss".into(), secs(start),
        "-i".into(), src.into(),
        "-t".into(), secs(end - start),
        "-c".into(), "copy".into(),
        "-avoid_negative_ts".into(), "make_zero".into(),
        "-movflags".into(), "+faststart".into(),
        dst.into(),
    ]
}

/// Arguments to save one frame at `at` seconds as a small JPEG.
pub fn thumbnail_args(src: &Path, at: f64, dst: &Path) -> Vec<OsString> {
    vec![
        "-y".into(), "-v".into(), "error".into(),
        "-ss".into(), secs(at),
        "-i".into(), src.into(),
        "-frames:v".into(), "1".into(),
        "-vf".into(), "scale=480:-2".into(),
        "-q:v".into(), "4".into(),
        dst.into(),
    ]
}

impl Ffmpeg {
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        Ffmpeg { exe: exe.into() }
    }

    /// Use `configured` if given, else an `ffmpeg` next to the running
    /// program, else whatever `ffmpeg` is on PATH.
    pub fn locate(configured: Option<&Path>) -> Self {
        if let Some(p) = configured {
            return Ffmpeg::new(p);
        }
        let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        if let Some(beside) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name))) {
            if beside.is_file() {
                return Ffmpeg::new(beside);
            }
        }
        Ffmpeg::new(name)
    }

    /// Whether the executable can be started at all.
    pub fn is_available(&self) -> bool {
        self.command().arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.exe);
        cmd.stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash
        }
        cmd
    }

    fn run(&self, args: Vec<OsString>) -> Result<()> {
        let out = self
            .command()
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| Error::Ffmpeg(format!("could not start {}: {e}", self.exe.display())))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Error::Ffmpeg(String::from_utf8_lossy(&out.stderr).trim().to_string()))
        }
    }

    pub fn remux(&self, src: &Path, dst: &Path) -> Result<()> {
        self.run(remux_args(src, dst))
    }

    pub fn cut(&self, src: &Path, start: f64, end: f64, dst: &Path) -> Result<()> {
        if end <= start {
            return Err(Error::Ffmpeg("clip end must be after its start".into()));
        }
        self.run(cut_args(src, start, end, dst))
    }

    pub fn thumbnail(&self, src: &Path, at: f64, dst: &Path) -> Result<()> {
        self.run(thumbnail_args(src, at, dst))
    }
}
