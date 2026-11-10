//! Chooses which full-match recordings to delete. Clips are never touched.

#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub match_id: i64,
    /// Unix seconds.
    pub started_at: i64,
    pub bytes: u64,
}

/// Returns the match ids whose recordings should be deleted: everything older
/// than `max_age_days`, then the oldest remaining ones until the total is at
/// most `max_total_bytes`. A limit of 0 disables that rule.
pub fn plan(recordings: &[Recording], now: i64, max_age_days: u32, max_total_bytes: u64) -> Vec<i64> {
    let mut sorted: Vec<&Recording> = recordings.iter().collect();
    sorted.sort_by_key(|r| r.started_at); // oldest first

    let cutoff = now - max_age_days as i64 * 86_400;
    let mut delete = Vec::new();
    let mut kept: Vec<&Recording> = Vec::new();
    for r in sorted {
        if max_age_days > 0 && r.started_at < cutoff {
            delete.push(r.match_id);
        } else {
            kept.push(r);
        }
    }

    if max_total_bytes > 0 {
        let mut total: u64 = kept.iter().map(|r| r.bytes).sum();
        for r in kept {
            if total <= max_total_bytes {
                break;
            }
            delete.push(r.match_id);
            total -= r.bytes;
        }
    }
    delete
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const GB: u64 = 1 << 30;

    fn rec(id: i64, age_days: i64, gb: u64) -> Recording {
        Recording { match_id: id, started_at: 1_000 * DAY - age_days * DAY, bytes: gb * GB }
    }

    #[test]
    fn deletes_by_age() {
        let r = [rec(1, 20, 4), rec(2, 15, 4), rec(3, 2, 4)];
        assert_eq!(plan(&r, 1_000 * DAY, 14, 0), vec![1, 2]);
        assert!(plan(&r, 1_000 * DAY, 0, 0).is_empty());
    }

    #[test]
    fn deletes_oldest_until_under_cap() {
        let r = [rec(3, 1, 5), rec(1, 3, 5), rec(2, 2, 5)];
        assert_eq!(plan(&r, 1_000 * DAY, 0, 10 * GB), vec![1]);
        assert_eq!(plan(&r, 1_000 * DAY, 0, 4 * GB), vec![1, 2, 3]);
        assert!(plan(&r, 1_000 * DAY, 0, 15 * GB).is_empty());
    }

    #[test]
    fn age_and_cap_combine() {
        let r = [rec(1, 30, 5), rec(2, 3, 5), rec(3, 2, 5), rec(4, 1, 5)];
        // #1 goes by age; the remaining 15 GB exceed the 10 GB cap, so #2 goes too.
        assert_eq!(plan(&r, 1_000 * DAY, 14, 10 * GB), vec![1, 2]);
    }
}
