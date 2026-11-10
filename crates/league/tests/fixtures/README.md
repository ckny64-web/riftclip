# Fixtures

`allgamedata_sample.json` and `ddragon_item_sample.json` are **hand-written** to the
documented shape of the APIs; they were not captured from a real game.

To replace them with real captures, run this on Windows during a match:

    cargo run -p league --example dump_live -- crates/league/tests/fixtures/allgamedata_real.json
