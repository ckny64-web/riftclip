//! League of Legends data layer: models and logic for the in-game Live Client
//! Data API and the League client (LCU) API. Everything outside the `client`
//! feature is pure and platform-neutral so it can be unit tested anywhere.

pub mod events;
pub mod gold;
pub mod lcu;
pub mod live;
pub mod timemap;

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "client")]
pub mod process;

pub use events::{classify, EventTracker, EventType, GameEvent, Involvement, Roster};
pub use gold::{gold_report, GoldReport, LaneGold, PriceTable};
pub use live::{AllGameData, Player, RawEvent};
pub use timemap::TimeMap;

pub const GAME_PROCESS: &str = "League of Legends.exe";
pub const CLIENT_UX_PROCESS: &str = "LeagueClientUx.exe";
