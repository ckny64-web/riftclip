//! SQLite storage.

use crate::retention::Recording;
use crate::settings::AppSettings;
use crate::{Error, Result};
use league::{EventType, Involvement};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS matches (
    id              INTEGER PRIMARY KEY,
    started_at      INTEGER NOT NULL,          -- unix seconds
    duration        REAL    NOT NULL DEFAULT 0, -- recording length, seconds
    champion        TEXT    NOT NULL DEFAULT '',
    game_mode       TEXT    NOT NULL DEFAULT '',
    queue           TEXT    NOT NULL DEFAULT '',
    result          TEXT    NOT NULL DEFAULT '', -- 'win', 'loss' or ''
    recording_path  TEXT,                       -- NULL once deleted by retention
    recording_bytes INTEGER NOT NULL DEFAULT 0,
    thumbnail_path  TEXT
);
CREATE TABLE IF NOT EXISTS players (
    id        INTEGER PRIMARY KEY,
    match_id  INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    team      TEXT    NOT NULL DEFAULT '',
    is_me     INTEGER NOT NULL DEFAULT 0,
    name      TEXT    NOT NULL DEFAULT '',
    champion  TEXT    NOT NULL DEFAULT '',
    position  TEXT    NOT NULL DEFAULT '',
    rank_tier TEXT    NOT NULL DEFAULT '',
    rank_division TEXT NOT NULL DEFAULT '',
    rank_lp   INTEGER NOT NULL DEFAULT 0,
    wins      INTEGER NOT NULL DEFAULT 0,
    losses    INTEGER NOT NULL DEFAULT 0,
    kills     INTEGER NOT NULL DEFAULT 0,
    deaths    INTEGER NOT NULL DEFAULT 0,
    assists   INTEGER NOT NULL DEFAULT 0,
    cs        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS players_match ON players(match_id);
CREATE TABLE IF NOT EXISTS events (
    id            INTEGER PRIMARY KEY,
    match_id      INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    game_event_id INTEGER,               -- NULL for manual marks
    type          TEXT    NOT NULL,
    involvement   TEXT    NOT NULL,
    rec_time      REAL    NOT NULL,      -- seconds into the recording
    game_time     REAL    NOT NULL,
    detail        TEXT    NOT NULL DEFAULT '',
    UNIQUE (match_id, game_event_id)
);
CREATE INDEX IF NOT EXISTS events_match ON events(match_id, rec_time);
CREATE TABLE IF NOT EXISTS clips (
    id         INTEGER PRIMARY KEY,
    match_id   INTEGER REFERENCES matches(id) ON DELETE SET NULL,
    start_time REAL    NOT NULL,
    end_time   REAL    NOT NULL,
    path       TEXT    NOT NULL,
    thumbnail_path TEXT,
    title      TEXT    NOT NULL DEFAULT '',
    auto       INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

#[derive(Debug, Clone, Default)]
pub struct NewMatch {
    pub started_at: i64,
    pub champion: String,
    pub game_mode: String,
    pub recording_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatchRow {
    pub id: i64,
    pub started_at: i64,
    pub duration: f64,
    pub champion: String,
    pub game_mode: String,
    pub queue: String,
    pub result: String,
    pub recording_path: Option<String>,
    pub recording_bytes: i64,
    pub thumbnail_path: Option<String>,
}

/// A match plus the numbers the match list shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatchSummary {
    #[serde(flatten)]
    pub row: MatchRow,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub clip_count: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlayerRow {
    pub team: String,
    pub is_me: bool,
    pub name: String,
    pub champion: String,
    pub position: String,
    pub rank_tier: String,
    pub rank_division: String,
    pub rank_lp: i64,
    pub wins: i64,
    pub losses: i64,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub cs: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub game_event_id: Option<i64>,
    pub ty: EventType,
    pub involvement: Involvement,
    pub rec_time: f64,
    pub game_time: f64,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventRow {
    pub id: i64,
    pub match_id: i64,
    pub game_event_id: Option<i64>,
    #[serde(rename = "type")]
    pub ty: EventType,
    pub involvement: Involvement,
    pub rec_time: f64,
    pub game_time: f64,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewClip {
    pub match_id: Option<i64>,
    pub start: f64,
    pub end: f64,
    pub path: String,
    pub thumbnail_path: Option<String>,
    pub title: String,
    pub auto: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipRow {
    pub id: i64,
    pub match_id: Option<i64>,
    pub start: f64,
    pub end: f64,
    pub path: String,
    pub thumbnail_path: Option<String>,
    pub title: String,
    pub auto: bool,
    pub created_at: i64,
}

pub struct Library {
    conn: Connection,
}

const MATCH_COLS: &str =
    "m.id, m.started_at, m.duration, m.champion, m.game_mode, m.queue, m.result, m.recording_path, m.recording_bytes, m.thumbnail_path";

fn match_row(r: &Row) -> rusqlite::Result<MatchRow> {
    Ok(MatchRow {
        id: r.get(0)?,
        started_at: r.get(1)?,
        duration: r.get(2)?,
        champion: r.get(3)?,
        game_mode: r.get(4)?,
        queue: r.get(5)?,
        result: r.get(6)?,
        recording_path: r.get(7)?,
        recording_bytes: r.get(8)?,
        thumbnail_path: r.get(9)?,
    })
}

fn clip_row(r: &Row) -> rusqlite::Result<ClipRow> {
    Ok(ClipRow {
        id: r.get(0)?,
        match_id: r.get(1)?,
        start: r.get(2)?,
        end: r.get(3)?,
        path: r.get(4)?,
        thumbnail_path: r.get(5)?,
        title: r.get(6)?,
        auto: r.get(7)?,
        created_at: r.get(8)?,
    })
}

const CLIP_COLS: &str = "id, match_id, start_time, end_time, path, thumbnail_path, title, auto, created_at";

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Library { conn })
    }

    // ---- matches

    pub fn insert_match(&self, m: &NewMatch) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO matches (started_at, champion, game_mode, recording_path) VALUES (?1, ?2, ?3, ?4)",
            params![m.started_at, m.champion, m.game_mode, m.recording_path],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Record how the match ended. `result` is "win", "loss" or "".
    pub fn finish_match(&self, id: i64, duration: f64, result: &str) -> Result<()> {
        self.changed(self.conn.execute(
            "UPDATE matches SET duration = ?2, result = ?3 WHERE id = ?1",
            params![id, duration, result],
        )?)
    }

    pub fn set_match_queue(&self, id: i64, queue: &str) -> Result<()> {
        self.changed(self.conn.execute("UPDATE matches SET queue = ?2 WHERE id = ?1", params![id, queue])?)
    }

    /// Point the match at its (remuxed) recording, or clear it with `None`.
    pub fn set_recording(&self, id: i64, path: Option<&str>, bytes: u64) -> Result<()> {
        self.changed(self.conn.execute(
            "UPDATE matches SET recording_path = ?2, recording_bytes = ?3 WHERE id = ?1",
            params![id, path, bytes as i64],
        )?)
    }

    pub fn set_match_thumbnail(&self, id: i64, path: &str) -> Result<()> {
        self.changed(self.conn.execute("UPDATE matches SET thumbnail_path = ?2 WHERE id = ?1", params![id, path])?)
    }

    pub fn get_match(&self, id: i64) -> Result<MatchRow> {
        self.conn
            .query_row(&format!("SELECT {MATCH_COLS} FROM matches m WHERE m.id = ?1"), [id], match_row)
            .optional()?
            .ok_or(Error::NotFound)
    }

    /// Newest first, with my K/D/A (from events) and the number of clips.
    pub fn list_matches(&self) -> Result<Vec<MatchSummary>> {
        let sql = format!(
            "SELECT {MATCH_COLS},
                (SELECT COUNT(*) FROM events e WHERE e.match_id = m.id AND e.type = 'champion_kill' AND e.involvement = 'killer'),
                (SELECT COUNT(*) FROM events e WHERE e.match_id = m.id AND e.type = 'champion_kill' AND e.involvement = 'victim'),
                (SELECT COUNT(*) FROM events e WHERE e.match_id = m.id AND e.type = 'champion_kill' AND e.involvement = 'assist'),
                (SELECT COUNT(*) FROM clips c WHERE c.match_id = m.id)
             FROM matches m ORDER BY m.started_at DESC, m.id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(MatchSummary {
                row: match_row(r)?,
                kills: r.get(10)?,
                deaths: r.get(11)?,
                assists: r.get(12)?,
                clip_count: r.get(13)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Delete a match with its players and events. Its clips are kept.
    pub fn delete_match(&self, id: i64) -> Result<()> {
        self.changed(self.conn.execute("DELETE FROM matches WHERE id = ?1", [id])?)
    }

    /// Matches that still have a recording on disk, for retention planning.
    pub fn recordings(&self) -> Result<Vec<Recording>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, started_at, recording_bytes FROM matches WHERE recording_path IS NOT NULL")?;
        let rows = stmt.query_map([], |r| {
            Ok(Recording { match_id: r.get(0)?, started_at: r.get(1)?, bytes: r.get::<_, i64>(2)?.max(0) as u64 })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- players

    /// Replace the player list of a match.
    pub fn set_players(&mut self, match_id: i64, players: &[PlayerRow]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM players WHERE match_id = ?1", [match_id])?;
        for p in players {
            tx.execute(
                "INSERT INTO players (match_id, team, is_me, name, champion, position, rank_tier, rank_division,
                                      rank_lp, wins, losses, kills, deaths, assists, cs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    match_id, p.team, p.is_me, p.name, p.champion, p.position, p.rank_tier, p.rank_division,
                    p.rank_lp, p.wins, p.losses, p.kills, p.deaths, p.assists, p.cs
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn players(&self, match_id: i64) -> Result<Vec<PlayerRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT team, is_me, name, champion, position, rank_tier, rank_division, rank_lp, wins, losses,
                    kills, deaths, assists, cs
             FROM players WHERE match_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([match_id], |r| {
            Ok(PlayerRow {
                team: r.get(0)?,
                is_me: r.get(1)?,
                name: r.get(2)?,
                champion: r.get(3)?,
                position: r.get(4)?,
                rank_tier: r.get(5)?,
                rank_division: r.get(6)?,
                rank_lp: r.get(7)?,
                wins: r.get(8)?,
                losses: r.get(9)?,
                kills: r.get(10)?,
                deaths: r.get(11)?,
                assists: r.get(12)?,
                cs: r.get(13)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- events

    /// Insert an event. A game event already stored for this match (same
    /// `game_event_id`) is ignored; returns the row id either way.
    pub fn insert_event(&self, match_id: i64, e: &NewEvent) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO events (match_id, game_event_id, type, involvement, rec_time, game_time, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![match_id, e.game_event_id, e.ty.as_str(), e.involvement.as_str(), e.rec_time, e.game_time, e.detail],
        )?;
        if self.conn.changes() > 0 {
            return Ok(self.conn.last_insert_rowid());
        }
        Ok(self.conn.query_row(
            "SELECT id FROM events WHERE match_id = ?1 AND game_event_id = ?2",
            params![match_id, e.game_event_id],
            |r| r.get(0),
        )?)
    }

    pub fn events(&self, match_id: i64) -> Result<Vec<EventRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, match_id, game_event_id, type, involvement, rec_time, game_time, detail
             FROM events WHERE match_id = ?1 ORDER BY rec_time, id",
        )?;
        let rows = stmt.query_map([match_id], |r| {
            let ty: String = r.get(3)?;
            let inv: String = r.get(4)?;
            Ok((
                EventRow {
                    id: r.get(0)?,
                    match_id: r.get(1)?,
                    game_event_id: r.get(2)?,
                    ty: EventType::Manual,
                    involvement: Involvement::None,
                    rec_time: r.get(5)?,
                    game_time: r.get(6)?,
                    detail: r.get(7)?,
                },
                ty,
                inv,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mut e, ty, inv) = row?;
            // Rows written by a newer version with unknown kinds are skipped.
            if let (Some(ty), Some(inv)) = (EventType::parse(&ty), Involvement::parse(&inv)) {
                e.ty = ty;
                e.involvement = inv;
                out.push(e);
            }
        }
        Ok(out)
    }

    // ---- clips

    pub fn insert_clip(&self, c: &NewClip) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO clips (match_id, start_time, end_time, path, thumbnail_path, title, auto, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![c.match_id, c.start, c.end, c.path, c.thumbnail_path, c.title, c.auto, c.created_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_clip(&self, id: i64) -> Result<ClipRow> {
        self.conn
            .query_row(&format!("SELECT {CLIP_COLS} FROM clips WHERE id = ?1"), [id], clip_row)
            .optional()?
            .ok_or(Error::NotFound)
    }

    /// All clips, newest first; or only those of one match, in timeline order.
    pub fn clips(&self, match_id: Option<i64>) -> Result<Vec<ClipRow>> {
        let (sql, arg) = match match_id {
            Some(id) => (format!("SELECT {CLIP_COLS} FROM clips WHERE match_id = ?1 ORDER BY start_time, id"), id),
            None => (format!("SELECT {CLIP_COLS} FROM clips WHERE ?1 = 0 ORDER BY created_at DESC, id DESC"), 0),
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([arg], clip_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn rename_clip(&self, id: i64, title: &str) -> Result<()> {
        self.changed(self.conn.execute("UPDATE clips SET title = ?2 WHERE id = ?1", params![id, title])?)
    }

    /// Remove the clip's row and return it so the caller can delete its files.
    pub fn delete_clip(&self, id: i64) -> Result<ClipRow> {
        let clip = self.get_clip(id)?;
        self.conn.execute("DELETE FROM clips WHERE id = ?1", [id])?;
        Ok(clip)
    }

    // ---- settings

    pub fn settings(&self) -> Result<AppSettings> {
        let json: Option<String> =
            self.conn.query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| r.get(0)).optional()?;
        match json {
            // Unknown or missing fields fall back to defaults instead of failing.
            Some(j) => Ok(serde_json::from_str(&j).unwrap_or_default()),
            None => Ok(AppSettings::default()),
        }
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES ('app', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    fn changed(&self, rows: usize) -> Result<()> {
        if rows == 0 {
            Err(Error::NotFound)
        } else {
            Ok(())
        }
    }
}
