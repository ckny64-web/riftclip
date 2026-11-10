//! Blocking HTTP clients for the two local Riot APIs.
//!
//! Both APIs serve HTTPS on 127.0.0.1 with a certificate signed by Riot's own
//! root, which is not in any system trust store. These clients skip certificate
//! verification and, to keep that safe, refuse to talk to anything but loopback.

use crate::lcu::{self, Credentials, GameflowSession, Rank};
use crate::live::{AllGameData, EventList};
use base64::Engine;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unexpected status {0} for {1}")]
    Status(u16, String),
}

type Result<T> = std::result::Result<T, ClientError>;

fn loopback_client(timeout: Duration) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .timeout(timeout)
        .build()
        .expect("building a local HTTP client cannot fail")
}

/// In-game API; only reachable while a match is running.
pub struct LiveClient {
    http: reqwest::blocking::Client,
}

impl Default for LiveClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveClient {
    const BASE: &'static str = "https://127.0.0.1:2999/liveclientdata";

    pub fn new() -> Self {
        LiveClient { http: loopback_client(Duration::from_millis(1500)) }
    }

    fn get_text(&self, path: &str) -> Result<String> {
        let url = format!("{}/{path}", Self::BASE);
        let resp = self.http.get(&url).send()?;
        if !resp.status().is_success() {
            return Err(ClientError::Status(resp.status().as_u16(), url));
        }
        Ok(resp.text()?)
    }

    /// Raw `allgamedata` JSON, e.g. for saving a test fixture.
    pub fn all_game_data_raw(&self) -> Result<String> {
        self.get_text("allgamedata")
    }

    pub fn all_game_data(&self) -> Result<AllGameData> {
        let url = format!("{}/allgamedata", Self::BASE);
        let resp = self.http.get(&url).send()?;
        if !resp.status().is_success() {
            // The API answers 404 during the loading screen.
            return Err(ClientError::Status(resp.status().as_u16(), url));
        }
        Ok(resp.json()?)
    }

    pub fn events(&self) -> Result<EventList> {
        let url = format!("{}/eventdata", Self::BASE);
        Ok(self.http.get(&url).send()?.error_for_status()?.json()?)
    }
}

/// League client (lobby) API.
pub struct LcuClient {
    http: reqwest::blocking::Client,
    base: String,
    auth: String,
}

impl LcuClient {
    pub fn new(creds: &Credentials) -> Self {
        let raw = format!("riot:{}", creds.token);
        LcuClient {
            http: loopback_client(Duration::from_secs(4)),
            base: format!("https://127.0.0.1:{}", creds.port),
            auth: format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(raw)),
        }
    }

    /// GET any LCU path (starting with `/`) as JSON.
    pub fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{path}", self.base);
        let resp = self.http.get(&url).header("Authorization", &self.auth).send()?;
        if !resp.status().is_success() {
            return Err(ClientError::Status(resp.status().as_u16(), url));
        }
        Ok(resp.json()?)
    }

    /// e.g. "None", "Lobby", "ChampSelect", "InProgress", "EndOfGame".
    pub fn gameflow_phase(&self) -> Result<String> {
        Ok(self.get("/lol-gameflow/v1/gameflow-phase")?.as_str().unwrap_or("").to_string())
    }

    pub fn gameflow_session(&self) -> Result<GameflowSession> {
        Ok(lcu::parse_gameflow_session(&self.get("/lol-gameflow/v1/session")?))
    }

    /// Solo/duo rank for a player.
    pub fn solo_rank(&self, puuid: &str) -> Result<Rank> {
        let stats = self.get(&format!("/lol-ranked/v1/ranked-stats/{puuid}"))?;
        Ok(lcu::rank_from_stats(&stats, lcu::QUEUE_SOLO))
    }

    /// `(gameName, tagLine)` for a player, if the client will reveal it.
    pub fn riot_id(&self, puuid: &str) -> Result<Option<(String, String)>> {
        let v = self.get(&format!("/lol-summoner/v2/summoners/puuid/{puuid}"))?;
        Ok(lcu::riot_id_from_summoner(&v))
    }

    pub fn end_of_game_stats(&self) -> Result<Value> {
        self.get("/lol-end-of-game/v1/eog-stats-block")
    }
}
