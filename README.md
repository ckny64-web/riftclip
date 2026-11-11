# Riftclip

A personal League of Legends companion for Windows, written in Rust: automatic
match recording with an event timeline and clips, plus an in-game rank viewer
and item-gold difference panel. Nothing is injected into the game; all data
comes from Riot's local APIs.

## Status

| Part | Where | State |
|---|---|---|
| League data (live API, events, gold, ranks, client parsing) | `crates/league` | Unit-tested against hand-written sample data |
| Clip library (SQLite, auto-clips, retention, ffmpeg) | `crates/library` | Unit-tested; ffmpeg operations tested for real |
| Recorder + capture spike | `crates/recorder` | Confirmed working on Windows (1440p60, game-only audio) |
| Match engine, runner, command-line tool | `crates/engine` | Flow tested with a fake recorder; real-game test in progress |
| Overlay (rank and gold panels) | `crates/overlay` | Renders correctly on a Linux test display; **not yet run on Windows or over the game** |
| Tray app with a basic library window | `app/src-tauri`, `app/ui` | Compiles for Windows; **never run** |
| Full library UI (in-app player, timeline, clip editor, settings) | — | Not started |

## Requirements (Windows PC)

- Windows 10 2004 or later (Windows 11 recommended)
- [Rust](https://rustup.rs) with the MSVC toolchain and the Visual Studio C++ build tools
- `ffmpeg.exe` on PATH: `winget install Gyan.FFmpeg`, then open a new terminal
- The WebView2 runtime (already present on Windows 11)

## Test 1: overlay on its own

In a Practice Tool game in **Borderless** mode, run in a terminal:

```
cargo run -p overlay --release -- --demo
```

Two sample panels should appear over the game. Check:

1. Do both panels show, with the game visible around them (not a black screen)?
2. Do clicks and key presses go to the game as if the panels were not there?
3. Does the game keep focus when the overlay starts?
4. Any FPS difference with the panels showing?

Press Ctrl+C in the terminal to close it.

## Test 2: League client probe

With the League client open (best during a normal game's loading screen or in game):

```
cargo run -p league --example lcu_probe
```

Paste the whole output. It shows whether ranks and names can be read the way Riftclip expects.

## Test 3: tray app

Build everything, then start the app (do not run the command-line tool at the same time):

```
cargo build --release
```

```
target\release\riftclip-app.exe
```

A green-triangle icon appears in the system tray (it may be under the ^ arrow). Then play a
normal game against real players, since Practice Tool has no one to show ranks for.

Check:

1. Tray menu status: "Idle", then "Game loading", then "Recording — <champion>".
2. Ranks panel on the loading screen, and for about 10 seconds after the game starts.
3. **F10** held: ranks panel shows. **F11**: gold panel on/off. **F9**: a beep, and a "Manual mark" in the match timeline afterwards.
4. After the game: recording and clips in `Videos\Riftclip`, and the match listed under tray menu > "Open library".
5. Closing the library window leaves the tray icon running; "Quit" exits.

If something does not work, send `Videos\Riftclip\riftclip.log`.

## Command-line tool (same engine, no tray or overlay)

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
