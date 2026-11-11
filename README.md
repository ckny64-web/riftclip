# Riftclip

A personal League of Legends companion for Windows, written in Rust: automatic
match recording with an event timeline and clips, plus an in-game rank viewer
and item-gold difference panel. Nothing is injected into the game; all data
comes from Riot's local APIs.

## Status

| Part | Where | State |
|---|---|---|
| League data (live API, events, gold, ranks, client parsing) | `crates/league` | Unit-tested; client endpoints confirmed against a real client |
| Clip library (SQLite, auto-clips, retention, ffmpeg) | `crates/library` | Unit-tested; ffmpeg operations tested for real |
| Recorder | `crates/recorder` | Confirmed working on Windows (1440p60, game-only audio) |
| Match engine, runner, command-line tool | `crates/engine` | Tested with a fake recorder and real ffmpeg; recording confirmed on Windows |
| Tray app | `app/src-tauri` | Recording and playback confirmed on Windows |
| Library window (player, timeline, clip cutting, clips, settings) | `app/ui` | Tested in a browser against a stand-in backend; **not yet run inside the app on Windows** |
| In-game overlay (rank and gold panels) | `crates/overlay` | **Parked.** Windows Smart App Control blocked the overlay program; it is left out of the default build |

## Requirements (Windows PC)

- Windows 10 2004 or later (Windows 11 recommended)
- [Rust](https://rustup.rs) with the MSVC toolchain and the Visual Studio C++ build tools
- `ffmpeg.exe` on PATH: `winget install Gyan.FFmpeg`, then open a new terminal
- The WebView2 runtime (already present on Windows 11)

## Build and run

Quit Riftclip from its tray icon first if it is running (Windows cannot replace a running program).

```
git pull
cargo build --release
target\release\riftclip-app.exe
```

A green-triangle icon appears in the system tray (it may be under the ^ arrow). Matches are
recorded automatically; "Open library" in the tray menu opens the library window.

Everything is stored in `Videos\Riftclip`. If something misbehaves, `riftclip.log` in that folder says what happened.

## Using the library window

- **Matches:** pick a match on the left. Click a marker or a timeline row to jump to 5 seconds before that moment.
- **Cutting a clip:** press **Clip** on a timeline row, or use **Set start** / **Set end** (keys **I** / **O**) and drag the gold handles, then **Save clip**. Clips are cut on one-second boundaries.
- **Keys:** Space plays and pauses, Left/Right skip 5 seconds (with Shift: 1 second).
- **Clips:** every clip, automatic or saved by you; click one to play it, rename, delete or show its file.
- **Settings:** recording quality, which moments become automatic clips and their padding, how long full recordings are kept, and the mark hotkey (F9 by default).

## Command-line tool (same engine, no tray or window)

```
cargo run -p engine --release --bin riftclip-cli
```

## Capture spike (milestone 1, done)

```
cargo run -p recorder --release --example spike -- --seconds 120 --out spike.mp4
```

Options: `--process NAME`, `--out PATH`, `--seconds N`, `--fps N`,
`--bitrate KBPS`, `--no-audio`. Without `--seconds` it records until Enter.

## Tests

```
cargo test --workspace
```

To capture real API data for the tests, run during a match:

```
cargo run -p league --example dump_live -- crates/league/tests/fixtures/allgamedata_real.json
```
