//! Item-gold estimate: the sum of the prices of the items each player holds.
//! Unspent gold of other players is not exposed by the game, so this is the
//! same approximation other companion apps show.

use crate::live::{AllGameData, Player};
use serde::Serialize;
use std::collections::HashMap;

/// Item id -> total price, loaded from Data Dragon's `item.json`.
#[derive(Debug, Clone, Default)]
pub struct PriceTable(HashMap<u32, u32>);

impl PriceTable {
    /// Parse `https://ddragon.leagueoflegends.com/cdn/<patch>/data/en_US/item.json`.
    pub fn from_ddragon_json(json: &str) -> serde_json::Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json)?;
        let mut map = HashMap::new();
        if let Some(data) = v.get("data").and_then(|d| d.as_object()) {
            for (id, item) in data {
                let total = item.pointer("/gold/total").and_then(|t| t.as_u64());
                if let (Ok(id), Some(total)) = (id.parse::<u32>(), total) {
                    map.insert(id, total as u32);
                }
            }
        }
        Ok(PriceTable(map))
    }

    pub fn get(&self, item_id: u32) -> Option<u32> {
        self.0.get(&item_id).copied()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Gold value of a player's inventory. Prices come from `prices` when given,
/// otherwise from the `price` field the live API attaches to each item.
pub fn item_gold(player: &Player, prices: Option<&PriceTable>) -> u32 {
    player
        .items
        .iter()
        .map(|i| {
            let unit = prices.and_then(|p| p.get(i.item_id)).unwrap_or(i.price);
            unit * i.count.max(1)
        })
        .sum()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LaneGold {
    pub position: String,
    pub ally_champion: String,
    pub ally_gold: u32,
    pub enemy_champion: String,
    pub enemy_gold: u32,
    /// Ally minus enemy; positive means my side is ahead.
    pub diff: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct GoldReport {
    /// One row per lane, top to support. Empty when the mode has no positions (ARAM etc.).
    pub lanes: Vec<LaneGold>,
    pub ally_total: u32,
    pub enemy_total: u32,
    pub diff: i64,
}

pub const POSITIONS: [&str; 5] = ["TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY"];

/// Compare item gold between my team and the enemy team. Returns `None` if the
/// local player cannot be found in the player list.
pub fn gold_report(data: &AllGameData, prices: Option<&PriceTable>) -> Option<GoldReport> {
    let my_team = data.me()?.team.clone();
    let (allies, enemies): (Vec<&Player>, Vec<&Player>) =
        data.all_players.iter().partition(|p| p.team == my_team);

    let ally_total: u32 = allies.iter().map(|p| item_gold(p, prices)).sum();
    let enemy_total: u32 = enemies.iter().map(|p| item_gold(p, prices)).sum();

    let mut lanes = Vec::new();
    for pos in POSITIONS {
        let ally = allies.iter().find(|p| p.position == pos);
        let enemy = enemies.iter().find(|p| p.position == pos);
        if let (Some(a), Some(e)) = (ally, enemy) {
            let (ag, eg) = (item_gold(a, prices), item_gold(e, prices));
            lanes.push(LaneGold {
                position: pos.to_string(),
                ally_champion: a.champion_name.clone(),
                ally_gold: ag,
                enemy_champion: e.champion_name.clone(),
                enemy_gold: eg,
                diff: ag as i64 - eg as i64,
            });
        }
    }

    Some(GoldReport { lanes, ally_total, enemy_total, diff: ally_total as i64 - enemy_total as i64 })
}
