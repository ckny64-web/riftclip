//! File names and folders.

use std::path::{Path, PathBuf};

/// `2026-11-11_18-30-05` (UTC) for a unix timestamp.
pub fn timestamp(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}_{:02}-{:02}-{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// Keep letters and digits, turn everything else into single dashes.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    let mut s: String = trimmed.chars().take(60).collect();
    if s.is_empty() {
        s.push_str("clip");
    }
    s
}

pub fn matches_dir(base: &Path) -> PathBuf {
    base.join("matches")
}

pub fn clips_dir(base: &Path, stem: &str) -> PathBuf {
    base.join("clips").join(stem)
}

pub fn thumbs_dir(base: &Path) -> PathBuf {
    base.join("thumbs")
}

pub fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(0), "1970-01-01_00-00-00");
        assert_eq!(timestamp(951_782_400), "2000-02-29_00-00-00");
        assert_eq!(timestamp(1_794_421_805), "2026-11-11_18-30-05");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Ahri killed Zed"), "Ahri-killed-Zed");
        assert_eq!(slug("Baron taken by Kai'Sa (stolen)"), "Baron-taken-by-Kai-Sa-stolen");
        assert_eq!(slug("  ../..\\ "), "clip");
        assert_eq!(slug("Double kill by Ahri +1"), "Double-kill-by-Ahri-1");
    }
}
