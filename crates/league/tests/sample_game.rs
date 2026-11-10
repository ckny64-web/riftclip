//! End-to-end checks of parsing, event classification and the gold model
//! against the hand-written sample game.

use league::gold::{item_gold, PriceTable};
use league::{classify, gold_report, AllGameData, EventTracker, EventType, GameEvent, Involvement, Roster};

fn sample() -> AllGameData {
    AllGameData::parse(include_str!("fixtures/allgamedata_sample.json")).unwrap()
}

fn events(data: &AllGameData) -> Vec<GameEvent> {
    let roster = Roster::from_game(data);
    data.events.events.iter().filter_map(|e| classify(e, &roster)).collect()
}

#[test]
fn finds_local_player() {
    let data = sample();
    let me = data.me().expect("local player present");
    assert_eq!(me.champion_name, "Ahri");
    assert_eq!(me.team, "ORDER");
    assert_eq!(Roster::from_game(&data).my_team(), "ORDER");
}

#[test]
fn classifies_every_tracked_event() {
    use EventType::*;
    use Involvement::*;
    let data = sample();
    let got: Vec<(EventType, Involvement)> = events(&data).iter().map(|e| (e.ty, e.involvement)).collect();
    let want = vec![
        (GameStart, None),
        (ChampionKill, Killer),  // I kill Zed
        (FirstBlood, Killer),
        (ChampionKill, Victim),  // Lee Sin kills me
        (Dragon, Assist),        // ally jungler, I assist
        (ChampionKill, Assist),  // ally ADC kills, I assist
        (ChampionKill, Killer),
        (ChampionKill, Killer),
        (Multikill, Killer),
        (Herald, Enemy),
        (Turret, Assist),        // minion takes enemy turret, I assist
        (ChampionKill, Enemy),   // enemy turret kills my top laner
        (ChampionKill, Enemy),
        (Turret, Enemy),         // enemy takes our turret
        (Baron, Killer),
        (Inhibitor, Team),
        (Ace, Team),
        (GameEnd, None),
    ];
    assert_eq!(got, want);
}

#[test]
fn details_use_champion_names() {
    let data = sample();
    let ev = events(&data);
    assert_eq!(ev[1].detail, "Ahri killed Zed");
    assert_eq!(ev[4].detail, "Fire dragon taken by Vi");
    assert_eq!(ev[8].detail, "Double kill by Ahri");
    assert_eq!(ev[14].detail, "Baron taken by Ahri (stolen)");
    assert_eq!(ev.last().unwrap().detail, "Victory");
    assert_eq!(ev[1].game_time, 185.2);
}

#[test]
fn minion_turret_without_my_assist_goes_to_the_right_team() {
    let mut data = sample();
    for e in &mut data.events.events {
        e.assisters.clear();
    }
    let ev = events(&data);
    let turret = ev.iter().find(|e| e.ty == EventType::Turret).unwrap();
    // Turret_T2 is a CHAOS turret, so ORDER (my team) destroyed it.
    assert_eq!(turret.involvement, Involvement::Team);
}

#[test]
fn name_matching_accepts_summoner_name_and_ignores_case() {
    let mut data = sample();
    // Older payloads: kills reported by summoner name, Riot ID fields absent.
    data.active_player.riot_id_game_name.clear();
    data.active_player.riot_id.clear();
    data.active_player.summoner_name = "MID ME".into();
    for p in &mut data.all_players {
        if p.riot_id_game_name == "Mid Me" {
            p.summoner_name = "Mid Me".into();
            p.riot_id_game_name.clear();
            p.riot_id.clear();
        }
    }
    let ev = events(&data);
    assert_eq!(ev[1].involvement, Involvement::Killer);
    assert_eq!(ev[3].involvement, Involvement::Victim);
}

#[test]
fn tracker_yields_each_event_once() {
    let data = sample();
    let all = &data.events.events;
    let mut t = EventTracker::new();
    assert_eq!(t.take_new(&all[..5]).len(), 5);
    assert_eq!(t.take_new(&all[..5]).len(), 0);
    let fresh = t.take_new(&all[..8]);
    assert_eq!(fresh.iter().map(|e| e.event_id).collect::<Vec<_>>(), vec![5, 6, 7]);
    assert_eq!(t.take_new(all).len(), all.len() - 8);
}

#[test]
fn gold_per_lane_and_total() {
    let data = sample();
    let r = gold_report(&data, Option::None).unwrap();
    let lanes: Vec<(&str, u32, u32, i64)> =
        r.lanes.iter().map(|l| (l.position.as_str(), l.ally_gold, l.enemy_gold, l.diff)).collect();
    assert_eq!(
        lanes,
        vec![
            ("TOP", 3633, 3300, 333),
            ("JUNGLE", 3000, 3350, -350),
            ("MIDDLE", 4050, 2800, 1250), // 2850 + 1100 + 2x50 + ward 0
            ("BOTTOM", 3450, 3900, -450),
            ("UTILITY", 2200, 2200, 0),
        ]
    );
    assert_eq!((r.ally_total, r.enemy_total, r.diff), (16333, 15550, 783));
    assert_eq!(r.lanes[2].ally_champion, "Ahri");
    assert_eq!(r.lanes[2].enemy_champion, "Zed");
}

#[test]
fn gold_without_positions_has_totals_only() {
    let mut data = sample();
    for p in &mut data.all_players {
        p.position.clear();
    }
    let r = gold_report(&data, Option::None).unwrap();
    assert!(r.lanes.is_empty());
    assert_eq!(r.diff, 783);
}

#[test]
fn price_table_overrides_api_price() {
    let table = PriceTable::from_ddragon_json(include_str!("fixtures/ddragon_item_sample.json")).unwrap();
    assert_eq!(table.len(), 2);
    assert_eq!(table.get(3078), Some(3333));
    let mut data = sample();
    let top = &mut data.all_players[0];
    top.items[0].price = 1; // wrong price from the API; the table wins
    assert_eq!(item_gold(top, Some(&table)), 3333 + 300);
    assert_eq!(item_gold(top, Option::None), 1 + 300);
}

#[test]
fn tolerates_unknown_and_missing_fields() {
    let data = AllGameData::parse(r#"{"allPlayers":[{"championName":"Ahri","newField":1}],"somethingNew":{}}"#).unwrap();
    assert_eq!(data.all_players.len(), 1);
    assert!(data.me().is_none());
    assert!(gold_report(&data, Option::None).is_none());
}
