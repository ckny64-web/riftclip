//! Draws the two panels.

use crate::format::*;
use crate::State;
use eframe::egui::{self, Color32, RichText};
use proto::{Gold, GoldLane, Message, RankRow};

fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(14, 17, 23, 225))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28)))
        .corner_radius(8.0)
        .inner_margin(12.0)
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(DIM).strong());
    ui.add_space(4.0);
}

fn rank_table(ui: &mut egui::Ui, id: &str, title: &str, rows: &[&RankRow]) {
    ui.vertical(|ui| {
        heading(ui, title);
        egui::Grid::new(id).num_columns(3).spacing([14.0, 5.0]).show(ui, |ui| {
            for row in rows {
                let who = match (row.champion.is_empty(), row.name.is_empty()) {
                    (false, false) => format!("{}  ·  {}", row.champion, row.name),
                    (false, true) => row.champion.clone(),
                    (true, false) => row.name.clone(),
                    (true, true) => "Hidden".to_string(),
                };
                let who_color = if row.is_me { Color32::WHITE } else { NEUTRAL };
                let mut who_text = RichText::new(who).size(14.0).color(who_color);
                if row.is_me {
                    who_text = who_text.strong();
                }
                ui.label(who_text);
                let rank = if row.rank.is_empty() { "—" } else { row.rank.as_str() };
                ui.label(RichText::new(rank).size(14.0).color(tier_color(&row.tier)));
                ui.label(RichText::new(win_rate(row.win_rate, row.games)).size(13.0).color(win_rate_color(row.win_rate)));
                ui.end_row();
            }
        });
    });
}

fn ranks_panel(ui: &mut egui::Ui, rows: &[RankRow]) {
    let allies: Vec<&RankRow> = rows.iter().filter(|r| r.ally).collect();
    let enemies: Vec<&RankRow> = rows.iter().filter(|r| !r.ally).collect();
    ui.horizontal_top(|ui| {
        rank_table(ui, "ranks_allies", "YOUR TEAM", &allies);
        if !enemies.is_empty() {
            ui.add_space(22.0);
            rank_table(ui, "ranks_enemies", "ENEMY TEAM", &enemies);
        }
    });
}

fn gold_panel(ui: &mut egui::Ui, gold: &Gold) {
    heading(ui, "ITEM GOLD");
    egui::Grid::new("gold_lanes").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
        for lane in &gold.lanes {
            ui.label(RichText::new(lane_label(&lane.position)).size(14.0).color(NEUTRAL));
            ui.label(RichText::new(gold_diff(lane.diff)).size(14.0).strong().color(diff_color(lane.diff)));
            ui.end_row();
        }
        ui.label(RichText::new("Team").size(14.0).color(DIM));
        ui.label(RichText::new(gold_diff(gold.team_diff)).size(15.0).strong().color(diff_color(gold.team_diff)));
        ui.end_row();
    });
}

pub fn draw(ctx: &egui::Context, state: &State) {
    let screen = ctx.viewport_rect();
    let at = |frac: (f32, f32)| egui::pos2(screen.width() * frac.0.clamp(0.0, 0.95), screen.height() * frac.1.clamp(0.0, 0.95));

    if state.show_ranks && !state.rows.is_empty() {
        egui::Area::new(egui::Id::unique("ranks")).fixed_pos(at(state.ranks_pos)).interactable(false).show(ctx, |ui| {
            panel_frame().show(ui, |ui| ranks_panel(ui, &state.rows));
        });
    }
    if state.show_gold {
        egui::Area::new(egui::Id::unique("gold")).fixed_pos(at(state.gold_pos)).interactable(false).show(ctx, |ui| {
            panel_frame().show(ui, |ui| gold_panel(ui, &state.gold));
        });
    }
}

/// Sample content for `--demo`.
pub fn demo_messages() -> Vec<Message> {
    let row = |ally: bool, me: bool, name: &str, champ: &str, rank: &str, tier: &str, wr: f32, games: u32| RankRow {
        ally,
        is_me: me,
        name: name.into(),
        champion: champ.into(),
        rank: rank.into(),
        tier: tier.into(),
        win_rate: (games > 0).then_some(wr),
        games,
    };
    let lane = |pos: &str, diff: i64| GoldLane { position: pos.into(), diff, ..Default::default() };
    vec![
        Message::Ranks {
            rows: vec![
                row(true, false, "TopAlly", "Garen", "Gold II 45 LP", "GOLD", 52.0, 120),
                row(true, false, "JungleAlly", "Vi", "Platinum IV 3 LP", "PLATINUM", 57.0, 88),
                row(true, true, "You", "Ahri", "Emerald III 71 LP", "EMERALD", 61.0, 203),
                row(true, false, "AdcAlly", "Jinx", "Unranked", "", 0.0, 0),
                row(true, false, "SupAlly", "Lulu", "Silver I 90 LP", "SILVER", 43.0, 61),
                row(false, false, "TopFoe", "Darius", "Diamond IV 12 LP", "DIAMOND", 55.0, 310),
                row(false, false, "JungleFoe", "Lee Sin", "Master 312 LP", "MASTER", 58.0, 420),
                row(false, false, "MidFoe", "Zed", "Gold I 0 LP", "GOLD", 49.0, 75),
                row(false, false, "", "Caitlyn", "", "", 0.0, 0),
                row(false, false, "SupFoe", "Thresh", "Bronze II 20 LP", "BRONZE", 47.0, 34),
            ],
        },
        Message::Gold(Gold {
            lanes: vec![lane("TOP", 333), lane("JUNGLE", -350), lane("MIDDLE", 1250), lane("BOTTOM", -450), lane("UTILITY", 0)],
            team_diff: 783,
        }),
        Message::Show { ranks: true, gold: true },
    ]
}
