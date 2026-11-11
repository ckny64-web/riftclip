//! Text and colour helpers for the panels.

use eframe::egui::Color32;

pub const GOOD: Color32 = Color32::from_rgb(92, 214, 140);
pub const BAD: Color32 = Color32::from_rgb(240, 110, 110);
pub const NEUTRAL: Color32 = Color32::from_rgb(200, 204, 212);
pub const DIM: Color32 = Color32::from_rgb(140, 146, 158);

/// "+1.2k", "-350", "0".
pub fn gold_diff(diff: i64) -> String {
    let sign = match diff {
        d if d > 0 => "+",
        d if d < 0 => "-",
        _ => "",
    };
    let abs = diff.unsigned_abs();
    if abs >= 1000 {
        format!("{sign}{}.{}k", abs / 1000, abs % 1000 / 100)
    } else {
        format!("{sign}{abs}")
    }
}

pub fn diff_color(diff: i64) -> Color32 {
    match diff {
        d if d > 0 => GOOD,
        d if d < 0 => BAD,
        _ => NEUTRAL,
    }
}

pub fn lane_label(position: &str) -> &'static str {
    match position {
        "TOP" => "Top",
        "JUNGLE" => "Jungle",
        "MIDDLE" => "Mid",
        "BOTTOM" => "Bot",
        "UTILITY" => "Support",
        _ => "",
    }
}

pub fn tier_color(tier: &str) -> Color32 {
    match tier {
        "IRON" => Color32::from_rgb(130, 120, 118),
        "BRONZE" => Color32::from_rgb(176, 124, 92),
        "SILVER" => Color32::from_rgb(170, 182, 192),
        "GOLD" => Color32::from_rgb(226, 184, 92),
        "PLATINUM" => Color32::from_rgb(92, 196, 190),
        "EMERALD" => Color32::from_rgb(70, 200, 120),
        "DIAMOND" => Color32::from_rgb(130, 160, 250),
        "MASTER" => Color32::from_rgb(190, 120, 230),
        "GRANDMASTER" => Color32::from_rgb(236, 96, 96),
        "CHALLENGER" => Color32::from_rgb(120, 220, 250),
        _ => DIM,
    }
}

/// "60% (50)" or "" when the player has no ranked games.
pub fn win_rate(rate: Option<f32>, games: u32) -> String {
    match rate {
        Some(r) if games > 0 => format!("{r:.0}% ({games})"),
        _ => String::new(),
    }
}

pub fn win_rate_color(rate: Option<f32>) -> Color32 {
    match rate {
        Some(r) if r >= 55.0 => GOOD,
        Some(r) if r < 45.0 => BAD,
        _ => NEUTRAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gold_text() {
        assert_eq!(gold_diff(0), "0");
        assert_eq!(gold_diff(333), "+333");
        assert_eq!(gold_diff(-350), "-350");
        assert_eq!(gold_diff(1250), "+1.2k");
        assert_eq!(gold_diff(-12_980), "-12.9k");
        assert_eq!(gold_diff(1000), "+1.0k");
    }

    #[test]
    fn colours_and_labels() {
        assert_eq!(diff_color(5), GOOD);
        assert_eq!(diff_color(-5), BAD);
        assert_eq!(diff_color(0), NEUTRAL);
        assert_eq!(lane_label("UTILITY"), "Support");
        assert_eq!(lane_label("NONE"), "");
        assert_eq!(tier_color(""), DIM);
        assert_ne!(tier_color("GOLD"), DIM);
        assert_eq!(win_rate(Some(59.6), 50), "60% (50)");
        assert_eq!(win_rate(None, 0), "");
        assert_eq!(win_rate_color(Some(60.0)), GOOD);
        assert_eq!(win_rate_color(Some(40.0)), BAD);
        assert_eq!(win_rate_color(Some(50.0)), NEUTRAL);
    }
}
