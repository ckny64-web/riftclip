//! Joins what the League client knows about the ten players (ranks, Riot IDs)
//! with what the game reports (champions, positions, teams).

use crate::ddragon::ChampionNames;
use crate::lcu::Rank;
use crate::live::AllGameData;

/// One player as the League client describes them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClientPlayer {
    pub puuid: String,
    /// 1 = blue side (ORDER), 2 = red side (CHAOS).
    pub team: u8,
    pub champion_id: i64,
    /// Empty when the client would not reveal the name.
    pub game_name: String,
    pub tag_line: String,
    pub rank: Rank,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RankedPlayer {
    /// "ORDER" or "CHAOS".
    pub team: String,
    pub is_me: bool,
    pub name: String,
    pub champion: String,
    pub position: String,
    pub rank: Rank,
}

fn team_name(team: u8) -> &'static str {
    if team == 2 {
        "CHAOS"
    } else {
        "ORDER"
    }
}

/// Build the rank table.
///
/// With `live` data (in game) the rows follow the game's own player list, so
/// champions, positions and order are exact, and ranks are attached by Riot
/// game name. Without it (loading screen) the rows come from the client's
/// list, with champion names looked up by id where `champions` is available.
pub fn join(
    client: &[ClientPlayer],
    live: Option<&AllGameData>,
    champions: &ChampionNames,
    my_puuid: &str,
) -> Vec<RankedPlayer> {
    match live.filter(|d| !d.all_players.is_empty()) {
        Some(data) => {
            let me = data.active_player.names();
            data.all_players
                .iter()
                .map(|p| {
                    let names = p.names();
                    let found = client.iter().find(|c| {
                        let full = format!("{}#{}", c.game_name, c.tag_line);
                        names.matches(&c.game_name) || names.matches(&full)
                    });
                    RankedPlayer {
                        team: p.team.clone(),
                        is_me: [&p.riot_id_game_name, &p.summoner_name, &p.riot_id].into_iter().any(|n| me.matches(n)),
                        name: p.display_name().to_string(),
                        champion: p.champion_name.clone(),
                        position: p.position.clone(),
                        rank: found.map(|c| c.rank.clone()).unwrap_or_default(),
                    }
                })
                .collect()
        }
        None => client
            .iter()
            .map(|c| RankedPlayer {
                team: team_name(c.team).to_string(),
                is_me: !my_puuid.is_empty() && c.puuid == my_puuid,
                name: c.game_name.clone(),
                champion: champions.get(c.champion_id).unwrap_or("").to_string(),
                position: String::new(),
                rank: c.rank.clone(),
            })
            .collect(),
    }
}

/// The local player's team in a rank table, defaulting to blue side.
pub fn my_team(rows: &[RankedPlayer]) -> &str {
    rows.iter().find(|r| r.is_me).map(|r| r.team.as_str()).unwrap_or("ORDER")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank(tier: &str, div: &str, lp: i64) -> Rank {
        Rank { tier: tier.into(), division: div.into(), lp, wins: 10, losses: 10 }
    }

    fn client() -> Vec<ClientPlayer> {
        vec![
            ClientPlayer { puuid: "me".into(), team: 1, champion_id: 103, game_name: "mid me".into(), tag_line: "EUW".into(), rank: rank("GOLD", "II", 45) },
            ClientPlayer { puuid: "foe".into(), team: 2, champion_id: 238, game_name: "MidFoe".into(), tag_line: "KR1".into(), rank: rank("PLATINUM", "IV", 3) },
            ClientPlayer { puuid: "hidden".into(), team: 2, champion_id: 999, ..Default::default() },
        ]
    }

    #[test]
    fn loading_screen_uses_client_list() {
        let champs = ChampionNames::from_json(r#"{"data":{"Ahri":{"key":"103","name":"Ahri"},"Zed":{"key":"238","name":"Zed"}}}"#).unwrap();
        let rows = join(&client(), None, &champs, "me");
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].team.as_str(), rows[0].is_me, rows[0].champion.as_str()), ("ORDER", true, "Ahri"));
        assert_eq!((rows[1].team.as_str(), rows[1].is_me, rows[1].champion.as_str()), ("CHAOS", false, "Zed"));
        assert_eq!(rows[1].rank.label(), "Platinum IV 3 LP");
        assert_eq!((rows[2].name.as_str(), rows[2].champion.as_str()), ("", "")); // hidden player, unknown champion
        assert_eq!(my_team(&rows), "ORDER");
        // Without knowing who I am nobody is flagged.
        assert!(join(&client(), None, &champs, "").iter().all(|r| !r.is_me));
    }

    #[test]
    fn in_game_follows_live_list_and_attaches_ranks_by_name() {
        let live = AllGameData::parse(include_str!("../tests/fixtures/allgamedata_sample.json")).unwrap();
        let rows = join(&client(), Some(&live), &ChampionNames::default(), "");
        assert_eq!(rows.len(), 10);
        let me = rows.iter().find(|r| r.is_me).unwrap();
        assert_eq!((me.champion.as_str(), me.position.as_str()), ("Ahri", "MIDDLE"));
        assert_eq!(me.rank.label(), "Gold II 45 LP"); // matched despite different letter case
        let zed = rows.iter().find(|r| r.champion == "Zed").unwrap();
        assert_eq!(zed.rank.tier, "PLATINUM");
        let garen = rows.iter().find(|r| r.champion == "Garen").unwrap();
        assert!(!garen.rank.is_ranked()); // the client had nothing for this player
        assert_eq!(my_team(&rows), "ORDER");
    }

    #[test]
    fn empty_live_data_falls_back_to_client_list() {
        let rows = join(&client(), Some(&AllGameData::default()), &ChampionNames::default(), "me");
        assert_eq!(rows.len(), 3);
    }
}
