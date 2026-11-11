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
| Recorder + capture spike | `crates/recorder` | Confirmed working on Windows (1440p60, game-only audio) |
| Match engine + command-line runner | `crates/engine` | Flow tested with a fake recorder; **not yet run against a real game** |
| Overlay, tray app, library UI | — | Not started |

## Requirements (Windows PC)

- Windows 10 2004 or later (Windows 11 recommended)
- [Rust](https://rustup.rs) with the MSVC toolchain and the Visual Studio C++ build tools
- `ffmpeg.exe` on PATH: `winget install Gyan.FFmpeg`, then open a new terminal

## Milestones 2 and 3: automatic recording, events and clips

In a terminal in this folder (League can be closed or open, but not yet in a game):

```
cargo run -p engine --release --bin riftclip-cli
```

Leave it running, play a Practice Tool game in Borderless mode (kill a target
dummy or two, die once if you can), then exit the game. Press Ctrl+C when it
has printed the saved files.

Everything is written to `Videos\Riftclip` in your user folder: `matches\`
holds the full recording, `clips\` the automatic clips.

What to check and report back:

1. The full terminal output.
2. Did recording start by itself once the game loaded, and stop when you left?
3. Are the listed events right, and do their times match the footage (within about a second)?
4. Do the clips in `clips\` show the right moments?
5. Does the full recording in `matches\` seek instantly when you drag the timeline?

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
