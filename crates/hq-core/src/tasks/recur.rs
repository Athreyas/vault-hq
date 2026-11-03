//! `🔁` rules and the next occurrence of a recurring task (SPEC 4.1):
//! `every day|week|month|year|N days|N weeks|N months|N years|weekday`, optionally
//! followed by `when done` (count from the day it was done, not from its dates).

use chrono::{Datelike, Days, Months, NaiveDate, Weekday};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Day,
    Week,
    Month,
    Year,
    Weekday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub every: u32,
    pub unit: Unit,
    pub when_done: bool,
}

pub fn parse_rule(text: &str) -> Result<Rule> {
    let lower = text.trim().to_lowercase();
    let bad = || {
        Error::Invalid(format!(
            "\"{text}\" is not a repeat rule HQ understands. Try \"every week\", \"every 3 days\", \"every month\" or \"every weekday\"."
        ))
    };
    let mut words: Vec<&str> = lower.split_whitespace().collect();
    let when_done = words.ends_with(&["when", "done"]);
    if when_done {
        words.truncate(words.len() - 2);
    }
    if words.first() != Some(&"every") {
        return Err(bad());
    }
    let (every, unit_word) = match &words[1..] {
        [u] => (1, *u),
        [n, u] => (n.parse::<u32>().map_err(|_| bad())?, *u),
        _ => return Err(bad()),
    };
    if every == 0 || every > 1000 {
        return Err(bad());
    }
    let unit = match unit_word.trim_end_matches('s') {
        "day" => Unit::Day,
        "week" => Unit::Week,
        "month" => Unit::Month,
        "year" => Unit::Year,
        "weekday" if every == 1 => Unit::Weekday,
        _ => return Err(bad()),
    };
    Ok(Rule {
        every,
        unit,
        when_done,
    })
}

fn add(date: NaiveDate, rule: &Rule) -> Option<NaiveDate> {
    match rule.unit {
        Unit::Day => date.checked_add_days(Days::new(rule.every as u64)),
        Unit::Week => date.checked_add_days(Days::new(7 * rule.every as u64)),
        // chrono clamps to the month's last day (Jan 31 + 1 month = Feb 28/29)
        Unit::Month => date.checked_add_months(Months::new(rule.every)),
        Unit::Year => date.checked_add_months(Months::new(12 * rule.every)),
        Unit::Weekday => {
            let mut d = date.succ_opt()?;
            while matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
                d = d.succ_opt()?;
            }
            Some(d)
        }
    }
}

/// The dates of the next occurrence. `dates` are the task's start/scheduled/due (any may
/// be missing); the reference is the due date, else scheduled, else start, else the day
/// it was done. Every date moves by the same step as the reference, so "start two days
/// before due" stays true. Months and years move each date by calendar months.
pub fn next_dates(
    rule: &Rule,
    dates: [Option<NaiveDate>; 3],
    done_on: NaiveDate,
) -> Result<[Option<NaiveDate>; 3]> {
    let overflow = || Error::Invalid("The next occurrence falls outside the calendar.".into());
    let Some(anchor) = dates[2].or(dates[1]).or(dates[0]) else {
        // a recurring task with no dates: the next one is simply due one step after it was done
        return Ok([None, None, Some(add(done_on, rule).ok_or_else(overflow)?)]);
    };
    let reference = if rule.when_done { done_on } else { anchor };
    let next_ref = add(reference, rule).ok_or_else(overflow)?;
    let mut out = [None; 3];
    for (i, d) in dates.iter().enumerate() {
        let Some(d) = d else { continue };
        out[i] = Some(match rule.unit {
            // calendar steps move each date on its own, unless it is the reference itself
            Unit::Month | Unit::Year if !rule.when_done => {
                if *d == anchor {
                    next_ref
                } else {
                    add(*d, rule).ok_or_else(overflow)?
                }
            }
            // otherwise every date keeps its distance from the anchor, which lands on next_ref
            _ => {
                let shift = next_ref.signed_duration_since(anchor);
                d.checked_add_signed(shift).ok_or_else(overflow)?
            }
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn rules() {
        let r = parse_rule("every week").unwrap();
        assert_eq!((r.every, r.unit, r.when_done), (1, Unit::Week, false));
        let r = parse_rule("Every 3 Days when done").unwrap();
        assert_eq!((r.every, r.unit, r.when_done), (3, Unit::Day, true));
        assert_eq!(parse_rule("every weekday").unwrap().unit, Unit::Weekday);
        assert_eq!(parse_rule("every 2 months").unwrap().every, 2);
        assert_eq!(parse_rule("every year").unwrap().unit, Unit::Year);
        for bad in [
            "",
            "weekly",
            "every",
            "every 0 days",
            "every fortnight",
            "every 2 weekdays",
            "every x days",
        ] {
            assert!(parse_rule(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn next_keeps_the_gap_between_dates() {
        let r = parse_rule("every week").unwrap();
        let n = next_dates(
            &r,
            [Some(d("2026-10-28")), None, Some(d("2026-11-01"))],
            d("2026-11-03"),
        )
        .unwrap();
        assert_eq!(n, [Some(d("2026-11-04")), None, Some(d("2026-11-08"))]);
    }

    #[test]
    fn months_clamp_and_weekdays_skip() {
        let r = parse_rule("every month").unwrap();
        let n = next_dates(&r, [None, None, Some(d("2026-01-31"))], d("2026-01-31")).unwrap();
        assert_eq!(n[2], Some(d("2026-02-28")));
        let r = parse_rule("every weekday").unwrap();
        let n = next_dates(&r, [None, None, Some(d("2026-10-30"))], d("2026-10-30")).unwrap();
        assert_eq!(n[2], Some(d("2026-11-02")), "Friday → Monday");
        let r = parse_rule("every year").unwrap();
        let n = next_dates(&r, [None, Some(d("2028-02-29")), None], d("2028-03-01")).unwrap();
        assert_eq!(n[1], Some(d("2029-02-28")));
    }

    #[test]
    fn when_done_counts_from_completion() {
        let r = parse_rule("every 3 days when done").unwrap();
        let n = next_dates(
            &r,
            [None, Some(d("2026-10-01")), Some(d("2026-10-02"))],
            d("2026-10-10"),
        )
        .unwrap();
        assert_eq!(n, [None, Some(d("2026-10-12")), Some(d("2026-10-13"))]);
        let n = next_dates(&r, [None, None, None], d("2026-10-10")).unwrap();
        assert_eq!(n, [None, None, Some(d("2026-10-13"))]);
    }
}
