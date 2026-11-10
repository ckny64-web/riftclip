//! Serde models for the Live Client Data API (`https://127.0.0.1:2999/liveclientdata/`).
//! Every field defaults, because Riot adds and removes fields between patches.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AllGameData {
    pub active_player: ActivePlayer,
    pub all_players: Vec<Player>,
    pub events: EventList,
    pub game_data: GameData,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivePlayer {
    pub summoner_name: String,
    pub riot_id: String,
    pub riot_id_game_name: String,
    pub riot_id_tag_line: String,
    pub current_gold: f64,
    pub level: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Player {
    pub champion_name: String,
    pub raw_champion_name: String,
    pub summoner_name: String,
    pub riot_id: String,
    pub riot_id_game_name: String,
    pub riot_id_tag_line: String,
    /// "ORDER" (blue side) or "CHAOS" (red side).
    pub team: String,
    /// "TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY", or empty outside Summoner's Rift.
    pub position: String,
    pub level: u32,
    pub is_dead: bool,
    pub is_bot: bool,
    pub items: Vec<Item>,
    pub scores: Scores,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Item {
    #[serde(rename = "itemID")]
    pub item_id: u32,
    pub count: u32,
    pub price: u32,
    pub display_name: String,
    pub slot: u32,
    pub consumable: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Scores {
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub creep_score: u32,
    pub ward_score: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EventList {
    #[serde(rename = "Events")]
    pub events: Vec<RawEvent>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct RawEvent {
    #[serde(rename = "EventID")]
    pub event_id: u32,
    pub event_name: String,
    /// Seconds on the game clock.
    pub event_time: f64,
    pub killer_name: String,
    pub victim_name: String,
    pub assisters: Vec<String>,
    pub kill_streak: u32,
    pub acer: String,
    pub acing_team: String,
    pub recipient: String,
    pub dragon_type: String,
    pub stolen: String,
    pub turret_killed: String,
    pub inhib_killed: String,
    pub result: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameData {
    pub game_mode: String,
    pub game_time: f64,
    pub map_name: String,
    pub map_number: u32,
}

/// The set of names one player can appear under in event payloads. The API has
/// used the summoner name, the Riot ID game name and the full `Name#TAG` form
/// at different times, so all are accepted (case-insensitive).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Names(Vec<String>);

impl Names {
    pub fn new<'a>(candidates: impl IntoIterator<Item = &'a str>) -> Self {
        let mut v: Vec<String> = Vec::new();
        for c in candidates {
            let c = c.trim().to_lowercase();
            if !c.is_empty() && !v.contains(&c) {
                v.push(c);
            }
        }
        Names(v)
    }

    pub fn matches(&self, name: &str) -> bool {
        let n = name.trim().to_lowercase();
        !n.is_empty() && self.0.contains(&n)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl ActivePlayer {
    pub fn names(&self) -> Names {
        let full = join_riot_id(&self.riot_id_game_name, &self.riot_id_tag_line);
        Names::new([
            self.riot_id_game_name.as_str(),
            self.summoner_name.as_str(),
            self.riot_id.as_str(),
            full.as_str(),
        ])
    }
}

impl Player {
    pub fn names(&self) -> Names {
        let full = join_riot_id(&self.riot_id_game_name, &self.riot_id_tag_line);
        Names::new([
            self.riot_id_game_name.as_str(),
            self.summoner_name.as_str(),
            self.riot_id.as_str(),
            full.as_str(),
        ])
    }

    /// Best name to show in the UI.
    pub fn display_name(&self) -> &str {
        if !self.riot_id_game_name.is_empty() {
            &self.riot_id_game_name
        } else if !self.summoner_name.is_empty() {
            &self.summoner_name
        } else {
            &self.riot_id
        }
    }
}

fn join_riot_id(name: &str, tag: &str) -> String {
    if name.is_empty() || tag.is_empty() {
        String::new()
    } else {
        format!("{name}#{tag}")
    }
}

impl AllGameData {
    pub fn parse(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    /// The entry in `all_players` for the local player.
    pub fn me(&self) -> Option<&Player> {
        let mine = self.active_player.names();
        self.all_players.iter().find(|p| {
            [&p.riot_id_game_name, &p.summoner_name, &p.riot_id]
                .into_iter()
                .any(|n| mine.matches(n))
        })
    }
}
