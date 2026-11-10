//! Decides which stretches of a recording become automatic clips.

use crate::db::EventRow;
use crate::settings::AutoClipRules;
use league::{EventType, Involvement};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipPlan {
    /// Seconds into the recording.
    pub start: f64,
    pub end: f64,
    pub title: String,
    /// Database ids of the events inside this clip.
    pub event_ids: Vec<i64>,
}

fn qualifies(e: &EventRow, rules: &AutoClipRules) -> bool {
    use Involvement::*;
    match e.ty {
        EventType::ChampionKill => match e.involvement {
            Killer => rules.kills,
            Victim => rules.deaths,
            Assist => rules.assists,
            _ => false,
        },
        EventType::Multikill | EventType::FirstBlood => rules.kills && e.involvement == Killer,
        EventType::Ace => rules.kills && matches!(e.involvement, Killer | Team),
        t if t.is_objective() => rules.objectives && matches!(e.involvement, Killer | Assist | Team),
        _ => false,
    }
}

/// Higher wins when choosing the title of a merged clip.
fn weight(e: &EventRow) -> u32 {
    match (e.ty, e.involvement) {
        (EventType::Multikill, _) => 90,
        (EventType::Ace, _) => 80,
        (EventType::Baron, _) | (EventType::Atakhan, _) => 70,
        (EventType::ChampionKill, Involvement::Killer) => 60,
        (EventType::Dragon, _) | (EventType::Herald, _) | (EventType::Inhibitor, _) => 50,
        (EventType::FirstBlood, _) => 45,
        (EventType::ChampionKill, Involvement::Victim) => 40,
        (EventType::ChampionKill, Involvement::Assist) => 30,
        _ => 10,
    }
}

/// Plan clips for a finished match. Each qualifying event gets
/// `pad_before`/`pad_after` seconds around it; windows that overlap or touch
/// are merged, so a teamfight becomes one clip. `duration` is the length of
/// the recording in seconds.
pub fn plan_clips(events: &[EventRow], rules: &AutoClipRules, duration: f64) -> Vec<ClipPlan> {
    if !rules.enabled || duration <= 0.0 {
        return Vec::new();
    }
    let mut picked: Vec<&EventRow> = events.iter().filter(|e| qualifies(e, rules)).collect();
    picked.sort_by(|a, b| a.rec_time.total_cmp(&b.rec_time));

    struct Group<'a> {
        start: f64,
        end: f64,
        events: Vec<&'a EventRow>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for e in picked {
        let start = (e.rec_time - rules.pad_before.max(0.0)).max(0.0);
        let end = (e.rec_time + rules.pad_after.max(0.0)).min(duration);
        if end <= start {
            continue; // event lies outside the recording
        }
        match groups.last_mut() {
            Some(g) if start <= g.end => {
                g.end = g.end.max(end);
                g.events.push(e);
            }
            _ => groups.push(Group { start, end, events: vec![e] }),
        }
    }

    groups
        .into_iter()
        .map(|g| {
            // First event with the highest weight names the clip.
            let lead = g.events.iter().rev().max_by_key(|e| weight(e)).expect("group is never empty");
            // A multikill already implies its kills; do not count them as extras.
            let extra = g
                .events
                .iter()
                .filter(|e| e.id != lead.id && !matches!(e.ty, EventType::Multikill | EventType::FirstBlood))
                .filter(|e| !(lead.ty == EventType::Multikill && e.ty == EventType::ChampionKill && e.involvement == Involvement::Killer))
                .count();
            let title = if extra > 0 { format!("{} +{extra}", lead.detail) } else { lead.detail.clone() };
            ClipPlan { start: g.start, end: g.end, title, event_ids: g.events.iter().map(|e| e.id).collect() }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: i64, ty: EventType, inv: Involvement, t: f64, detail: &str) -> EventRow {
        EventRow { id, match_id: 1, game_event_id: Some(id), ty, involvement: inv, rec_time: t, game_time: t, detail: detail.into() }
    }

    fn rules() -> AutoClipRules {
        AutoClipRules { pad_before: 10.0, pad_after: 5.0, ..Default::default() }
    }

    #[test]
    fn single_kill_gets_padding() {
        let e = [ev(1, EventType::ChampionKill, Involvement::Killer, 100.0, "Ahri killed Zed")];
        let plan = plan_clips(&e, &rules(), 600.0);
        assert_eq!(plan, vec![ClipPlan { start: 90.0, end: 105.0, title: "Ahri killed Zed".into(), event_ids: vec![1] }]);
    }

    #[test]
    fn overlapping_events_merge_and_multikill_names_the_clip() {
        let e = [
            ev(1, EventType::ChampionKill, Involvement::Killer, 100.0, "Ahri killed Zed"),
            ev(2, EventType::ChampionKill, Involvement::Killer, 104.0, "Ahri killed Lee Sin"),
            ev(3, EventType::Multikill, Involvement::Killer, 104.0, "Double kill by Ahri"),
            ev(4, EventType::ChampionKill, Involvement::Assist, 112.0, "Jinx killed Thresh"),
            ev(5, EventType::ChampionKill, Involvement::Killer, 300.0, "Ahri killed Darius"),
        ];
        let plan = plan_clips(&e, &rules(), 600.0);
        assert_eq!(plan.len(), 2);
        assert_eq!((plan[0].start, plan[0].end), (90.0, 117.0));
        assert_eq!(plan[0].title, "Double kill by Ahri +1"); // the assist is the extra
        assert_eq!(plan[0].event_ids, vec![1, 2, 3, 4]);
        assert_eq!(plan[1].title, "Ahri killed Darius");
    }

    #[test]
    fn rules_filter_event_kinds() {
        let e = [
            ev(1, EventType::ChampionKill, Involvement::Killer, 50.0, "k"),
            ev(2, EventType::ChampionKill, Involvement::Victim, 150.0, "d"),
            ev(3, EventType::ChampionKill, Involvement::Assist, 250.0, "a"),
            ev(4, EventType::Dragon, Involvement::Team, 350.0, "o"),
            ev(5, EventType::Dragon, Involvement::Enemy, 450.0, "enemy objective"),
            ev(6, EventType::ChampionKill, Involvement::Enemy, 500.0, "someone else"),
            ev(7, EventType::Manual, Involvement::None, 550.0, "mark"),
        ];
        let titles = |r: &AutoClipRules| plan_clips(&e, r, 600.0).into_iter().map(|c| c.title).collect::<Vec<_>>();
        assert_eq!(titles(&rules()), vec!["k", "d", "a", "o"]);
        assert_eq!(titles(&AutoClipRules { deaths: false, assists: false, ..rules() }), vec!["k", "o"]);
        assert_eq!(titles(&AutoClipRules { kills: false, objectives: false, ..rules() }), vec!["d", "a"]);
        assert!(titles(&AutoClipRules { enabled: false, ..rules() }).is_empty());
    }

    #[test]
    fn clamps_to_recording_bounds() {
        let e = [
            ev(1, EventType::ChampionKill, Involvement::Killer, 3.0, "early"),
            ev(2, EventType::ChampionKill, Involvement::Killer, 598.0, "late"),
            ev(3, EventType::ChampionKill, Involvement::Killer, 700.0, "after the recording ended"),
        ];
        let plan = plan_clips(&e, &rules(), 600.0);
        assert_eq!(plan.len(), 2);
        assert_eq!((plan[0].start, plan[0].end), (0.0, 8.0));
        assert_eq!((plan[1].start, plan[1].end), (588.0, 600.0));
    }

    #[test]
    fn unsorted_input_is_handled() {
        let e = [
            ev(2, EventType::ChampionKill, Involvement::Killer, 200.0, "second"),
            ev(1, EventType::ChampionKill, Involvement::Killer, 100.0, "first"),
        ];
        let plan = plan_clips(&e, &rules(), 600.0);
        assert_eq!(plan.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["first", "second"]);
    }
}
