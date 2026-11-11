//! Manual clip cutting and deletions, against real files and the real ffmpeg.

use engine::manage::{create_clip, delete_clip, delete_match, delete_recording};
use library::{Ffmpeg, Library, NewMatch};
use std::path::{Path, PathBuf};
use std::process::Command;

fn have_ffmpeg() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|t| Command::new(t).arg("-version").output().is_ok_and(|o| o.status.success()))
}

fn duration(path: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// A 20 s recording like the finished ones in the library, registered as a match.
fn setup(dir: &Path) -> (Library, i64, PathBuf) {
    let matches = dir.join("matches");
    std::fs::create_dir_all(&matches).unwrap();
    let rec = matches.join("2026-11-11_18-30-05_Ahri.mp4");
    let ok = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=60", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-t", "20", "-c:v", "libx264", "-preset", "ultrafast", "-g", "60", "-bf", "0", "-pix_fmt", "yuv420p", "-c:a", "aac", "-movflags", "+faststart"])
        .arg(&rec)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let lib = Library::open_in_memory().unwrap();
    let id = lib.insert_match(&NewMatch { started_at: 100, champion: "Ahri".into(), ..Default::default() }).unwrap();
    lib.finish_match(id, 20.0, "win").unwrap();
    lib.set_recording(id, Some(rec.to_str().unwrap()), 1234).unwrap();
    let thumb = dir.join("thumb.jpg");
    std::fs::write(&thumb, b"jpg").unwrap();
    lib.set_match_thumbnail(id, thumb.to_str().unwrap()).unwrap();
    (lib, id, rec)
}

#[test]
fn cut_rename_and_delete() {
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg/ffprobe not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (lib, id, rec) = setup(dir.path());
    let ff = Ffmpeg::locate(None);

    let clip = create_clip(&lib, &ff, dir.path(), id, 5.0, 11.0, "  Nice flash  ", 1_794_421_805).unwrap();
    assert_eq!(clip.title, "Nice flash");
    assert_eq!((clip.start, clip.end, clip.auto, clip.match_id), (5.0, 11.0, false, Some(id)));
    assert!(clip.path.ends_with("2026-11-11_18-30-05_Nice-flash.mp4"));
    assert!(Path::new(&clip.path).starts_with(dir.path().join("clips").join("2026-11-11_18-30-05_Ahri")));
    let d = duration(Path::new(&clip.path));
    assert!((5.9..=7.1).contains(&d), "clip is about 6 s: {d}");
    assert!(Path::new(clip.thumbnail_path.as_ref().unwrap()).is_file());

    // Same title in the same second gets its own file; the range is clamped to the recording.
    let second = create_clip(&lib, &ff, dir.path(), id, -3.0, 500.0, "Nice flash", 1_794_421_805).unwrap();
    assert_ne!(second.path, clip.path);
    assert_eq!((second.start, second.end), (0.0, 20.0));
    // An empty title falls back to "Clip".
    assert_eq!(create_clip(&lib, &ff, dir.path(), id, 1.0, 3.0, " ", 1).unwrap().title, "Clip");

    // Refusals.
    assert!(create_clip(&lib, &ff, dir.path(), id, 5.0, 5.5, "x", 1).unwrap_err().contains("at least"));
    assert!(create_clip(&lib, &ff, dir.path(), id, 9.0, 4.0, "x", 1).is_err());
    assert!(create_clip(&lib, &ff, dir.path(), id, f64::NAN, 4.0, "x", 1).is_err());
    assert!(create_clip(&lib, &ff, dir.path(), 999, 1.0, 5.0, "x", 1).unwrap_err().contains("no longer exists"));

    // Deleting a clip removes its files.
    delete_clip(&lib, clip.id).unwrap();
    assert!(!Path::new(&clip.path).exists());
    assert!(!Path::new(clip.thumbnail_path.as_ref().unwrap()).exists());
    assert!(delete_clip(&lib, clip.id).is_err());
    assert_eq!(lib.clips(Some(id)).unwrap().len(), 2);

    // Deleting the recording keeps the match and its clips, and stops further cutting.
    delete_recording(&lib, id).unwrap();
    assert!(!rec.exists());
    assert_eq!(lib.get_match(id).unwrap().recording_path, None);
    assert!(create_clip(&lib, &ff, dir.path(), id, 1.0, 5.0, "x", 1).unwrap_err().contains("no recording"));
    assert!(Path::new(&second.path).is_file());

    // Deleting the match removes its thumbnail; clips survive without a match.
    delete_match(&lib, id).unwrap();
    assert!(!dir.path().join("thumb.jpg").exists());
    assert!(lib.get_match(id).is_err());
    let left = lib.clips(None).unwrap();
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|c| c.match_id.is_none() && Path::new(&c.path).is_file()));
    assert!(delete_match(&lib, id).is_err());
}

#[test]
fn deleting_a_match_removes_its_recording_file() {
    let dir = tempfile::tempdir().unwrap();
    let rec = dir.path().join("rec.mp4");
    std::fs::write(&rec, b"video").unwrap();
    let lib = Library::open_in_memory().unwrap();
    let id = lib.insert_match(&NewMatch { started_at: 1, ..Default::default() }).unwrap();
    lib.set_recording(id, Some(rec.to_str().unwrap()), 5).unwrap();
    delete_match(&lib, id).unwrap();
    assert!(!rec.exists());
}
