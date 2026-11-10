//! Saves the current `allgamedata` response to a file, for use as a test fixture.
//! Run during a match: `cargo run -p league --example dump_live -- out.json`

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "allgamedata.json".to_string());
    match league::client::LiveClient::new().all_game_data_raw() {
        Ok(json) => {
            std::fs::write(&path, json).expect("write file");
            println!("saved {path}");
        }
        Err(e) => {
            eprintln!("could not reach the live game API (is a match running?): {e}");
            std::process::exit(1);
        }
    }
}
