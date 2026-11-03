//! Reading task lines and the notes that hold them. Parsing never changes a byte:
//! `parse_line(l).print() == l` for every line that parses (see the round-trip tests).

use chrono::NaiveDate;

use super::model::{FieldKind, Seg, TaskLine, SIGNIFIERS};
use crate::frontmatter;
use crate::types::Annotation;

const VS16: char = '\u{fe0f}';

/// A checkbox list item, or `None` for any other line.
pub fn parse_line(line: &str) -> Option<TaskLine> {
    // indent: whitespace, and block-quote markers so tasks in callouts count too
    let indent_len = line
        .char_indices()
        .find(|(_, c)| !(c.is_whitespace() || *c == '>'))
        .map(|(i, _)| i)?;
    let indent = &line[..indent_len];
    if indent.contains('>') && !indent.trim_start().starts_with('>') {
        return None;
    }
    let rest = &line[indent_len..];
    let bullet_len = if rest.starts_with(['-', '*', '+']) {
        1
    } else {
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 || !rest[digits..].starts_with(['.', ')']) {
            return None;
        }
        digits + 1
    };
    let bullet = &rest[..bullet_len];
    let rest = &rest[bullet_len..];
    let gap_len = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    if gap_len == 0 {
        return None;
    }
    let gap = &rest[..gap_len];
    let rest = &rest[gap_len..];
    let mut chars = rest.chars();
    if chars.next() != Some('[') {
        return None;
    }
    let status = chars.next()?;
    if status == ']' || status == '\n' || chars.next() != Some(']') {
        return None;
    }
    let body = chars.as_str();
    if !(body.is_empty() || body.starts_with(char::is_whitespace)) {
        return None;
    }
    Some(TaskLine {
        indent: indent.to_string(),
        bullet: bullet.to_string(),
        gap: gap.to_string(),
        status,
        body: parse_body(body),
    })
}

fn parse_body(body: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < body.len() {
        let here = &body[i..];
        if let Some((emoji_len, kind)) = signifier_at(here) {
            if let Some((sep_len, value_len)) = value_after(kind, &here[emoji_len..]) {
                let trimmed = text.trim_end().len();
                let lead = text.split_off(trimmed);
                if !text.is_empty() {
                    segs.push(Seg::Text(std::mem::take(&mut text)));
                }
                let emoji = &here[..emoji_len];
                let sep = &here[emoji_len..emoji_len + sep_len];
                let value = &here[emoji_len + sep_len..emoji_len + sep_len + value_len];
                segs.push(Seg::Field {
                    lead,
                    kind,
                    emoji: emoji.to_string(),
                    sep: sep.to_string(),
                    value: value.to_string(),
                });
                i += emoji_len + sep_len + value_len;
                continue;
            }
        }
        let c = here.chars().next().expect("i < len");
        text.push(c);
        i += c.len_utf8();
    }
    if !text.is_empty() {
        segs.push(Seg::Text(text));
    }
    segs
}

/// The byte length of the emoji at the start of `s` (with its variation selector), and its field.
fn signifier_at(s: &str) -> Option<(usize, FieldKind)> {
    SIGNIFIERS.iter().find_map(|(e, k)| {
        s.strip_prefix(e).map(|after| {
            let vs = if after.starts_with(VS16) {
                VS16.len_utf8()
            } else {
                0
            };
            (e.len() + vs, *k)
        })
    })
}

/// `(whitespace length, value length)` if a valid value follows the emoji.
fn value_after(kind: FieldKind, s: &str) -> Option<(usize, usize)> {
    if kind == FieldKind::Priority {
        return Some((0, 0));
    }
    let sep = s.len() - s.trim_start_matches([' ', '\t']).len();
    let v = &s[sep..];
    let len = match kind {
        FieldKind::Priority => 0,
        k if k.is_date() => {
            let d = v.get(..10)?;
            NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
            if v[10..].starts_with(|c: char| c.is_alphanumeric()) {
                return None;
            }
            10
        }
        FieldKind::DueTime => {
            let t = v.get(..5)?;
            check_time(t)?;
            if v[5..].starts_with(|c: char| c.is_alphanumeric() || c == ':') {
                return None;
            }
            5
        }
        FieldKind::Id => id_len(v),
        FieldKind::Depends => {
            let mut len = id_len(v);
            if len == 0 {
                return None;
            }
            loop {
                let rest = &v[len..];
                let comma = rest.len() - rest.trim_start_matches([' ', ',']).len();
                if !rest[..comma].contains(',') {
                    break;
                }
                let next = id_len(&rest[comma..]);
                if next == 0 {
                    break;
                }
                len += comma + next;
            }
            len
        }
        FieldKind::Recur => {
            // up to the next field's emoji, without the whitespace before it
            let mut end = v.len();
            for (j, _) in v.char_indices() {
                if signifier_at(&v[j..]).is_some() {
                    end = j;
                    break;
                }
            }
            v[..end].trim_end().len()
        }
        _ => 0,
    };
    (len > 0).then_some((sep, len))
}

/// `HH:MM`, 24-hour, as HQ writes it after `⏰`.
pub fn check_time(s: &str) -> Option<(u32, u32)> {
    let b = s.as_bytes();
    if b.len() != 5
        || b[2] != b':'
        || !s
            .chars()
            .enumerate()
            .all(|(i, c)| i == 2 || c.is_ascii_digit())
    {
        return None;
    }
    let h: u32 = s[..2].parse().ok()?;
    let m: u32 = s[3..].parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

fn id_len(s: &str) -> usize {
    s.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .map(char::len_utf8)
        .sum()
}

/// A task found in a note, with where it sits.
#[derive(Debug, Clone)]
pub struct Found {
    /// 0-based line index in the text that was parsed.
    pub line: usize,
    /// Index just past the task's annotation lines.
    pub end: usize,
    pub task: TaskLine,
    pub raw: String,
    pub annotations: Vec<Annotation>,
}

/// Width of an indent, tabs counting four.
fn indent_width(s: &str) -> usize {
    s.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// `(text, date)` from an annotation line's content: a trailing `(YYYY-MM-DD)` is its date.
pub fn parse_annotation(content: &str) -> Annotation {
    let c = content.trim();
    if let Some(open) = c.strip_suffix(')').and_then(|x| x.rfind('(')) {
        let date = &c[open + 1..c.len() - 1];
        if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok() {
            return Annotation {
                text: c[..open].trim_end().to_string(),
                date: Some(date.to_string()),
            };
        }
    }
    Annotation {
        text: c.to_string(),
        date: None,
    }
}

/// The lines of a text without their line endings.
pub fn lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// Every task in a whole note (frontmatter and fenced code are skipped). Line indexes
/// count from the top of the file.
pub fn parse_note(text: &str) -> Vec<Found> {
    let fm = frontmatter::parse(text).len;
    let skip = text[..fm].matches('\n').count();
    let mut out = parse_lines(&text[fm..]);
    for f in &mut out {
        f.line += skip;
        f.end += skip;
    }
    out
}

/// Tasks in text with no frontmatter.
pub fn parse_lines(text: &str) -> Vec<Found> {
    let ls = lines(text);
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    let mut i = 0;
    while i < ls.len() {
        let l = ls[i];
        let t = l.trim_start();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            i += 1;
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(&t[..3]);
            i += 1;
            continue;
        }
        let Some(task) = parse_line(l) else {
            i += 1;
            continue;
        };
        let width = indent_width(&task.indent);
        let mut annotations = Vec::new();
        let mut j = i + 1;
        while j < ls.len() {
            let n = ls[j];
            if n.trim().is_empty() || indent_width(n) <= width || parse_line(n).is_some() {
                break;
            }
            let Some(content) = n.trim_start().strip_prefix("- ") else {
                break;
            };
            annotations.push(parse_annotation(content));
            j += 1;
        }
        out.push(Found {
            line: i,
            end: j,
            task,
            raw: l.to_string(),
            annotations,
        });
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Priority, TaskStatus};

    fn rt(line: &str) -> TaskLine {
        let t = parse_line(line).unwrap_or_else(|| panic!("should parse: {line}"));
        assert_eq!(t.print(), line, "round trip");
        t
    }

    #[test]
    fn spec_example_round_trips_with_every_field() {
        let line = "- [ ] Lay the main line along the fence #garden #outdoor ⏫ ➕ 2026-10-20 🛫 2026-10-28 ⏳ 2026-10-30 📅 2026-11-01 🔁 every week 🆔 a1b2c3 ⛔ x9y8z7";
        let t = rt(line);
        assert_eq!(t.status(), TaskStatus::Pending);
        assert_eq!(t.description(), "Lay the main line along the fence");
        assert_eq!(t.tags(), vec!["garden", "outdoor"]);
        assert_eq!(t.priority(), Some(Priority::High));
        assert_eq!(t.get(FieldKind::Created), Some("2026-10-20"));
        assert_eq!(t.get(FieldKind::Start), Some("2026-10-28"));
        assert_eq!(t.get(FieldKind::Scheduled), Some("2026-10-30"));
        assert_eq!(t.get(FieldKind::Due), Some("2026-11-01"));
        assert_eq!(t.get(FieldKind::Recur), Some("every week"));
        assert_eq!(t.get(FieldKind::Id), Some("a1b2c3"));
        assert_eq!(t.depends(), vec!["x9y8z7"]);
    }

    /// Every subset of fields, in two orders, with every status and bullet, round-trips
    /// and reads back the same values.
    #[test]
    fn every_field_combination_round_trips() {
        let fields: &[(&str, FieldKind, &str)] = &[
            ("🔼", FieldKind::Priority, ""),
            ("➕ 2026-01-02", FieldKind::Created, "2026-01-02"),
            ("🛫 2026-01-03", FieldKind::Start, "2026-01-03"),
            ("⏳ 2026-01-04", FieldKind::Scheduled, "2026-01-04"),
            ("📅 2026-01-05", FieldKind::Due, "2026-01-05"),
            ("⏰ 17:30", FieldKind::DueTime, "17:30"),
            (
                "🔁 every 2 weeks when done",
                FieldKind::Recur,
                "every 2 weeks when done",
            ),
            ("✅ 2026-01-06", FieldKind::Done, "2026-01-06"),
            ("❌ 2026-01-07", FieldKind::Cancelled, "2026-01-07"),
            ("🆔 abc123", FieldKind::Id, "abc123"),
            ("⛔ aaa111,bbb222", FieldKind::Depends, "aaa111,bbb222"),
        ];
        let statuses = [' ', '/', 'x', 'X', '-', '>', '!'];
        let bullets = ["- ", "* ", "+ ", "1. ", "12) "];
        let mut n = 0;
        for mask in 0u32..(1 << fields.len()) {
            let chosen: Vec<_> = fields
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, f)| f)
                .collect();
            for reversed in [false, true] {
                let mut order = chosen.clone();
                if reversed {
                    order.reverse();
                }
                let status = statuses[n % statuses.len()];
                let bullet = bullets[n % bullets.len()];
                let indent = ["", "  ", "\t", "> "][n % 4];
                n += 1;
                let mut line = format!("{indent}{bullet}[{status}] Do the thing #a/b");
                for (text, _, _) in &order {
                    line.push(' ');
                    line.push_str(text);
                }
                let t = rt(&line);
                assert_eq!(t.description(), "Do the thing");
                assert_eq!(t.tags(), vec!["a/b"]);
                for (_, kind, value) in &order {
                    if *kind == FieldKind::Priority {
                        assert_eq!(t.priority(), Some(Priority::Medium));
                    } else {
                        assert_eq!(t.get(*kind), Some(*value), "{line}");
                    }
                }
            }
        }
        assert_eq!(n, 4096);
    }

    #[test]
    fn unknown_emoji_and_odd_spacing_are_kept() {
        for line in [
            "- [ ] Water 🌱 the beds 🏁 delete 📅 2026-02-01",
            "- [ ]   spaced   out  📅  2026-02-01   ",
            "- [x] done ✅ 2026-01-01 and a trailing note",
            "- [ ] variation ⛔️ abc123 ⏳️ 2026-01-01",
            "- [ ]",
            "- [ ] ",
            "- [ ] 📅 not-a-date #tag",
            "- [ ] 🆔",
            "- [ ] issue #123 is not a tag, but #x1 is",
            "- [ ] tabs\tbetween\t📅\t2026-03-03",
            "    - [/] nested started 🔺",
            "- [ ] ⏫⏫ twice",
            "- [ ] 🔁 every day 🔁 every week",
            "- [ ] with [[link|alias]] and `code 📅 2026-01-01` and **bold**",
        ] {
            rt(line);
        }
        let t = rt("- [ ] Water 🌱 the beds 🏁 delete 📅 2026-02-01");
        assert_eq!(t.description(), "Water 🌱 the beds 🏁 delete");
        let t = rt("- [ ] variation ⛔️ abc123 ⏳️ 2026-01-01");
        assert_eq!(t.depends(), vec!["abc123"]);
        assert_eq!(t.get(FieldKind::Scheduled), Some("2026-01-01"));
        let t = rt("- [ ] 📅 not-a-date #tag");
        assert_eq!(t.get(FieldKind::Due), None);
        assert_eq!(t.tags(), vec!["tag"]);
        let t = rt("- [ ] issue #123 is not a tag, but #x1 is");
        assert_eq!(t.tags(), vec!["x1"]);
        let t = rt("    - [/] nested started 🔺");
        assert_eq!(t.status(), TaskStatus::Started);
        assert_eq!(t.priority(), Some(Priority::Highest));
        assert_eq!(rt("- [!] custom").status(), TaskStatus::Pending);
        assert_eq!(rt("- [>] w").status(), TaskStatus::Waiting);
    }

    #[test]
    fn not_tasks() {
        for line in [
            "",
            "plain text",
            "- plain bullet",
            "-[ ] no gap",
            "- [ ]x glued",
            "- [] empty box",
            "- [ab] two chars",
            "# - [ ] heading",
            "text > - [ ] quote in the middle",
        ] {
            assert!(parse_line(line).is_none(), "{line:?}");
        }
    }

    #[test]
    fn edits_touch_only_their_field() {
        let mut t = rt("- [ ] Fix the gate #home ⏫ 📅 2026-01-05 🆔 abc123");
        t.set(FieldKind::Due, Some("2026-02-01"));
        assert_eq!(
            t.print(),
            "- [ ] Fix the gate #home ⏫ 📅 2026-02-01 🆔 abc123"
        );
        t.set(FieldKind::Created, Some("2026-01-01"));
        assert_eq!(
            t.print(),
            "- [ ] Fix the gate #home ⏫ ➕ 2026-01-01 📅 2026-02-01 🆔 abc123"
        );
        t.set(FieldKind::Done, Some("2026-01-09"));
        assert_eq!(
            t.print(),
            "- [ ] Fix the gate #home ⏫ ➕ 2026-01-01 📅 2026-02-01 ✅ 2026-01-09 🆔 abc123"
        );
        t.set(FieldKind::Due, None);
        t.set_priority(None);
        assert_eq!(
            t.print(),
            "- [ ] Fix the gate #home ➕ 2026-01-01 ✅ 2026-01-09 🆔 abc123"
        );
        t.set_priority(Some(Priority::Low));
        assert_eq!(
            t.print(),
            "- [ ] Fix the gate #home 🔽 ➕ 2026-01-01 ✅ 2026-01-09 🆔 abc123"
        );
        t.add_tag("urgent");
        assert_eq!(
            t.print(),
            "- [ ] Fix the gate #home #urgent 🔽 ➕ 2026-01-01 ✅ 2026-01-09 🆔 abc123"
        );
        assert!(t.remove_tag("home"));
        assert!(!t.remove_tag("nope"));
        assert_eq!(
            t.print(),
            "- [ ] Fix the gate #urgent 🔽 ➕ 2026-01-01 ✅ 2026-01-09 🆔 abc123"
        );
        t.set_description("Fix the side gate");
        assert_eq!(
            t.print(),
            "- [ ] Fix the side gate #urgent 🔽 ➕ 2026-01-01 ✅ 2026-01-09 🆔 abc123"
        );
        t.set_status(TaskStatus::Done);
        assert!(t.print().starts_with("- [x] "));
    }

    #[test]
    fn adding_fields_to_a_bare_line() {
        let mut t = rt("- [ ] Call the plumber");
        t.set(FieldKind::Id, Some("zzz999"));
        t.set(FieldKind::Created, Some("2026-01-01"));
        t.add_tag("home");
        assert_eq!(
            t.print(),
            "- [ ] Call the plumber #home ➕ 2026-01-01 🆔 zzz999"
        );
        let mut t = rt("- [ ] 📅 2026-01-01");
        t.add_tag("x");
        assert_eq!(t.print(), "- [ ] #x 📅 2026-01-01");
        let mut t = rt("- [ ] trailing   ");
        t.set(FieldKind::Due, Some("2026-01-01"));
        assert_eq!(t.print(), "- [ ] trailing 📅 2026-01-01   ");
    }

    #[test]
    fn notes_with_annotations_fences_and_frontmatter() {
        let text = "---\ntitle: T\ntype: project\n---\n\n## Tasks\n- [ ] one 🆔 aaa111\n    - parts are in the garage (2026-10-29)\n    - Annotation: second\n  - [ ] child\n```\n- [ ] in code\n```\n- [x] two\r\n- not a task\n";
        let found = parse_note(text);
        let raws: Vec<_> = found.iter().map(|f| f.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec!["- [ ] one 🆔 aaa111", "  - [ ] child", "- [x] two"]
        );
        assert_eq!(found[0].line, 6);
        assert_eq!(found[0].end, 9);
        assert_eq!(
            found[0].annotations,
            vec![
                Annotation {
                    text: "parts are in the garage".into(),
                    date: Some("2026-10-29".into())
                },
                Annotation {
                    text: "Annotation: second".into(),
                    date: None
                },
            ]
        );
        assert_eq!(found[2].line, 13);
    }

    #[test]
    fn due_time_round_trips_and_bad_times_stay_text() {
        let line = "- [ ] Call the bank 📅 2026-11-06 ⏰ 09:00 🆔 abc123";
        let t = rt(line);
        assert_eq!(t.get(FieldKind::DueTime), Some("09:00"));
        assert_eq!(t.description(), "Call the bank");
        for bad in [
            "⏰ 24:00",
            "⏰ 9:00",
            "⏰ 09:60",
            "⏰ 09:000",
            "⏰ 0900",
            "⏰ 09:00am",
        ] {
            let l = format!("- [ ] x {bad}");
            let t = rt(&l);
            assert_eq!(t.get(FieldKind::DueTime), None, "{bad}");
        }
        let mut t = rt("- [ ] Call 📅 2026-11-06 🆔 abc123");
        t.set(FieldKind::DueTime, Some("17:05"));
        assert_eq!(t.print(), "- [ ] Call 📅 2026-11-06 ⏰ 17:05 🆔 abc123");
        assert_eq!(check_time("23:59"), Some((23, 59)));
        assert_eq!(check_time("7:00"), None);
    }
}
