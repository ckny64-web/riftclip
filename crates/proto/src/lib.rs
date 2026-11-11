//! Messages the app sends to the overlay process, one JSON object per line on
//! the overlay's standard input.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Replace the rank panel's rows.
    Ranks { rows: Vec<RankRow> },
    /// Replace the gold panel's contents.
    Gold(Gold),
    /// Which panels are visible. With both off the overlay window is hidden.
    Show { ranks: bool, gold: bool },
    /// Panel placement: top-left corners as fractions of the screen, and a size factor.
    Config { ranks_pos: (f32, f32), gold_pos: (f32, f32), scale: f32 },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RankRow {
    /// True for the local player's team.
    pub ally: bool,
    pub is_me: bool,
    pub name: String,
    /// May be empty on the loading screen.
    pub champion: String,
    /// Ready-to-show text such as "Gold II 45 LP" or "Unranked"; empty if unknown.
    pub rank: String,
    /// "IRON" … "CHALLENGER" or empty; used for colouring.
    pub tier: String,
    /// Ranked win rate in percent, if the player has ranked games.
    pub win_rate: Option<f32>,
    pub games: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Gold {
    pub lanes: Vec<GoldLane>,
    /// My team's item gold minus the enemy team's.
    pub team_diff: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GoldLane {
    /// "TOP", "JUNGLE", "MIDDLE", "BOTTOM" or "UTILITY".
    pub position: String,
    pub ally_champion: String,
    pub enemy_champion: String,
    /// Ally minus enemy item gold.
    pub diff: i64,
}

impl Message {
    /// One line of JSON, without the trailing newline.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("messages always serialize")
    }

    pub fn from_line(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let messages = vec![
            Message::Ranks {
                rows: vec![RankRow {
                    ally: true,
                    is_me: true,
                    name: "Mid Me".into(),
                    champion: "Ahri".into(),
                    rank: "Gold II 45 LP".into(),
                    tier: "GOLD".into(),
                    win_rate: Some(60.0),
                    games: 50,
                }],
            },
            Message::Gold(Gold {
                lanes: vec![GoldLane { position: "MIDDLE".into(), ally_champion: "Ahri".into(), enemy_champion: "Zed".into(), diff: -350 }],
                team_diff: 783,
            }),
            Message::Show { ranks: true, gold: false },
            Message::Config { ranks_pos: (0.3, 0.2), gold_pos: (0.01, 0.3), scale: 1.25 },
        ];
        for m in messages {
            let line = m.to_line();
            assert!(!line.contains('\n'));
            assert_eq!(Message::from_line(&line), Some(m));
        }
    }

    #[test]
    fn wire_format_is_stable_and_lenient() {
        assert_eq!(Message::Show { ranks: true, gold: false }.to_line(), r#"{"type":"show","ranks":true,"gold":false}"#);
        // Missing row fields default; unknown fields are ignored.
        let m = Message::from_line(r#"{"type":"ranks","rows":[{"name":"X","future":1}]}"#).unwrap();
        assert_eq!(m, Message::Ranks { rows: vec![RankRow { name: "X".into(), ..Default::default() }] });
        assert_eq!(Message::from_line("not json"), None);
        assert_eq!(Message::from_line(r#"{"type":"unknown"}"#), None);
    }
}
