# Riftclip

A personal League of Legends companion for Windows, written in Rust: automatic
match recording with an event timeline and clips, plus an in-game rank viewer
and item-gold difference panel. Nothing is injected into the game; all data
comes from Riot's local APIs.

## Status

| Part | Crate | State |
|---|---|---|
| League data (live API, events, gold, client parsing) | `crates/league` | Unit-tested against hand-written sample data |
| Clip library (SQLite, auto-clips, retention, ffmpeg) | `crates/library` | Unit-tested; ffmpeg operations tested for real |
| Recorder + capture spike | `crates/recorder` | Compiles for Windows; **not yet run on Windows** |
| Overlay, tray app, library UI | — | Not started |

## Requirements (Windows PC)

- Windows 10 2004 or later (Windows 11 recommended)
- [Rust](https://rustup.rs) with the default MSVC toolchain, and the
  "Desktop development with C++" workload from Visual Studio Build Tools
- `ffmpeg.exe` on PATH (needed from the library milestone on, not for the spike)

## Milestone 1: capture spike

Start a Practice Tool game (Borderless mode), then in a terminal in this folder:

```
cargo run -p recorder --release --example spike -- --seconds 120 --out spike.mp4
```

Options: `--process NAME`, `--out PATH`, `--seconds N`, `--fps N`,
`--bitrate KBPS`, `--no-audio`. Without `--seconds` it records until Enter.

What to check and report back:

1. Does it start, and what does it print (resolution, audio line, the fps lines)?
2. Does `spike.mp4` play, smoothly, at the right resolution?
3. Is the sound only the game (play music or a video in the background while recording), and is it in sync?
4. In-game FPS with and without the recorder running.
5. Task Manager > Performance > GPU: is "Video Encode" active while recording?
6. Same test in exclusive Fullscreen mode: does it still record?

If it fails, copy the full error text.

## Tests

```
cargo test --workspace
```

To capture real API data for the tests, run during a match:

```
cargo run -p league --example dump_live -- crates/league/tests/fixtures/allgamedata_real.json
```
