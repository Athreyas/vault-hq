//! Dates and times in plain English, without a model: "tomorrow at 5pm", "Friday 9am",
//! "next week", "Nov 7", "in 3 days", "17:00". Everything is resolved against `now`, the
//! local date and time, so the same words always give the same answer for a given moment.
//!
//! The rules (kept small and predictable, since a model's answer is checked against them):
//! - a weekday alone, or "this <weekday>", is the next such day from today on (today counts);
//!   "next <weekday>" is that day in next week (Monday-based weeks);
//! - "next week" is next Monday, "next month" the 1st of next month, "weekend" the coming
//!   Saturday;
//! - a month-day date without a year that has already passed this year is next year's;
//! - "at 5" without am/pm means 17:00 for 1–6 and the hour itself otherwise;
//! - a time without a day is today if it is still ahead, otherwise tomorrow.
//!
//! Short weekday names (`mon`, `fri`, …) count only after `on`, `by`, `due`, `this`, `next`
//! or `until`, so "sat on the sofa" stays text.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct When {
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    /// Byte ranges of the text that said it, to cut from the description.
    pub spans: Vec<(usize, usize)>,
}

impl When {
    /// The text with the date and time words removed, whitespace tidied.
    pub fn strip(&self, text: &str) -> String {
        let mut spans = self.spans.clone();
        spans.sort();
        let mut out = String::new();
        let mut at = 0;
        for (s, e) in spans {
            if s >= at {
                out.push_str(&text[at..s]);
                out.push(' ');
                at = e;
            }
        }
        out.push_str(&text[at.min(text.len())..]);
        tidy(&out)
    }
}

/// Collapse whitespace, drop space before punctuation, and trim dangling joiners.
pub fn tidy(s: &str) -> String {
    let mut out = s.split_whitespace().collect::<Vec<_>>().join(" ");
    for p in [" ,", " .", " ;", " !", " ?"] {
        out = out.replace(p, &p[1..]);
    }
    loop {
        let t = out
            .trim()
            .trim_end_matches([',', ';', ':', '-', '.'])
            .trim()
            .to_string();
        let lower = t.to_lowercase();
        let cut = [
            "at", "on", "by", "due", "for", "before", "until", "and", "this", "next",
        ]
        .iter()
        .find_map(|w| lower.strip_suffix(&format!(" {w}")).map(|x| x.len()));
        match cut {
            Some(n) => out = t[..n].to_string(),
            None => {
                out = t;
                break;
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
struct Tok {
    s: String,
    start: usize,
    end: usize,
}

fn tokens(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let push = |out: &mut Vec<Tok>, a: usize, b: usize| {
        // a trailing '.', ':' or '-' ends a sentence, it is not part of the word
        let word = text[a..b].trim_end_matches(['.', ':', '-']);
        if !word.is_empty() {
            out.push(Tok {
                s: word.to_lowercase(),
                start: a,
                end: a + word.len(),
            });
        }
    };
    for (i, c) in text.char_indices() {
        // ':', '-' and '.' stay inside a word so 17:30, 9.30 and 2026-11-07 are one token
        let keep = c.is_alphanumeric() || (matches!(c, ':' | '-' | '.') && start.is_some());
        match (keep, start) {
            (true, None) => start = Some(i),
            (false, Some(a)) => {
                push(&mut out, a, i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(a) = start {
        push(&mut out, a, text.len());
    }
    out
}

const PREPS: &[&str] = &["at", "on", "by", "due", "before", "until", "for"];

fn weekday(s: &str, short_ok: bool) -> Option<Weekday> {
    let full = match s {
        "monday" => Some(Weekday::Mon),
        "tuesday" => Some(Weekday::Tue),
        "wednesday" => Some(Weekday::Wed),
        "thursday" => Some(Weekday::Thu),
        "friday" => Some(Weekday::Fri),
        "saturday" => Some(Weekday::Sat),
        "sunday" => Some(Weekday::Sun),
        _ => None,
    };
    if full.is_some() || !short_ok {
        return full;
    }
    match s {
        "mon" => Some(Weekday::Mon),
        "tue" | "tues" => Some(Weekday::Tue),
        "wed" => Some(Weekday::Wed),
        "thu" | "thur" | "thurs" => Some(Weekday::Thu),
        "fri" => Some(Weekday::Fri),
        "sat" => Some(Weekday::Sat),
        "sun" => Some(Weekday::Sun),
        _ => None,
    }
}

fn month(s: &str) -> Option<u32> {
    let m = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let full = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let s = s.trim_end_matches('.');
    full.iter()
        .position(|f| *f == s)
        .or_else(|| m.iter().position(|f| *f == s))
        .or_else(|| (s == "sept").then_some(8))
        .map(|i| i as u32 + 1)
}

/// `7`, `7th`, `21st`: a day of the month.
fn day_number(s: &str) -> Option<u32> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    let rest = &s[digits.len()..];
    if digits.is_empty() || digits.len() > 2 || !matches!(rest, "" | "st" | "nd" | "rd" | "th") {
        return None;
    }
    let d: u32 = digits.parse().ok()?;
    (1..=31).contains(&d).then_some(d)
}

fn year(s: &str) -> Option<i32> {
    (s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
        .filter(|y| (2000..2200).contains(y))
}

fn small_number(s: &str) -> Option<i64> {
    match s {
        "a" | "an" | "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        _ => s.parse().ok().filter(|n| (1..=999).contains(n)),
    }
}

/// `5pm`, `5:30pm`, `5`, `17:00`, `9.30am` with an optional separate `am`/`pm` token.
/// Returns the time and whether it was explicit (had a colon or am/pm).
fn clock(t: &str, ampm: Option<&str>) -> Option<(NaiveTime, bool)> {
    let (num, suffix) = match t.find(|c: char| c.is_ascii_alphabetic()) {
        Some(i) => (&t[..i], Some(&t[i..])),
        None => (t, ampm),
    };
    let suffix = suffix.map(|s| s.replace('.', ""));
    let pm = match suffix.as_deref() {
        Some("pm") | Some("p") => Some(true),
        Some("am") | Some("a") => Some(false),
        None => None,
        Some(_) => return None,
    };
    let (h, m, colon) = match num.split_once([':', '.']) {
        Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?, true),
        Some(_) => return None,
        None if !num.is_empty() && num.len() <= 2 => (num.parse::<u32>().ok()?, 0, false),
        None => return None,
    };
    if m > 59 {
        return None;
    }
    let h = match pm {
        Some(true) if (1..=12).contains(&h) => {
            if h == 12 {
                12
            } else {
                h + 12
            }
        }
        Some(false) if (1..=12).contains(&h) => {
            if h == 12 {
                0
            } else {
                h
            }
        }
        Some(_) => return None,
        None if h < 24 => h,
        None => return None,
    };
    Some((NaiveTime::from_hms_opt(h, m, 0)?, colon || pm.is_some()))
}

#[derive(Default)]
struct Hit {
    len: usize,
    date: Option<NaiveDate>,
    time: Option<NaiveTime>,
    /// A time implied by a word ("tonight", "morning"), overridden by an explicit one.
    soft: bool,
}

fn next_weekday(from: NaiveDate, wd: Weekday) -> NaiveDate {
    let ahead = (wd.num_days_from_monday() + 7 - from.weekday().num_days_from_monday()) % 7;
    from + Duration::days(ahead as i64)
}

fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

fn add_months(d: NaiveDate, n: i64) -> Option<NaiveDate> {
    let total = d.year() as i64 * 12 + d.month0() as i64 + n;
    let (y, m) = ((total / 12) as i32, (total % 12) as u32 + 1);
    (0..4).find_map(|back| NaiveDate::from_ymd_opt(y, m, d.day() - back))
}

fn at(toks: &[Tok], i: usize) -> &str {
    toks.get(i).map(|t| t.s.as_str()).unwrap_or("")
}

/// The pattern starting at token `i`, if any.
fn match_at(toks: &[Tok], i: usize, now: NaiveDateTime) -> Option<Hit> {
    let today = now.date();
    let w = at(toks, i);
    let prev = if i > 0 { at(toks, i - 1) } else { "" };
    let hit = |len: usize, date: Option<NaiveDate>, time: Option<NaiveTime>| {
        Some(Hit {
            len,
            date,
            time,
            soft: false,
        })
    };
    // ISO date
    if w.len() == 10 {
        if let Ok(d) = NaiveDate::parse_from_str(w, "%Y-%m-%d") {
            return hit(1, Some(d), None);
        }
    }
    match w {
        "today" => return hit(1, Some(today), None),
        "tonight" => {
            return Some(Hit {
                len: 1,
                date: Some(today),
                time: NaiveTime::from_hms_opt(20, 0, 0),
                soft: true,
            })
        }
        "tomorrow" | "tmrw" | "tmr" | "tomorow" => {
            return hit(1, Some(today + Duration::days(1)), None)
        }
        "noon" | "midday" => return hit(1, None, NaiveTime::from_hms_opt(12, 0, 0)),
        "day" if at(toks, i + 1) == "after" && at(toks, i + 2) == "tomorrow" => {
            return hit(3, Some(today + Duration::days(2)), None)
        }
        "morning" | "afternoon" | "evening" => {
            let h = match w {
                "morning" => 9,
                "afternoon" => 15,
                _ => 18,
            };
            // "this evening" is today; "in the morning" and "tomorrow morning" only set the hour
            let date = (prev == "this").then_some(today);
            return Some(Hit {
                len: 1,
                date,
                time: NaiveTime::from_hms_opt(h, 0, 0),
                soft: true,
            });
        }
        "next" | "this" => {
            let n = at(toks, i + 1);
            if let Some(wd) = weekday(n, true) {
                let d = if w == "next" {
                    monday_of(today) + Duration::days(7 + wd.num_days_from_monday() as i64)
                } else {
                    next_weekday(today, wd)
                };
                return hit(2, Some(d), None);
            }
            match (w, n) {
                ("next", "week") => {
                    return hit(2, Some(monday_of(today) + Duration::days(7)), None)
                }
                ("next", "month") => {
                    let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
                    return hit(2, add_months(first, 1), None);
                }
                ("next", "year") => {
                    return hit(2, NaiveDate::from_ymd_opt(today.year() + 1, 1, 1), None)
                }
                ("this", "weekend") => return hit(2, Some(weekend(today)), None),
                _ => {}
            }
        }
        "weekend" => return hit(1, Some(weekend(today)), None),
        "in" => {
            // in N days/weeks/months/hours/minutes, in a week
            if let Some(n) = small_number(at(toks, i + 1)) {
                let unit = at(toks, i + 2).trim_end_matches('s');
                let (date, time) = match unit {
                    "day" => (Some(today + Duration::days(n)), None),
                    "week" => (Some(today + Duration::weeks(n)), None),
                    "month" => (add_months(today, n), None),
                    "hour" | "hr" => {
                        let t = now + Duration::hours(n);
                        (Some(t.date()), Some(round_minute(t.time())))
                    }
                    "minute" | "min" => {
                        let t = now + Duration::minutes(n);
                        (Some(t.date()), Some(round_minute(t.time())))
                    }
                    _ => return None,
                };
                return hit(3, date, time);
            }
            if at(toks, i + 1) == "the"
                && matches!(at(toks, i + 2), "morning" | "afternoon" | "evening")
            {
                let mut h = match_at(toks, i + 2, now)?;
                h.len += 2;
                return Some(h);
            }
        }
        _ => {}
    }
    let short_ok = matches!(
        prev,
        "on" | "by" | "due" | "this" | "next" | "until" | "before"
    );
    if let Some(wd) = weekday(w, short_ok) {
        return hit(1, Some(next_weekday(today, wd)), None);
    }
    // Month Day [Year] / Day [of] Month [Year]
    if let Some(m) = month(w) {
        if let Some(d) = day_number(at(toks, i + 1)) {
            let y = year(at(toks, i + 2));
            return hit(2 + y.is_some() as usize, month_day(today, m, d, y), None);
        }
    }
    if let Some(d) = day_number(w) {
        let of = (at(toks, i + 1) == "of") as usize;
        if let Some(m) = month(at(toks, i + 1 + of)) {
            let y = year(at(toks, i + 2 + of));
            return hit(
                2 + of + y.is_some() as usize,
                month_day(today, m, d, y),
                None,
            );
        }
    }
    // times: "at 5", "5pm", "5 pm", "17:00", "at 9.30"
    let next = at(toks, i + 1);
    let ampm = matches!(next, "am" | "pm" | "a.m" | "p.m" | "a" | "p").then_some(next);
    if let Some((t, explicit)) = clock(w, ampm) {
        let len = 1 + ampm.is_some() as usize;
        if explicit {
            return hit(len, None, Some(t));
        }
        if prev == "at" {
            // a bare hour: 1–6 is afternoon, the rest as said
            let h = t.format("%H").to_string().parse::<u32>().unwrap_or(0);
            let t = if (1..=6).contains(&h) {
                NaiveTime::from_hms_opt(h + 12, 0, 0)?
            } else {
                t
            };
            return hit(len, None, Some(t));
        }
    }
    None
}

fn round_minute(t: NaiveTime) -> NaiveTime {
    NaiveTime::from_hms_opt(
        t.format("%H").to_string().parse().unwrap_or(0),
        t.format("%M").to_string().parse().unwrap_or(0),
        0,
    )
    .unwrap_or(t)
}

fn weekend(today: NaiveDate) -> NaiveDate {
    if today.weekday() == Weekday::Sun {
        today
    } else {
        next_weekday(today, Weekday::Sat)
    }
}

fn month_day(today: NaiveDate, m: u32, d: u32, y: Option<i32>) -> Option<NaiveDate> {
    match y {
        Some(y) => NaiveDate::from_ymd_opt(y, m, d),
        None => {
            let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
            if this < today {
                NaiveDate::from_ymd_opt(today.year() + 1, m, d)
            } else {
                Some(this)
            }
        }
    }
}

/// Find the date and time in `text`, as of `now` (local).
pub fn parse(text: &str, now: NaiveDateTime) -> When {
    let toks = tokens(text);
    let mut w = When::default();
    let mut soft_time = false;
    let mut i = 0;
    while i < toks.len() {
        let Some(h) = match_at(&toks, i, now) else {
            i += 1;
            continue;
        };
        let wants_date = h.date.is_some();
        let wants_time = h.time.is_some();
        let date_ok = !wants_date || w.date.is_none();
        let time_ok = !wants_time || w.time.is_none() || (soft_time && !h.soft);
        if date_ok && time_ok && (wants_date || wants_time) {
            if let Some(d) = h.date {
                w.date = Some(d);
            }
            if let Some(t) = h.time {
                // "tomorrow morning at 7": the explicit 7 wins over the soft 9:00
                if w.time.is_none() || (soft_time && !h.soft) {
                    w.time = Some(t);
                    soft_time = h.soft;
                }
            }
            // take "by", "on the", "due" … in front of it along
            let mut first = i;
            if first > 0 && at(&toks, first - 1) == "the" {
                first -= 1;
            }
            if first > 0 && PREPS.contains(&at(&toks, first - 1)) {
                first -= 1;
            }
            if first < i
                && at(&toks, first) == "the"
                && first + 1 == i
                && !matches!(at(&toks, i), "day")
            {
                // a lone "the" before a date that is not "the day after…" stays text
                first = i;
            }
            let claimed = w.spans.iter().map(|s| s.1).max().unwrap_or(0);
            while first < i && toks[first].start < claimed {
                first += 1;
            }
            let start = toks[first].start;
            // "in the morning": "in the" belongs to it; already counted in len
            w.spans.push((start, toks[i + h.len - 1].end));
        }
        i += h.len;
    }
    if w.date.is_none() {
        if let Some(t) = w.time {
            // a time alone: the next time the clock shows it
            w.date = Some(if t > now.time() {
                now.date()
            } else {
                now.date() + Duration::days(1)
            });
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tuesday 2026-11-03, 10:30 local.
    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 3)
            .unwrap()
            .and_hms_opt(10, 30, 0)
            .unwrap()
    }

    fn fmt(w: &When) -> String {
        format!(
            "{} {}",
            w.date.map(|d| d.to_string()).unwrap_or("-".into()),
            w.time
                .map(|t| t.format("%H:%M").to_string())
                .unwrap_or("-".into())
        )
    }

    #[test]
    fn table() {
        let cases: &[(&str, &str, &str)] = &[
            // text, "date time", description left over
            (
                "build the feature tomorrow at 5PM",
                "2026-11-04 17:00",
                "build the feature",
            ),
            (
                "call the bank Friday 9am",
                "2026-11-06 09:00",
                "call the bank",
            ),
            (
                "call the bank on Friday at 9:30 am",
                "2026-11-06 09:30",
                "call the bank",
            ),
            ("pay rent today", "2026-11-03 -", "pay rent"),
            ("pay rent tonight", "2026-11-03 20:00", "pay rent"),
            ("pay rent tonight at 22:15", "2026-11-03 22:15", "pay rent"),
            ("water plants at 17:00", "2026-11-03 17:00", "water plants"),
            ("water plants at 9am", "2026-11-04 09:00", "water plants"),
            ("water plants at 5", "2026-11-03 17:00", "water plants"),
            ("standup at noon", "2026-11-03 12:00", "standup"),
            ("review on Tuesday", "2026-11-03 -", "review"),
            ("review this tue", "2026-11-03 -", "review"),
            ("review next tuesday", "2026-11-10 -", "review"),
            ("review next friday", "2026-11-13 -", "review"),
            ("plan next week", "2026-11-09 -", "plan"),
            ("plan next month", "2026-12-01 -", "plan"),
            (
                "clean the garage this weekend",
                "2026-11-07 -",
                "clean the garage",
            ),
            ("renew passport by Nov 20", "2026-11-20 -", "renew passport"),
            (
                "renew passport by 20th November",
                "2026-11-20 -",
                "renew passport",
            ),
            (
                "renew passport by the 3rd of March",
                "2027-03-03 -",
                "renew passport",
            ),
            ("taxes due April 15 2027", "2027-04-15 -", "taxes"),
            ("dentist 2026-12-01 at 8:15", "2026-12-01 08:15", "dentist"),
            ("follow up in 3 days", "2026-11-06 -", "follow up"),
            ("follow up in a week", "2026-11-10 -", "follow up"),
            (
                "check the oven in 2 hours",
                "2026-11-03 12:30",
                "check the oven",
            ),
            ("gym tomorrow morning", "2026-11-04 09:00", "gym"),
            ("gym tomorrow morning at 7", "2026-11-04 07:00", "gym"),
            (
                "email Sam the day after tomorrow",
                "2026-11-05 -",
                "email Sam",
            ),
            ("sat on the sofa", "- -", "sat on the sofa"),
            ("march to the shop", "- -", "march to the shop"),
            ("read 5 chapters", "- -", "read 5 chapters"),
            ("may the force", "- -", "may the force"),
        ];
        for (text, want, desc) in cases {
            let w = parse(text, now());
            assert_eq!(fmt(&w), *want, "{text}");
            assert_eq!(w.strip(text), *desc, "{text}");
        }
    }

    #[test]
    fn tidy_trims_joiners() {
        assert_eq!(tidy("call mum , then  at"), "call mum, then");
        assert_eq!(tidy("ship it by"), "ship it");
        assert_eq!(tidy("  "), "");
    }

    #[test]
    fn month_end_and_years() {
        let jan31 = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        assert_eq!(add_months(jan31, 1), NaiveDate::from_ymd_opt(2026, 2, 28));
        assert_eq!(
            month_day(jan31, 1, 1, None),
            NaiveDate::from_ymd_opt(2027, 1, 1)
        );
        assert_eq!(month_day(jan31, 2, 30, None), None);
    }
}
