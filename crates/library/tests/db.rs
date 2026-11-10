use league::{EventType, Involvement};
use library::*;

fn event(id: Option<i64>, ty: EventType, inv: Involvement, t: f64, detail: &str) -> NewEvent {
    NewEvent { game_event_id: id, ty, involvement: inv, rec_time: t, game_time: t + 15.0, detail: detail.into() }
}

fn new_match(started_at: i64) -> NewMatch {
    NewMatch { started_at, champion: "Ahri".into(), game_mode: "CLASSIC".into(), recording_path: Some(format!("rec{started_at}.mp4")) }
}

#[test]
fn match_lifecycle_and_summary() {
    let mut lib = Library::open_in_memory().unwrap();
    let old = lib.insert_match(&new_match(100)).unwrap();
    let id = lib.insert_match(&new_match(200)).unwrap();

    lib.insert_event(id, &event(Some(2), EventType::ChampionKill, Involvement::Killer, 50.0, "Ahri killed Zed")).unwrap();
    lib.insert_event(id, &event(Some(3), EventType::ChampionKill, Involvement::Killer, 60.0, "k2")).unwrap();
    lib.insert_event(id, &event(Some(4), EventType::ChampionKill, Involvement::Victim, 70.0, "d")).unwrap();
    lib.insert_event(id, &event(Some(5), EventType::ChampionKill, Involvement::Assist, 80.0, "a")).unwrap();
    lib.insert_event(id, &event(Some(6), EventType::ChampionKill, Involvement::Enemy, 90.0, "other")).unwrap();
    lib.insert_event(id, &event(Some(7), EventType::Dragon, Involvement::Killer, 95.0, "dragon")).unwrap();

    lib.finish_match(id, 1700.0, "win").unwrap();
    lib.set_match_queue(id, "Ranked Solo/Duo").unwrap();
    lib.set_recording(id, Some("final.mp4"), 4_000_000_000).unwrap();
    lib.set_match_thumbnail(id, "thumb.jpg").unwrap();
    lib.set_players(id, &[
        PlayerRow { team: "ORDER".into(), is_me: true, name: "Mid Me".into(), champion: "Ahri".into(), rank_tier: "GOLD".into(), rank_division: "II".into(), rank_lp: 45, ..Default::default() },
        PlayerRow { team: "CHAOS".into(), name: "MidFoe".into(), champion: "Zed".into(), ..Default::default() },
    ]).unwrap();

    let list = lib.list_matches().unwrap();
    assert_eq!(list.iter().map(|m| m.row.id).collect::<Vec<_>>(), vec![id, old]); // newest first
    let m = &list[0];
    assert_eq!((m.kills, m.deaths, m.assists, m.clip_count), (2, 1, 1, 0));
    assert_eq!(m.row.result, "win");
    assert_eq!(m.row.queue, "Ranked Solo/Duo");
    assert_eq!(m.row.recording_path.as_deref(), Some("final.mp4"));
    assert_eq!(m.row.recording_bytes, 4_000_000_000);
    assert_eq!(lib.get_match(id).unwrap(), m.row);

    let players = lib.players(id).unwrap();
    assert_eq!(players.len(), 2);
    assert!(players[0].is_me);
    assert_eq!(players[0].rank_tier, "GOLD");
    // set_players replaces, it does not append
    lib.set_players(id, &players[..1]).unwrap();
    assert_eq!(lib.players(id).unwrap().len(), 1);

    assert!(matches!(lib.get_match(9999), Err(Error::NotFound)));
    assert!(matches!(lib.finish_match(9999, 1.0, ""), Err(Error::NotFound)));
}

#[test]
fn events_are_deduplicated_and_ordered() {
    let lib = Library::open_in_memory().unwrap();
    let id = lib.insert_match(&new_match(1)).unwrap();
    let a = lib.insert_event(id, &event(Some(7), EventType::Baron, Involvement::Team, 300.0, "baron")).unwrap();
    let again = lib.insert_event(id, &event(Some(7), EventType::Baron, Involvement::Team, 300.0, "baron")).unwrap();
    assert_eq!(a, again);
    // manual marks have no game id and never collide
    lib.insert_event(id, &event(None, EventType::Manual, Involvement::None, 10.0, "mark")).unwrap();
    lib.insert_event(id, &event(None, EventType::Manual, Involvement::None, 20.0, "mark")).unwrap();

    let ev = lib.events(id).unwrap();
    assert_eq!(ev.iter().map(|e| e.rec_time).collect::<Vec<_>>(), vec![10.0, 20.0, 300.0]);
    assert_eq!(ev[2].ty, EventType::Baron);
    assert_eq!(ev[2].involvement, Involvement::Team);
    assert_eq!(ev[2].game_time, 315.0);

    // the same game event id in another match is a different event
    let other = lib.insert_match(&new_match(2)).unwrap();
    lib.insert_event(other, &event(Some(7), EventType::Baron, Involvement::Enemy, 1.0, "x")).unwrap();
    assert_eq!(lib.events(other).unwrap().len(), 1);
}

#[test]
fn clips_survive_match_deletion() {
    let lib = Library::open_in_memory().unwrap();
    let id = lib.insert_match(&new_match(1)).unwrap();
    lib.insert_event(id, &event(Some(1), EventType::ChampionKill, Involvement::Killer, 5.0, "k")).unwrap();
    let clip = |start: f64, created: i64, auto: bool| NewClip {
        match_id: Some(id), start, end: start + 15.0, path: format!("clip{start}.mp4"), thumbnail_path: None,
        title: "Ahri killed Zed".into(), auto, created_at: created,
    };
    let c1 = lib.insert_clip(&clip(100.0, 10, true)).unwrap();
    let c2 = lib.insert_clip(&clip(40.0, 20, false)).unwrap();

    assert_eq!(lib.clips(Some(id)).unwrap().iter().map(|c| c.id).collect::<Vec<_>>(), vec![c2, c1]); // timeline order
    assert_eq!(lib.clips(None).unwrap().iter().map(|c| c.id).collect::<Vec<_>>(), vec![c2, c1]); // newest first
    assert_eq!(lib.list_matches().unwrap()[0].clip_count, 2);

    lib.rename_clip(c1, "Outplay").unwrap();
    assert_eq!(lib.get_clip(c1).unwrap().title, "Outplay");
    assert!(lib.get_clip(c1).unwrap().auto);

    lib.delete_match(id).unwrap();
    assert!(lib.events(id).unwrap().is_empty()); // cascade
    let left = lib.clips(None).unwrap();
    assert_eq!(left.len(), 2);
    assert_eq!(left[0].match_id, None);

    let removed = lib.delete_clip(c2).unwrap();
    assert_eq!(removed.path, "clip40.mp4");
    assert!(matches!(lib.delete_clip(c2), Err(Error::NotFound)));
}

#[test]
fn recordings_list_and_retention_plan() {
    let lib = Library::open_in_memory().unwrap();
    let day = 86_400;
    let a = lib.insert_match(&new_match(10 * day)).unwrap();
    let b = lib.insert_match(&new_match(28 * day)).unwrap();
    let c = lib.insert_match(&new_match(29 * day)).unwrap();
    for id in [a, b, c] {
        lib.set_recording(id, Some("x.mp4"), 5 << 30).unwrap();
    }
    let plan = library::retention::plan(&lib.recordings().unwrap(), 30 * day, 14, 5 << 30);
    assert_eq!(plan, vec![a, b]); // a by age, b by size cap
    for id in plan {
        lib.set_recording(id, None, 0).unwrap();
    }
    assert_eq!(lib.recordings().unwrap().len(), 1);
    assert_eq!(lib.get_match(a).unwrap().recording_path, None);
}

#[test]
fn settings_roundtrip_and_persist_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lib.db");
    {
        let lib = Library::open(&path).unwrap();
        assert_eq!(lib.settings().unwrap(), AppSettings::default());
        let mut s = AppSettings { bitrate_kbps: 30_000, ..Default::default() };
        s.auto_clips.deaths = false;
        s.hotkeys.mark = "Ctrl+F8".into();
        lib.save_settings(&s).unwrap();
        lib.save_settings(&s).unwrap(); // upsert
        lib.insert_match(&new_match(5)).unwrap();
    }
    let lib = Library::open(&path).unwrap(); // reopening keeps data and re-running the schema is harmless
    let s = lib.settings().unwrap();
    assert_eq!(s.bitrate_kbps, 30_000);
    assert!(!s.auto_clips.deaths);
    assert_eq!(s.hotkeys.mark, "Ctrl+F8");
    assert_eq!(s.fps, 60);
    assert_eq!(lib.list_matches().unwrap().len(), 1);
}
