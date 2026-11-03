//! Daily memories (an idea from Daymark): one or two old bookmarks brought back on Home.
//!
//! - candidates are older than 7 days, not dismissed, and not shown on any of the 14 days
//!   before today (being shown today doesn't count, so the pick holds all day);
//! - ones with notes, tags or a summary come first: those are the ones worth a second look;
//! - the order is a shuffle seeded by the date, so it is stable for the day and different
//!   tomorrow; "show different ones" (`salt` 1, 2, …) steps through that same order.

use chrono::NaiveDate;

use crate::locks::sha256;
use crate::settings::MemoriesState;
use crate::types::{Bookmark, Memory};

pub const PER_DAY: usize = 2;
const MIN_AGE_DAYS: i64 = 7;
const QUIET_DAYS: i64 = 14;
/// `last_surfaced` entries older than this are dropped so settings don't grow forever.
const KEEP_DAYS: i64 = 30;

fn day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

/// splitmix64: tiny, good enough to shuffle a list, and the same on every Mac.
struct Rng(u64);

impl Rng {
    fn seeded(text: &str) -> Rng {
        let h = sha256(text);
        Rng(u64::from_str_radix(&h[..16], 16).unwrap_or(0))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

fn rich(b: &Bookmark) -> bool {
    b.has_notes || !b.tags.is_empty() || b.summary.is_some()
}

/// Today's memories. Pure: the caller records what was shown with [`record`].
pub fn pick(items: &[Bookmark], today: &str, salt: u32, state: &MemoriesState) -> Vec<Memory> {
    let Some(t) = day(today) else {
        return Vec::new();
    };
    let mut cands: Vec<(&Bookmark, i64)> = items
        .iter()
        .filter(|b| !state.dismissed.contains(&b.id))
        .filter_map(|b| {
            let age = (t - day(&b.added)?).num_days();
            (age >= MIN_AGE_DAYS).then_some((b, age))
        })
        .filter(|(b, _)| {
            state
                .last_surfaced
                .get(&b.id)
                .and_then(|d| day(d))
                .is_none_or(|d| {
                    let ago = (t - d).num_days();
                    ago <= 0 || ago > QUIET_DAYS
                })
        })
        .collect();
    // a fixed order before the shuffle, so the same day gives the same pick everywhere
    cands.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    let mut rng = Rng::seeded(&format!("memories|{today}"));
    rng.shuffle(&mut cands);
    cands.sort_by_key(|(b, _)| !rich(b)); // stable: the shuffle holds within each group

    if cands.is_empty() {
        return Vec::new();
    }
    let n = PER_DAY.min(cands.len());
    let start = (salt as usize).wrapping_mul(n) % cands.len();
    (0..n)
        .map(|i| cands[(start + i) % cands.len()])
        .map(|(b, age)| Memory {
            id: b.id.clone(),
            title: b.title.clone(),
            url: b.url.clone(),
            platform: b.platform,
            source: b.source.clone(),
            added: b.added.clone(),
            age_days: age as u32,
            summary: b.summary.clone(),
            tags: b.tags.clone(),
            has_notes: b.has_notes,
        })
        .collect()
}

/// Remember what was shown today and forget entries too old to matter.
pub fn record(state: &mut MemoriesState, shown: &[Memory], today: &str) {
    for m in shown {
        state.last_surfaced.insert(m.id.clone(), today.to_string());
    }
    if let Some(t) = day(today) {
        state
            .last_surfaced
            .retain(|_, d| day(d).is_some_and(|d| (t - d).num_days() <= KEEP_DAYS));
    }
}

/// Hide one for good.
pub fn dismiss(state: &mut MemoriesState, id: &str) {
    if !state.dismissed.iter().any(|d| d == id) {
        state.dismissed.push(id.to_string());
    }
    state.last_surfaced.remove(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Platform;

    fn bm(id: &str, added: &str, rich: bool) -> Bookmark {
        Bookmark {
            id: id.into(),
            title: id.into(),
            url: format!("https://example.com/{id}"),
            source: "chrome".into(),
            folder: String::new(),
            platform: Platform::Article,
            added: added.into(),
            tags: if rich { vec!["t".into()] } else { vec![] },
            summary: None,
            has_notes: false,
            status: None,
        }
    }

    fn many() -> Vec<Bookmark> {
        (0..20)
            .map(|i| bm(&format!("v:b{i:02}"), "2026-01-01", i % 4 == 0))
            .collect()
    }

    #[test]
    fn stable_for_a_day_and_prefers_rich_ones() {
        let s = MemoriesState::default();
        let a = pick(&many(), "2026-11-02", 0, &s);
        let b = pick(&many(), "2026-11-02", 0, &s);
        assert_eq!(a.len(), 2);
        assert_eq!(a, b);
        assert!(
            a.iter().all(|m| !m.tags.is_empty()),
            "rich ones first: {a:?}"
        );
        let other_days: Vec<_> = ["2026-11-03", "2026-11-04", "2026-11-05"]
            .iter()
            .map(|d| pick(&many(), d, 0, &s))
            .collect();
        assert!(other_days
            .iter()
            .any(|o| o.iter().map(|m| &m.id).ne(a.iter().map(|m| &m.id))));
    }

    #[test]
    fn salt_shows_different_ones() {
        let s = MemoriesState::default();
        let a = pick(&many(), "2026-11-02", 0, &s);
        let b = pick(&many(), "2026-11-02", 1, &s);
        assert!(a.iter().all(|m| !b.iter().any(|n| n.id == m.id)));
        assert_eq!(b, pick(&many(), "2026-11-02", 1, &s));
    }

    #[test]
    fn age_quiet_window_and_dismissal() {
        let items = vec![
            bm("v:new", "2026-10-30", true),
            bm("v:old", "2026-01-01", false),
            bm("v:shown", "2026-01-01", true),
            bm("v:gone", "2026-01-01", true),
            bm("v:nodate", "", true),
        ];
        let mut s = MemoriesState::default();
        s.last_surfaced
            .insert("v:shown".into(), "2026-10-25".into());
        dismiss(&mut s, "v:gone");
        let m = pick(&items, "2026-11-02", 0, &s);
        assert_eq!(
            m.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["v:old"]
        );
        assert!(m[0].age_days > 300);

        // shown today still counts as today's pick; 15 days ago is eligible again
        s.last_surfaced
            .insert("v:shown".into(), "2026-11-02".into());
        assert_eq!(pick(&items, "2026-11-02", 0, &s).len(), 2);
        let later: Vec<_> = items.iter().filter(|b| b.id != "v:new").cloned().collect();
        assert_eq!(pick(&later, "2026-11-17", 0, &s).len(), 2);
        assert_eq!(pick(&later, "2026-11-16", 0, &s).len(), 1);
    }

    #[test]
    fn record_prunes() {
        let mut s = MemoriesState::default();
        s.last_surfaced
            .insert("v:ancient".into(), "2025-01-01".into());
        let m = pick(&[bm("v:a", "2026-01-01", false)], "2026-11-02", 0, &s);
        record(&mut s, &m, "2026-11-02");
        assert_eq!(
            s.last_surfaced.get("v:a").map(String::as_str),
            Some("2026-11-02")
        );
        assert!(!s.last_surfaced.contains_key("v:ancient"));
        assert!(pick(&[], "2026-11-02", 3, &s).is_empty());
        assert!(pick(&many(), "not a date", 0, &s).is_empty());
    }
}
