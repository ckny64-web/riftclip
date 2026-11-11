//! Decides what the overlay shows and when, and turns game data into overlay
//! messages. Pure state: the caller forwards the returned messages to the
//! overlay process.

use league::ranks::{my_team, RankedPlayer};
use league::GoldReport;
use library::settings::OverlaySettings;
use proto::{Gold, GoldLane, Message, RankRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// No game running (or the match is over).
    Idle,
    /// Game process exists, match not started yet (loading screen).
    Loading,
    InGame,
}

/// Ranks stay up this long after the game starts, then only on the hotkey.
const RANKS_LINGER_SECS: f64 = 10.0;

pub struct OverlayState {
    cfg: OverlaySettings,
    phase: Phase,
    rows: Vec<RankRow>,
    gold: Option<Gold>,
    held: bool,
    gold_on: bool,
    ranks_until: f64,
    shown: (bool, bool),
}

pub fn rank_rows(players: &[RankedPlayer]) -> Vec<RankRow> {
    let mine = my_team(players).to_string();
    let row = |p: &RankedPlayer| RankRow {
        ally: p.team == mine,
        is_me: p.is_me,
        name: p.name.clone(),
        champion: p.champion.clone(),
        // No name and no rank means the client hid this player.
        rank: if p.name.is_empty() && !p.rank.is_ranked() { String::new() } else { p.rank.label() },
        tier: p.rank.tier.clone(),
        win_rate: p.rank.win_rate().map(|w| w as f32),
        games: (p.rank.wins + p.rank.losses).max(0) as u32,
        wins: p.rank.wins.max(0) as u32,
    };
    // Allies first, each side in the order given.
    let allies = players.iter().filter(|p| p.team == mine).map(row);
    let enemies = players.iter().filter(|p| p.team != mine).map(row);
    allies.chain(enemies).collect()
}

pub fn gold_message(report: &GoldReport) -> Gold {
    Gold {
        lanes: report
            .lanes
            .iter()
            .map(|l| GoldLane {
                position: l.position.clone(),
                ally_champion: l.ally_champion.clone(),
                enemy_champion: l.enemy_champion.clone(),
                diff: l.diff,
            })
            .collect(),
        team_diff: report.diff,
    }
}

impl OverlayState {
    pub fn new(cfg: OverlaySettings) -> Self {
        let gold_on = cfg.gold_enabled;
        OverlayState {
            cfg,
            phase: Phase::Idle,
            rows: Vec::new(),
            gold: None,
            held: false,
            gold_on,
            ranks_until: 0.0,
            shown: (false, false),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    fn config_message(&self) -> Message {
        Message::Config { ranks_pos: self.cfg.ranks_pos, gold_pos: self.cfg.gold_pos, scale: self.cfg.scale }
    }

    fn want(&self, now: f64) -> (bool, bool) {
        let ranks = self.cfg.ranks_enabled
            && !self.rows.is_empty()
            && match self.phase {
                Phase::Idle => false,
                Phase::Loading => true,
                Phase::InGame => self.held || now < self.ranks_until,
            };
        let gold = self.cfg.gold_enabled && self.gold_on && self.phase == Phase::InGame && self.gold.is_some();
        (ranks, gold)
    }

    fn sync(&mut self, now: f64, out: &mut Vec<Message>) {
        let want = self.want(now);
        if want != self.shown {
            self.shown = want;
            out.push(Message::Show { ranks: want.0, gold: want.1 });
        }
    }

    /// Move to a new phase. Leaving `Idle` means a fresh overlay process is
    /// listening, so its configuration is (re)sent first.
    pub fn set_phase(&mut self, phase: Phase, now: f64) -> Vec<Message> {
        let mut out = Vec::new();
        if phase == self.phase {
            return out;
        }
        let was = self.phase;
        self.phase = phase;
        match phase {
            Phase::Idle => {
                self.rows.clear();
                self.gold = None;
                self.held = false;
            }
            Phase::Loading | Phase::InGame => {
                if was == Phase::Idle {
                    self.shown = (false, false);
                    self.gold_on = self.cfg.gold_enabled;
                    out.push(self.config_message());
                }
                if phase == Phase::InGame {
                    self.ranks_until = now + RANKS_LINGER_SECS;
                }
            }
        }
        self.sync(now, &mut out);
        out
    }

    pub fn set_ranks(&mut self, players: &[RankedPlayer], now: f64) -> Vec<Message> {
        let rows = rank_rows(players);
        let mut out = Vec::new();
        if rows != self.rows && self.phase != Phase::Idle {
            self.rows = rows.clone();
            out.push(Message::Ranks { rows });
        }
        self.sync(now, &mut out);
        out
    }

    pub fn set_gold(&mut self, report: &GoldReport, now: f64) -> Vec<Message> {
        let gold = gold_message(report);
        let mut out = Vec::new();
        if self.phase == Phase::InGame && self.gold.as_ref() != Some(&gold) {
            self.gold = Some(gold.clone());
            out.push(Message::Gold(gold));
        }
        self.sync(now, &mut out);
        out
    }

    /// The show-ranks hotkey went down (`true`) or up (`false`).
    pub fn hold_ranks(&mut self, held: bool, now: f64) -> Vec<Message> {
        self.held = held;
        if held {
            self.ranks_until = 0.0; // a press ends the automatic showing
        }
        let mut out = Vec::new();
        self.sync(now, &mut out);
        out
    }

    pub fn toggle_gold(&mut self, now: f64) -> Vec<Message> {
        self.gold_on = !self.gold_on;
        let mut out = Vec::new();
        self.sync(now, &mut out);
        out
    }

    /// Call regularly so time-based changes (the ranks linger) take effect.
    pub fn tick(&mut self, now: f64) -> Vec<Message> {
        let mut out = Vec::new();
        self.sync(now, &mut out);
        out
    }

    pub fn set_config(&mut self, cfg: OverlaySettings, now: f64) -> Vec<Message> {
        let mut out = Vec::new();
        if cfg != self.cfg {
            self.cfg = cfg;
            if self.phase != Phase::Idle {
                out.push(self.config_message());
            }
        }
        self.sync(now, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use league::lcu::Rank;
    use league::{gold_report, AllGameData};

    fn players() -> Vec<RankedPlayer> {
        let p = |team: &str, me: bool, name: &str, tier: &str| RankedPlayer {
            team: team.into(),
            is_me: me,
            name: name.into(),
            champion: String::new(),
            position: String::new(),
            rank: Rank { tier: tier.into(), division: if tier.is_empty() { "" } else { "II" }.into(), lp: 10, wins: 6, losses: 4 },
        };
        vec![p("ORDER", false, "Foe", "GOLD"), p("CHAOS", true, "Me", "SILVER"), p("ORDER", false, "", "")]
    }

    fn report() -> GoldReport {
        let data = AllGameData::parse(include_str!("../../league/tests/fixtures/allgamedata_sample.json")).unwrap();
        gold_report(&data, None).unwrap()
    }

    fn show(ranks: bool, gold: bool) -> Message {
        Message::Show { ranks, gold }
    }

    #[test]
    fn rows_put_my_team_first_and_blank_hidden_players() {
        let rows = rank_rows(&players());
        assert_eq!(rows.iter().map(|r| (r.name.as_str(), r.ally)).collect::<Vec<_>>(), vec![("Me", true), ("Foe", false), ("", false)]);
        assert_eq!(rows[0].rank, "Silver II 10 LP");
        assert_eq!((rows[0].win_rate, rows[0].games, rows[0].wins), (Some(60.0), 10, 6));
        assert_eq!(rows[2].rank, ""); // hidden, not "Unranked"
    }

    #[test]
    fn a_whole_game() {
        let mut o = OverlayState::new(OverlaySettings::default());
        // Nothing happens while idle.
        assert!(o.set_ranks(&players(), 0.0).is_empty());
        assert!(o.toggle_gold(0.0).is_empty());
        o.toggle_gold(0.0);

        // Loading screen: config first; ranks show as soon as they arrive.
        let m = o.set_phase(Phase::Loading, 1.0);
        assert!(matches!(m[..], [Message::Config { .. }]));
        let m = o.set_ranks(&players(), 2.0);
        assert!(matches!(m[0], Message::Ranks { .. }));
        assert_eq!(m[1], show(true, false));
        assert!(o.set_ranks(&players(), 3.0).is_empty()); // unchanged rows are not resent

        // Game starts at t=20: ranks linger, gold appears with its first data.
        assert!(o.set_phase(Phase::InGame, 20.0).is_empty());
        let m = o.set_gold(&report(), 21.0);
        assert!(matches!(&m[0], Message::Gold(g) if g.team_diff == 783 && g.lanes.len() == 5));
        assert_eq!(m[1], show(true, true));
        assert!(o.set_gold(&report(), 22.0).is_empty()); // unchanged gold is not resent
        assert!(o.tick(29.9).is_empty());
        assert_eq!(o.tick(30.0), vec![show(false, true)]); // linger over

        // Hold the ranks key, release it, toggle gold off and on.
        assert_eq!(o.hold_ranks(true, 40.0), vec![show(true, true)]);
        assert_eq!(o.hold_ranks(false, 41.0), vec![show(false, true)]);
        assert_eq!(o.toggle_gold(42.0), vec![show(false, false)]);
        assert_eq!(o.toggle_gold(43.0), vec![show(false, true)]);

        // Match over: everything hides and is forgotten.
        assert_eq!(o.set_phase(Phase::Idle, 50.0), vec![show(false, false)]);
        // Next game starts from a clean slate, with config resent.
        let m = o.set_phase(Phase::Loading, 60.0);
        assert!(matches!(m[..], [Message::Config { .. }]));
    }

    #[test]
    fn pressing_the_key_during_the_linger_ends_it() {
        let mut o = OverlayState::new(OverlaySettings::default());
        o.set_phase(Phase::Loading, 0.0);
        o.set_ranks(&players(), 0.0);
        o.set_phase(Phase::InGame, 5.0);
        assert!(o.hold_ranks(true, 6.0).is_empty()); // already visible
        assert_eq!(o.hold_ranks(false, 7.0), vec![show(false, false)]);
    }

    #[test]
    fn disabled_panels_never_show() {
        let cfg = OverlaySettings { ranks_enabled: false, gold_enabled: false, ..Default::default() };
        let mut o = OverlayState::new(cfg);
        o.set_phase(Phase::InGame, 0.0);
        let m = o.set_ranks(&players(), 0.0);
        assert!(matches!(m[..], [Message::Ranks { .. }]));
        assert!(matches!(o.set_gold(&report(), 0.0)[..], [Message::Gold(_)]));
        assert!(o.hold_ranks(true, 1.0).is_empty());
        assert!(o.toggle_gold(1.0).is_empty());

        // Enabling them in settings takes effect straight away.
        let m = o.set_config(OverlaySettings::default(), 2.0);
        assert!(matches!(m[0], Message::Config { .. }));
        assert_eq!(m[1], show(true, true)); // ranks key still held; gold was toggled on above
    }
}
