//! Runs the real ffmpeg against a generated fragmented recording that mimics
//! what the recorder writes (H.264 60 fps, 1 s keyframes, AAC, fragmented MP4).
//! Skipped when ffmpeg/ffprobe are not installed.

use library::Ffmpeg;
use std::path::Path;
use std::process::Command;

fn have(tool: &str) -> bool {
    Command::new(tool).arg("-version").output().is_ok_and(|o| o.status.success())
}

fn duration(path: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn stream_kinds(path: &Path) -> String {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_name", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).split_whitespace().collect::<Vec<_>>().join(",")
}

#[test]
fn remux_cut_and_thumbnail() {
    if !have("ffmpeg") || !have("ffprobe") {
        eprintln!("skipping: ffmpeg/ffprobe not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("raw.mp4");
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=60", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-t", "20", "-c:v", "libx264", "-preset", "ultrafast", "-g", "60", "-keyint_min", "60", "-sc_threshold", "0", "-bf", "0", "-pix_fmt", "yuv420p"])
        .args(["-c:a", "aac", "-movflags", "frag_keyframe+empty_moov+default_base_moof"])
        .arg(&raw)
        .status()
        .unwrap();
    assert!(status.success());

    let ff = Ffmpeg::locate(None);
    assert!(ff.is_available());

    let full = dir.path().join("full.mp4");
    ff.remux(&raw, &full).unwrap();
    assert!((duration(&full) - 20.0).abs() < 0.2, "remux keeps the length: {}", duration(&full));
    assert_eq!(stream_kinds(&full), "h264,aac");

    // Cut 7.4 s – 13.4 s. With 1 s keyframes the clip may start up to 1 s early.
    let clip = dir.path().join("clip.mp4");
    ff.cut(&full, 7.4, 13.4, &clip).unwrap();
    let d = duration(&clip);
    assert!((5.9..=7.1).contains(&d), "clip length within one keyframe interval: {d}");
    assert_eq!(stream_kinds(&clip), "h264,aac");

    let thumb = dir.path().join("thumb.jpg");
    ff.thumbnail(&full, 5.0, &thumb).unwrap();
    assert!(std::fs::metadata(&thumb).unwrap().len() > 500);

    assert!(ff.cut(&full, 5.0, 5.0, &clip).is_err());
    let err = ff.remux(&dir.path().join("missing.mp4"), &full).unwrap_err();
    assert!(matches!(err, library::Error::Ffmpeg(_)));
    assert!(!Ffmpeg::new(dir.path().join("no-such-ffmpeg")).is_available());
}
