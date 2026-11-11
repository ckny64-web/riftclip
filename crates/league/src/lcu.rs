//! Pure parsing for the League client (LCU) API: connection details and the
//! response shapes this app reads. The endpoints are undocumented, so every
//! extractor is lenient and returns defaults for anything missing.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How to reach the local League client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub port: u16,
    pub token: String,
}

impl Credentials {
    /// Parse the client's `lockfile`: `name:pid:port:password:protocol`.
    pub fn from_lockfile(contents: &str) -> Option<Self> {
        let parts: Vec<&str> = contents.trim().split(':').collect();
        if parts.len() < 5 {
            return None;
        }
        Some(Credentials { port: parts[2].parse().ok()?, token: parts[3].to_string() })
    }

    /// Parse the `LeagueClientUx.exe` command line (`--app-port=…`, `--remoting-auth-token=…`).
    pub fn from_cmdline<S: AsRef<str>>(args: &[S]) -> Option<Self> {
        let mut port = None;
        let mut token = None;
        for a in args {
            // Arguments may arrive as one string or split, with or without quotes.
            for piece in a.as_ref().split_whitespace() {
                let piece = piece.trim_matches('"');
                if let Some(v) = piece.strip_prefix("--app-port=") {
                    port = v.parse().ok();
                } else if let Some(v) = piece.strip_prefix("--remoting-auth-token=") {
                    token = Some(v.to_string());
                }
            }
        }
        Some(Credentials { port: port?, token: token? })
    }
}

/// One ranked queue entry for a player.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rank {
    /// "IRON" … "CHALLENGER"; empty when unranked.
    pub tier: String,
    /// "I" … "IV"; empty for Master and above or unranked.
    pub division: String,
    pub lp: i64,
    pub wins: i64,
    pub losses: i64,
}

impl Rank {
    pub fn is_ranked(&self) -> bool {
        !self.tier.is_empty()
    }

    /// Win rate in percent, when it can be known. The League client reports
    /// zero losses for every player except the signed-in one, so a record
    /// without losses is treated as "unknown" rather than 100%.
    pub fn win_rate(&self) -> Option<f64> {
        let games = self.wins + self.losses;
        (self.losses > 0 && games > 0).then(|| self.wins as f64 * 100.0 / games as f64)
    }

    /// e.g. "Gold II 45 LP", "Master 312 LP", "Unranked".
    pub fn label(&self) -> String {
        if !self.is_ranked() {
            return "Unranked".to_string();
        }
        let mut tier = self.tier.to_lowercase();
        if let Some(first) = tier.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        if self.division.is_empty() {
            format!("{tier} {} LP", self.lp)
        } else {
            format!("{tier} {} {} LP", self.division, self.lp)
        }
    }
}

pub const QUEUE_SOLO: &str = "RANKED_SOLO_5x5";
pub const QUEUE_FLEX: &str = "RANKED_FLEX_SR";

/// Extract one queue from a `/lol-ranked/v1/ranked-stats/{puuid}` response.
pub fn rank_from_stats(stats: &Value, queue: &str) -> Rank {
    let q = match stats.pointer(&format!("/queueMap/{queue}")) {
        Some(q) => q,
        None => return Rank::default(),
    };
    let s = |k: &str| q.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let n = |k: &str| q.get(k).and_then(Value::as_i64).unwrap_or(0);
    let mut tier = s("tier");
    if tier.eq_ignore_ascii_case("NONE") || tier.eq_ignore_ascii_case("UNRANKED") {
        tier.clear();
    }
    let mut division = s("division");
    if division.eq_ignore_ascii_case("NA") || tier.is_empty() {
        division.clear();
    }
    Rank { tier, division, lp: n("leaguePoints"), wins: n("wins"), losses: n("losses") }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionPlayer {
    pub puuid: String,
    pub champion_id: i64,
    /// 1 = `teamOne` (blue / ORDER), 2 = `teamTwo` (red / CHAOS).
    pub team: u8,
}

/// The parts of `/lol-gameflow/v1/session` this app uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameflowSession {
    pub phase: String,
    pub queue_id: i64,
    pub queue_description: String,
    pub game_id: i64,
    pub players: Vec<SessionPlayer>,
}

pub fn parse_gameflow_session(v: &Value) -> GameflowSession {
    let mut players = Vec::new();
    for (key, team) in [("teamOne", 1u8), ("teamTwo", 2u8)] {
        if let Some(list) = v.pointer(&format!("/gameData/{key}")).and_then(Value::as_array) {
            for p in list {
                let puuid = p.get("puuid").and_then(Value::as_str).unwrap_or("").to_string();
                if puuid.is_empty() {
                    continue; // bots have no puuid
                }
                players.push(SessionPlayer {
                    puuid,
                    champion_id: p.get("championId").and_then(Value::as_i64).unwrap_or(0),
                    team,
                });
            }
        }
    }
    // Custom games can list the same player on both teams. Keep one entry per
    // player: the one on the bigger team (the other list is then usually a
    // stray single entry), or the first on a tie.
    let size = |team: u8| players.iter().filter(|p| p.team == team).count();
    let (one, two) = (size(1), size(2));
    let mut unique: Vec<SessionPlayer> = Vec::new();
    for p in players {
        match unique.iter_mut().find(|u| u.puuid == p.puuid) {
            None => unique.push(p),
            Some(existing) => {
                let bigger = |team: u8| if team == 1 { one } else { two };
                if bigger(p.team) > bigger(existing.team) {
                    *existing = p;
                }
            }
        }
    }
    let players = unique;

    GameflowSession {
        phase: v.get("phase").and_then(Value::as_str).unwrap_or("").to_string(),
        queue_id: v.pointer("/gameData/queue/id").and_then(Value::as_i64).unwrap_or(0),
        queue_description: v
            .pointer("/gameData/queue/description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        game_id: v.pointer("/gameData/gameId").and_then(Value::as_i64).unwrap_or(0),
        players,
    }
}

/// Riot ID from a `/lol-summoner/v2/summoners/puuid/{puuid}` response: `(gameName, tagLine)`.
pub fn riot_id_from_summoner(v: &Value) -> Option<(String, String)> {
    let name = v.get("gameName").and_then(Value::as_str)?;
    let tag = v.get("tagLine").and_then(Value::as_str).unwrap_or("");
    (!name.is_empty()).then(|| (name.to_string(), tag.to_string()))
}

/// A short queue name for the library when the client gives no description.
pub fn queue_name(queue_id: i64) -> &'static str {
    match queue_id {
        400 => "Normal Draft",
        420 => "Ranked Solo/Duo",
        430 => "Normal Blind",
        440 => "Ranked Flex",
        450 => "ARAM",
        480 | 490 => "Quickplay",
        700 => "Clash",
        1700 => "Arena",
        0 => "Custom",
        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lockfile() {
        let c = Credentials::from_lockfile("LeagueClient:1234:54321:s3cretTok:https").unwrap();
        assert_eq!(c, Credentials { port: 54321, token: "s3cretTok".into() });
        assert!(Credentials::from_lockfile("garbage").is_none());
    }

    #[test]
    fn cmdline_split_and_joined() {
        let split = ["LeagueClientUx.exe", "\"--remoting-auth-token=abc\"", "--app-port=5000"];
        assert_eq!(Credentials::from_cmdline(&split), Some(Credentials { port: 5000, token: "abc".into() }));
        let joined = ["\"C:/x/LeagueClientUx.exe\" \"--app-port=6000\" \"--remoting-auth-token=zz\""];
        assert_eq!(Credentials::from_cmdline(&joined), Some(Credentials { port: 6000, token: "zz".into() }));
        assert_eq!(Credentials::from_cmdline(&["--app-port=1"]), None);
    }

    #[test]
    fn ranks() {
        let stats = json!({"queueMap": {
            "RANKED_SOLO_5x5": {"tier": "GOLD", "division": "II", "leaguePoints": 45, "wins": 30, "losses": 20},
            "RANKED_FLEX_SR": {"tier": "MASTER", "division": "NA", "leaguePoints": 312, "wins": 1, "losses": 0},
            "RANKED_TFT": {"tier": "NONE", "division": "NA", "leaguePoints": 0, "wins": 0, "losses": 0}
        }});
        let solo = rank_from_stats(&stats, QUEUE_SOLO);
        assert_eq!(solo.label(), "Gold II 45 LP");
        assert_eq!(solo.win_rate(), Some(60.0));
        assert_eq!(rank_from_stats(&stats, QUEUE_FLEX).label(), "Master 312 LP");
        let none = rank_from_stats(&stats, "RANKED_TFT");
        assert_eq!(none.label(), "Unranked");
        assert_eq!(none.win_rate(), None);
        assert_eq!(rank_from_stats(&json!({}), QUEUE_SOLO), Rank::default());
    }

    #[test]
    fn gameflow() {
        let v = json!({"phase": "InProgress", "gameData": {
            "gameId": 77, "queue": {"id": 420, "description": "Ranked Solo/Duo"},
            "teamOne": [{"puuid": "a", "championId": 103}, {"puuid": "", "championId": 1}],
            "teamTwo": [{"puuid": "b", "championId": 238}]
        }});
        let s = parse_gameflow_session(&v);
        assert_eq!(s.phase, "InProgress");
        assert_eq!((s.queue_id, s.game_id), (420, 77));
        assert_eq!(s.players.len(), 2);
        assert_eq!(s.players[1], SessionPlayer { puuid: "b".into(), champion_id: 238, team: 2 });
        assert_eq!(parse_gameflow_session(&json!({})), GameflowSession::default());
    }

    #[test]
    fn gameflow_drops_a_player_listed_on_both_teams() {
        // Shape seen in a multiplayer Practice Tool game: me alone in teamOne,
        // and again among the five in teamTwo.
        let v = json!({"gameData": {
            "teamOne": [{"puuid": "me", "championId": 114}],
            "teamTwo": [{"puuid": "a", "championId": 40}, {"puuid": "me", "championId": 114},
                        {"puuid": "b", "championId": 517}, {"puuid": "c", "championId": 245}, {"puuid": "d", "championId": 21}]
        }});
        let s = parse_gameflow_session(&v);
        assert_eq!(s.players.len(), 5);
        let me: Vec<_> = s.players.iter().filter(|p| p.puuid == "me").collect();
        assert_eq!(me.len(), 1);
        assert_eq!(me[0].team, 2);
        // With equal team sizes the first listing wins.
        let tie = json!({"gameData": {"teamOne": [{"puuid": "x"}], "teamTwo": [{"puuid": "x"}]}});
        assert_eq!(parse_gameflow_session(&tie).players, vec![SessionPlayer { puuid: "x".into(), champion_id: 0, team: 1 }]);
    }

    #[test]
    fn win_rate_is_unknown_without_losses() {
        // Other players come back as e.g. 393 wins, 0 losses.
        let other = Rank { tier: "PLATINUM".into(), division: "II".into(), lp: 83, wins: 393, losses: 0 };
        assert_eq!(other.win_rate(), None);
        let me = Rank { wins: 105, losses: 101, ..other };
        assert!((me.win_rate().unwrap() - 50.97).abs() < 0.01);
    }
}
