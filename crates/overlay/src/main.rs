// No console window when started by the app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! The in-game overlay: a transparent, click-through, always-on-top window
//! that draws the rank and gold panels. It is told what to show through JSON
//! lines on standard input (see the `proto` crate) and exits when that closes.
//!
//!   riftclip-overlay          normal mode, driven by the app
//!   riftclip-overlay --demo   show both panels with sample data

mod format;
mod panels;
mod platform;

use eframe::egui;
use proto::Message;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, OnceLock};

static LOG: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

/// Appends a line to the file given with `--log`, if any. Used to diagnose
/// why panels do not appear, since the overlay has no console.
pub fn log(line: &str) {
    if let Some(file) = LOG.get() {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(file.lock().unwrap(), "[{secs}] {line}");
    }
}

/// Everything the panels draw from; written by the input thread.
#[derive(Default)]
pub struct State {
    pub rows: Vec<proto::RankRow>,
    pub gold: proto::Gold,
    pub show_ranks: bool,
    pub show_gold: bool,
    pub ranks_pos: (f32, f32),
    pub gold_pos: (f32, f32),
    pub scale: f32,
}

impl State {
    fn new() -> Self {
        State { ranks_pos: (0.35, 0.2), gold_pos: (0.01, 0.3), scale: 1.0, ..Default::default() }
    }

    fn apply(&mut self, msg: Message) {
        match msg {
            Message::Ranks { rows } => self.rows = rows,
            Message::Gold(gold) => self.gold = gold,
            Message::Show { ranks, gold } => {
                self.show_ranks = ranks;
                self.show_gold = gold;
            }
            Message::Config { ranks_pos, gold_pos, scale } => {
                self.ranks_pos = ranks_pos;
                self.gold_pos = gold_pos;
                self.scale = scale.clamp(0.5, 3.0);
            }
        }
    }

    fn anything_visible(&self) -> bool {
        self.show_ranks || self.show_gold
    }
}

struct Overlay {
    state: Arc<Mutex<State>>,
    placed: bool,
    frames: u64,
    last_shown: (bool, bool),
    window: platform::Window,
}

impl eframe::App for Overlay {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0] // fully transparent background
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // Cover the monitor once its size is known.
        if !self.placed {
            if let Some(size) = ctx.input(|i| i.viewport().monitor_size) {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                self.placed = true;
                log(&format!("monitor size {size:?}, window resized to cover it"));
            }
        }
        let state = self.state.lock().unwrap();
        self.frames += 1;
        let shown = (state.show_ranks, state.show_gold);
        if self.frames <= 3 || shown != self.last_shown {
            self.last_shown = shown;
            log(&format!(
                "frame {}: drawing area {:?}, ranks shown {} ({} rows), gold shown {}",
                self.frames,
                ctx.viewport_rect(),
                shown.0,
                state.rows.len(),
                shown.1
            ));
            // The windowing library may have changed styles since creation.
            if self.frames == 2 {
                self.window.make_overlay();
            }
            log(&format!("window: {}", self.window.describe()));
        }
        if (ctx.zoom_factor() - state.scale).abs() > 0.001 {
            ctx.set_zoom_factor(state.scale);
        }
        panels::draw(ctx, &state);
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|a| a == "--demo");
    if let Some(path) = args.iter().position(|a| a == "--log").and_then(|i| args.get(i + 1)) {
        if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = LOG.set(Mutex::new(file));
        }
    }
    log(&format!("overlay starting (demo: {demo})"));
    let state = Arc::new(Mutex::new(State::new()));
    if demo {
        let mut s = state.lock().unwrap();
        for m in panels::demo_messages() {
            s.apply(m);
        }
        println!("Showing demo panels. Press Ctrl+C in this window to close the overlay.");
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Riftclip Overlay")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_taskbar(false)
            .with_resizable(false)
            .with_active(false)
            .with_visible(demo)
            .with_position([0.0, 0.0])
            .with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };

    let thread_state = state.clone();
    eframe::run_native(
        "Riftclip Overlay",
        options,
        Box::new(move |cc| {
            let window = platform::Window::from_creation(cc);
            log(&format!("window created, native handle found: {}", window.is_valid()));
            window.make_overlay();
            log(&format!("window after setup: {}", window.describe()));
            if !demo {
                let ctx = cc.egui_ctx.clone();
                std::thread::spawn(move || {
                    for line in std::io::stdin().lock().lines() {
                        let Ok(line) = line else { break };
                        let Some(msg) = Message::from_line(&line) else {
                            log("ignored a line that is not a known message");
                            continue;
                        };
                        match &msg {
                            Message::Ranks { rows } => log(&format!("received ranks: {} rows", rows.len())),
                            Message::Gold(g) => log(&format!("received gold: {} lanes, team {}", g.lanes.len(), g.team_diff)),
                            Message::Show { ranks, gold } => log(&format!("received show: ranks {ranks}, gold {gold}")),
                            Message::Config { .. } => log("received config"),
                        }
                        let visible = {
                            let mut s = thread_state.lock().unwrap();
                            s.apply(msg);
                            s.anything_visible()
                        };
                        // Hidden entirely when there is nothing to draw, so the
                        // overlay costs nothing most of the time.
                        // Shown without activating so the game keeps focus; the
                        // UI library is told as well so it paints the window.
                        window.set_visible(visible);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(visible));
                        ctx.request_repaint();
                    }
                    // The app closed our input: it is gone or the game ended.
                    log("input closed, exiting");
                    std::process::exit(0);
                });
            }
            Ok(Box::new(Overlay { state, placed: false, frames: 0, last_shown: (false, false), window }))
        }),
    )
}
