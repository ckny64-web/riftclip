//! Prints what the League client answers on the endpoints Riftclip uses, so
//! wrong assumptions can be fixed from one paste. Run with the client open,
//! ideally while in a game (loading screen or later):
//!
//!   cargo run -p league --example lcu_probe
//!
//! The connection token is never printed.

use league::client::LcuClient;
use league::process::ProcessWatcher;

fn show(client: &LcuClient, path: &str) -> Option<serde_json::Value> {
    println!("\n== GET {path}");
    match client.get(path) {
        Ok(v) => {
            let text = v.to_string();
            let cut: String = text.chars().take(1800).collect();
            println!("{cut}{}", if text.len() > cut.len() { " …(truncated)" } else { "" });
            Some(v)
        }
        Err(e) => {
            println!("FAILED: {e}");
            None
        }
    }
}

fn main() {
    let mut watcher = ProcessWatcher::new();
    println!("Game process running: {}", watcher.game_pid().is_some());
    let Some(creds) = watcher.client_credentials() else {
        println!("League client not found (is it open? If it runs as administrator, run this the same way).");
        std::process::exit(1);
    };
    println!("League client found on port {}", creds.port);
    let client = LcuClient::new(&creds);

    show(&client, "/lol-gameflow/v1/gameflow-phase");
    let me = show(&client, "/lol-summoner/v1/current-summoner");
    let session = show(&client, "/lol-gameflow/v1/session");

    let my_puuid = me.as_ref().and_then(|v| v.get("puuid")).and_then(|p| p.as_str()).unwrap_or("").to_string();
    if !my_puuid.is_empty() {
        show(&client, &format!("/lol-ranked/v1/ranked-stats/{my_puuid}"));
    }

    // Someone else in the game, to see whether their name and rank are readable.
    let other = session
        .as_ref()
        .map(league::lcu::parse_gameflow_session)
        .and_then(|s| s.players.into_iter().find(|p| p.puuid != my_puuid));
    match other {
        Some(p) => {
            show(&client, &format!("/lol-summoner/v2/summoners/puuid/{}", p.puuid));
            show(&client, &format!("/lol-ranked/v1/ranked-stats/{}", p.puuid));
        }
        None => println!("\nNo other players in the current session (not in a game with other people)."),
    }

    println!("\n== Parsed result");
    match client.game_players() {
        Ok((s, players)) => {
            println!("phase={} queue={} ({}) players={}", s.phase, s.queue_id, s.queue_description, players.len());
            for p in players {
                println!("  team {} champion {:>4}  {:<20} {}", p.team, p.champion_id, p.game_name, p.rank.label());
            }
        }
        Err(e) => println!("FAILED: {e}"),
    }
}
