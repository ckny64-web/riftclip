//! Turns raw Live Client events into typed events tagged with how the local
//! player was involved.

use crate::live::{AllGameData, Names, RawEvent};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    GameStart,
    ChampionKill,
    Multikill,
    FirstBlood,
    Ace,
    Dragon,
    Baron,
    Herald,
    Voidgrubs,
    Atakhan,
    Turret,
    Inhibitor,
    GameEnd,
    /// Added by the manual-mark hotkey, never by the game.
    Manual,
}

impl EventType {
    pub const ALL: [EventType; 14] = [
        EventType::GameStart,
        EventType::ChampionKill,
        EventType::Multikill,
        EventType::FirstBlood,
        EventType::Ace,
        EventType::Dragon,
        EventType::Baron,
        EventType::Herald,
        EventType::Voidgrubs,
        EventType::Atakhan,
        EventType::Turret,
        EventType::Inhibitor,
        EventType::GameEnd,
        EventType::Manual,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EventType::GameStart => "game_start",
            EventType::ChampionKill => "champion_kill",
            EventType::Multikill => "multikill",
            EventType::FirstBlood => "first_blood",
            EventType::Ace => "ace",
            EventType::Dragon => "dragon",
            EventType::Baron => "baron",
            EventType::Herald => "herald",
            EventType::Voidgrubs => "voidgrubs",
            EventType::Atakhan => "atakhan",
            EventType::Turret => "turret",
            EventType::Inhibitor => "inhibitor",
            EventType::GameEnd => "game_end",
            EventType::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    pub fn is_objective(self) -> bool {
        matches!(
            self,
            EventType::Dragon
                | EventType::Baron
                | EventType::Herald
                | EventType::Voidgrubs
                | EventType::Atakhan
                | EventType::Turret
                | EventType::Inhibitor
        )
    }
}

/// How the local player relates to an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Involvement {
    /// I got the kill / took the objective / got the multikill.
    Killer,
    /// I died.
    Victim,
    /// I assisted.
    Assist,
    /// My team did it without me.
    Team,
    /// The enemy team did it (and I was not the victim).
    Enemy,
    /// Not attributable (game start, game end, manual marks).
    None,
}

impl Involvement {
    pub const ALL: [Involvement; 6] = [
        Involvement::Killer,
        Involvement::Victim,
        Involvement::Assist,
        Involvement::Team,
        Involvement::Enemy,
        Involvement::None,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Involvement::Killer => "killer",
            Involvement::Victim => "victim",
            Involvement::Assist => "assist",
            Involvement::Team => "team",
            Involvement::Enemy => "enemy",
            Involvement::None => "none",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameEvent {
    /// The game's own event id (monotonic within a match).
    pub id: u32,
    pub ty: EventType,
    /// Seconds on the game clock.
    pub game_time: f64,
    pub involvement: Involvement,
    /// Short human-readable description, e.g. "Ahri killed Zed".
    pub detail: String,
}

#[derive(Debug, Clone)]
struct RosterEntry {
    names: Names,
    team: String,
    champion: String,
}

/// Who is in the game, used to attribute events to teams and champions.
#[derive(Debug, Clone, Default)]
pub struct Roster {
    me: Names,
    my_team: String,
    players: Vec<RosterEntry>,
}

impl Roster {
    pub fn from_game(data: &AllGameData) -> Self {
        let my_team = data.me().map(|p| p.team.clone()).unwrap_or_default();
        Roster {
            me: data.active_player.names(),
            my_team,
            players: data
                .all_players
                .iter()
                .map(|p| RosterEntry {
                    names: p.names(),
                    team: p.team.clone(),
                    champion: p.champion_name.clone(),
                })
                .collect(),
        }
    }

    pub fn my_team(&self) -> &str {
        &self.my_team
    }

    fn find(&self, name: &str) -> Option<&RosterEntry> {
        self.players.iter().find(|p| p.names.matches(name))
    }

    fn is_me(&self, name: &str) -> bool {
        self.me.matches(name)
    }

    /// Champion name for a player, or the raw name for non-players (turrets, minions).
    fn label(&self, name: &str) -> String {
        match self.find(name) {
            Some(p) if !p.champion.is_empty() => p.champion.clone(),
            _ if name.is_empty() => "Unknown".to_string(),
            _ => name.to_string(),
        }
    }

    fn side_of_team(&self, team: &str) -> Involvement {
        if team.is_empty() || self.my_team.is_empty() {
            Involvement::None
        } else if team.eq_ignore_ascii_case(&self.my_team) {
            Involvement::Team
        } else {
            Involvement::Enemy
        }
    }

    /// Involvement for something done by `actor` with `assisters`.
    /// `fallback_team` is used when the actor is not a player.
    fn actor_involvement(&self, actor: &str, assisters: &[String], fallback_team: &str) -> Involvement {
        if self.is_me(actor) {
            return Involvement::Killer;
        }
        if assisters.iter().any(|a| self.is_me(a)) {
            return Involvement::Assist;
        }
        match self.find(actor) {
            Some(p) => self.side_of_team(&p.team),
            None => self.side_of_team(fallback_team),
        }
    }
}

/// Structures are named `Turret_T1_…` / `Barracks_T2_…`; T1 belongs to ORDER
/// and T2 to CHAOS. Returns the team that *destroyed* the structure.
fn destroyer_of_structure(structure: &str) -> &'static str {
    if structure.contains("_T1_") {
        "CHAOS"
    } else if structure.contains("_T2_") {
        "ORDER"
    } else {
        ""
    }
}

fn other_team(team: &str) -> &'static str {
    match team {
        "ORDER" => "CHAOS",
        "CHAOS" => "ORDER",
        _ => "",
    }
}

/// Convert one raw event. Returns `None` for event names that are not tracked
/// (e.g. `MinionsSpawning`, `FirstBrick`).
pub fn classify(raw: &RawEvent, roster: &Roster) -> Option<GameEvent> {
    let (ty, involvement, detail) = match raw.event_name.as_str() {
        "GameStart" => (EventType::GameStart, Involvement::None, "Game started".to_string()),
        "ChampionKill" => {
            let involvement = if roster.is_me(&raw.killer_name) {
                Involvement::Killer
            } else if roster.is_me(&raw.victim_name) {
                Involvement::Victim
            } else if raw.assisters.iter().any(|a| roster.is_me(a)) {
                Involvement::Assist
            } else {
                // If the killer is a turret or minion, credit the team opposite the victim.
                let victim_team = roster.find(&raw.victim_name).map(|p| p.team.as_str()).unwrap_or("");
                roster.actor_involvement(&raw.killer_name, &[], other_team(victim_team))
            };
            let detail = format!("{} killed {}", roster.label(&raw.killer_name), roster.label(&raw.victim_name));
            (EventType::ChampionKill, involvement, detail)
        }
        "Multikill" => {
            let name = match raw.kill_streak {
                2 => "Double kill",
                3 => "Triple kill",
                4 => "Quadra kill",
                5 => "Penta kill",
                _ => "Multikill",
            };
            (
                EventType::Multikill,
                roster.actor_involvement(&raw.killer_name, &[], ""),
                format!("{name} by {}", roster.label(&raw.killer_name)),
            )
        }
        "FirstBlood" => (
            EventType::FirstBlood,
            roster.actor_involvement(&raw.recipient, &[], ""),
            format!("First blood to {}", roster.label(&raw.recipient)),
        ),
        "Ace" => (
            EventType::Ace,
            roster.actor_involvement(&raw.acer, &[], &raw.acing_team),
            format!("Ace by {}", roster.label(&raw.acer)),
        ),
        "DragonKill" | "BaronKill" | "HeraldKill" | "HordeKill" | "AtakhanKill" => {
            let (ty, what) = match raw.event_name.as_str() {
                "DragonKill" if raw.dragon_type.is_empty() => (EventType::Dragon, "Dragon".to_string()),
                "DragonKill" => (EventType::Dragon, format!("{} dragon", raw.dragon_type)),
                "BaronKill" => (EventType::Baron, "Baron".to_string()),
                "HeraldKill" => (EventType::Herald, "Rift Herald".to_string()),
                "HordeKill" => (EventType::Voidgrubs, "Voidgrub".to_string()),
                _ => (EventType::Atakhan, "Atakhan".to_string()),
            };
            let stolen = if raw.stolen.eq_ignore_ascii_case("true") { " (stolen)" } else { "" };
            (
                ty,
                roster.actor_involvement(&raw.killer_name, &raw.assisters, ""),
                format!("{what} taken by {}{stolen}", roster.label(&raw.killer_name)),
            )
        }
        "TurretKilled" => (
            EventType::Turret,
            roster.actor_involvement(&raw.killer_name, &raw.assisters, destroyer_of_structure(&raw.turret_killed)),
            format!("Turret destroyed by {}", roster.label(&raw.killer_name)),
        ),
        "InhibKilled" => (
            EventType::Inhibitor,
            roster.actor_involvement(&raw.killer_name, &raw.assisters, destroyer_of_structure(&raw.inhib_killed)),
            format!("Inhibitor destroyed by {}", roster.label(&raw.killer_name)),
        ),
        "GameEnd" => {
            let detail = match raw.result.as_str() {
                "Win" => "Victory",
                "Lose" => "Defeat",
                _ => "Game ended",
            };
            (EventType::GameEnd, Involvement::None, detail.to_string())
        }
        _ => return None,
    };
    Some(GameEvent { id: raw.event_id, ty, game_time: raw.event_time, involvement, detail })
}

/// The API returns the whole event list on every poll; this yields each event once.
#[derive(Debug, Default)]
pub struct EventTracker {
    next_id: u32,
}

impl EventTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take_new<'a>(&mut self, events: &'a [RawEvent]) -> Vec<&'a RawEvent> {
        let fresh: Vec<&RawEvent> = events.iter().filter(|e| e.event_id >= self.next_id).collect();
        if let Some(max) = fresh.iter().map(|e| e.event_id).max() {
            self.next_id = max + 1;
        }
        fresh
    }
}
