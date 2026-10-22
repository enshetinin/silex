//! Page history (ADR 0009), without SQL or egui: how a version's blocks
//! are written as JSON and read back, and which old versions to drop.
//!
//! A version is a whole snapshot of a page (its title and blocks). The
//! database saves one when a page is saved and its last version is older
//! than [`MIN_GAP_MINUTES`], and thins them out with [`to_prune`].
use serde_json::{Map, Value, json};

use crate::model::{Block, BlockKind, MAX_INDENT};

/// Newest format of `page_versions.blocks` this version of Silex reads:
/// 1 is plain blocks, 2 adds inline formatting (`marks`, ADR 0015). A
/// version is written as 1 when none of its blocks has formatting, so
/// pages without it keep the same versions as before.
pub const FORMAT: i64 = 2;
/// At most one version per page in this many minutes of editing.
pub const MIN_GAP_MINUTES: i64 = 10;
/// Every version younger than this (in days) is kept.
const KEEP_ALL_DAYS: f64 = 1.0;
/// Up to this age (in days), the last version of each day is kept; after
/// it, the last of each week.
const KEEP_DAILY_DAYS: f64 = 30.0;
/// Julian day of 1970-01-01T00:00Z.
const UNIX_EPOCH_JD: f64 = 2_440_587.5;

/// A version as listed, without its blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub id: i64,
    pub page_id: String,
    /// UTC, `YYYY-MM-DDTHH:MM:SS.sssZ`.
    pub created_at: String,
    pub title: String,
    /// Bytes of its blocks' JSON.
    pub size: u64,
    /// In the system's time zone, `YYYY-MM-DD HH:MM`.
    pub local: String,
}

/// Short Spanish month names.
pub(crate) const MONTHS: [&str; 12] = [
    "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
];

/// When a version was saved, as the history lists it: «Hoy, 17:40»,
/// «Ayer, 09:05», «18 oct, 17:40», or «18 oct 2025, 17:40» in another
/// year. `local` is `YYYY-MM-DD HH:MM` and `today` `YYYY-MM-DD`, both in
/// local time; anything else is shown as it is.
pub fn describe_when(local: &str, today: &str) -> String {
    let parse = |date: &str| -> Option<(i64, u32, u32)> {
        let mut parts = date.splitn(3, '-');
        let year = parts.next()?.parse().ok()?;
        let month = parts
            .next()?
            .parse()
            .ok()
            .filter(|m| (1..=12).contains(m))?;
        let day = parts
            .next()?
            .parse()
            .ok()
            .filter(|d| (1..=31).contains(d))?;
        Some((year, month, day))
    };
    let (Some(date), Some(time)) = (local.get(..10), local.get(11..16)) else {
        return local.to_owned();
    };
    let (Some(at), Some(now)) = (parse(date), parse(today)) else {
        return local.to_owned();
    };
    let days = days_from_civil(now) - days_from_civil(at);
    let (year, month, day) = at;
    let name = MONTHS[(month - 1) as usize];
    match days {
        0 => format!("Hoy, {time}"),
        1 => format!("Ayer, {time}"),
        _ if year == now.0 => format!("{day} {name}, {time}"),
        _ => format!("{day} {name} {year}, {time}"),
    }
}

/// Days since 1970-01-01 of a calendar date (proleptic Gregorian).
fn days_from_civil((year, month, day): (i64, u32, u32)) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// A version's blocks, or why they cannot be shown (a newer Silex, or
/// damaged data): one unreadable version never hides the others.
pub type VersionBlocks = Result<Vec<Block>, String>;

/// The format [`blocks_to_json`] writes for `blocks`: 2 if any of them
/// has inline formatting, 1 otherwise.
pub fn format_of(blocks: &[Block]) -> i64 {
    if blocks.iter().any(|b| !b.marks.is_empty()) {
        2
    } else {
        1
    }
}

/// The blocks as stored in a version: a JSON list with `id`, `kind`,
/// `content`, `checked`, `indent` and, when they have one, `attachment`,
/// `link` and `marks` (format 2). Positions are the list order.
pub fn blocks_to_json(blocks: &[Block]) -> String {
    let list: Vec<Value> = blocks
        .iter()
        .map(|b| {
            let mut item = json!({
                "id": b.id,
                "kind": b.kind.as_str(),
                "content": b.content,
                "checked": b.checked,
                "indent": b.indent,
            });
            if let Value::Object(map) = &mut item {
                if let Some(attachment) = &b.attachment {
                    map.insert("attachment".into(), json!(attachment));
                }
                if let Some(link) = &b.link {
                    map.insert("link".into(), json!(link));
                }
                if !b.marks.is_empty() {
                    map.insert("marks".into(), crate::marks::to_value(&b.marks));
                }
            }
            item
        })
        .collect();
    Value::Array(list).to_string()
}

/// Reads a version's blocks (of `page_id`). Unknown fields are ignored;
/// an unknown format or kind, a missing id or a file block without its
/// file make the version unreadable, with a message for the user.
/// Formatting that does not fit a block's text is dropped, never the text.
pub fn blocks_from_json(format: i64, json: &str, page_id: &str) -> VersionBlocks {
    const UNREADABLE: &str = "Esta versión no se puede leer";
    if !(1..=FORMAT).contains(&format) {
        return Err(format!(
            "{UNREADABLE}: es de una versión más reciente de Silex"
        ));
    }
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else {
        return Err(format!("{UNREADABLE}: los datos están dañados"));
    };
    let damaged = || format!("{UNREADABLE}: los datos están dañados");
    let mut blocks = Vec::with_capacity(items.len());
    for (position, item) in items.iter().enumerate() {
        let Value::Object(map) = item else {
            return Err(damaged());
        };
        let text = |key: &str| map.get(key).and_then(Value::as_str).map(str::to_owned);
        let id = text("id").filter(|id| !id.is_empty()).ok_or_else(damaged)?;
        let kind_name = text("kind").ok_or_else(damaged)?;
        let Some(kind) = BlockKind::parse(&kind_name) else {
            return Err(format!(
                "{UNREADABLE}: tiene bloques de una versión más reciente de Silex"
            ));
        };
        let attachment = text("attachment").filter(|_| kind == BlockKind::File);
        if kind == BlockKind::File && attachment.is_none() {
            return Err(damaged());
        }
        let content = text("content").unwrap_or_default();
        let marks = map
            .get("marks")
            .filter(|_| kind.takes_marks())
            .and_then(|marks| crate::marks::from_value(marks, content.chars().count()).ok())
            .unwrap_or_default();
        blocks.push(Block {
            id,
            page_id: page_id.to_owned(),
            kind,
            content,
            checked: map.get("checked").and_then(Value::as_bool).unwrap_or(false),
            position: position as i64,
            indent: indent(map),
            attachment,
            link: text("link").filter(|_| kind == BlockKind::PageLink),
            marks,
        });
    }
    Ok(blocks)
}

/// The indent, within range (a bad value is 0, not an error).
fn indent(map: &Map<String, Value>) -> u8 {
    map.get("indent")
        .and_then(Value::as_u64)
        .and_then(|n| u8::try_from(n).ok())
        .filter(|n| *n <= MAX_INDENT)
        .unwrap_or(0)
}

/// Which versions of one page to drop, given `(id, julian day)` of each
/// and the time now (julian day, UTC). Keeps every version of the last
/// day, the last one of each (UTC) day up to 30 days, and the last one of
/// each week (Monday to Sunday) after that. Ties go to the higher id.
pub fn to_prune(versions: &[(i64, f64)], now: f64) -> Vec<i64> {
    use std::collections::HashMap;
    // Bucket → the version kept so far (julian day, id).
    let mut kept: HashMap<(u8, i64), (f64, i64)> = HashMap::new();
    let mut drop = Vec::new();
    for &(id, at) in versions {
        let age = now - at;
        if age < KEEP_ALL_DAYS {
            continue;
        }
        let day = (at - UNIX_EPOCH_JD).floor() as i64;
        // 1970-01-01 was a Thursday: +3 starts weeks on Monday.
        let bucket = if age < KEEP_DAILY_DAYS {
            (0, day)
        } else {
            (1, (day + 3).div_euclid(7))
        };
        match kept.get(&bucket) {
            Some(&(best_at, best_id)) if (best_at, best_id) >= (at, id) => drop.push(id),
            Some(&(_, best_id)) => {
                drop.push(best_id);
                kept.insert(bucket, (at, id));
            }
            None => {
                kept.insert(bucket, (at, id));
            }
        }
    }
    drop.sort_unstable();
    drop
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, kind: BlockKind) -> Block {
        Block {
            id: id.into(),
            page_id: "p".into(),
            kind,
            content: format!("texto «{id}» con \"comillas\"\n y saltos"),
            checked: false,
            position: 0,
            indent: 0,
            attachment: None,
            link: None,
            marks: Vec::new(),
        }
    }

    #[test]
    fn blocks_round_trip_with_files_links_checks_and_indent() {
        let mut todo = block("a", BlockKind::Todo);
        todo.checked = true;
        todo.indent = 2;
        let mut file = block("b", BlockKind::File);
        file.attachment = Some("adj".into());
        let mut link = block("c", BlockKind::PageLink);
        link.link = Some("destino".into());
        let mut gone = block("d", BlockKind::PageLink);
        gone.link = None;
        let blocks: Vec<Block> = [todo, file, link, gone]
            .into_iter()
            .enumerate()
            .map(|(i, b)| Block {
                position: i as i64,
                ..b
            })
            .collect();
        let json = blocks_to_json(&blocks);
        assert_eq!(format_of(&blocks), 1);
        assert!(!json.contains("marks"));
        assert_eq!(blocks_from_json(1, &json, "p"), Ok(blocks));
        assert_eq!(blocks_from_json(FORMAT, "[]", "p"), Ok(vec![]));
    }

    #[test]
    fn formatting_makes_a_format_2_version_and_comes_back() {
        use crate::marks::{Mark, MarkKind};
        let mut bold = block("a", BlockKind::Paragraph);
        bold.marks = vec![
            Mark::new(0, 5, MarkKind::Bold),
            Mark::new(6, 9, MarkKind::Link("https://ejemplo.com".into())),
        ];
        let plain = Block {
            position: 1,
            ..block("b", BlockKind::Quote)
        };
        let blocks = vec![bold, plain];
        assert_eq!(format_of(&blocks), 2);
        let json = blocks_to_json(&blocks);
        assert_eq!(blocks_from_json(2, &json, "p"), Ok(blocks));
        // Damaged formatting is dropped; the block and its text stay.
        let read = blocks_from_json(
            2,
            r#"[{"id":"a","kind":"paragraph","content":"hola","marks":[{"start":0,"end":99,"type":"bold"}]},
                {"id":"b","kind":"code","content":"x","marks":[{"start":0,"end":1,"type":"bold"}]}]"#,
            "p",
        )
        .expect("readable");
        assert_eq!(read[0].content, "hola");
        assert!(read[0].marks.is_empty());
        assert!(read[1].marks.is_empty(), "code takes no formatting");
    }

    #[test]
    fn unreadable_versions_say_why_and_never_panic() {
        let err =
            |format: i64, json: &str| blocks_from_json(format, json, "p").expect_err("unreadable");
        assert!(err(3, "[]").contains("más reciente"));
        assert!(err(0, "[]").contains("más reciente"));
        assert!(err(FORMAT, r#"[{"id":"a","kind":"mapa"}]"#).contains("más reciente"));
        for damaged in [
            "no es json",
            "{}",
            "[1]",
            r#"[{"kind":"paragraph"}]"#,
            r#"[{"id":"","kind":"paragraph"}]"#,
            r#"[{"id":"a"}]"#,
            r#"[{"id":"a","kind":"file"}]"#,
        ] {
            assert!(err(FORMAT, damaged).contains("dañados"), "{damaged}");
        }
        // Unknown fields are ignored; bad values fall back.
        let blocks = blocks_from_json(
            FORMAT,
            r#"[{"id":"a","kind":"paragraph","indent":99,"checked":"sí","color":"rojo",
                 "attachment":"x","link":"y"}]"#,
            "p",
        )
        .expect("readable");
        assert_eq!(blocks[0].indent, 0);
        assert!(!blocks[0].checked);
        assert_eq!(
            (blocks[0].attachment.clone(), blocks[0].link.clone()),
            (None, None)
        );
        assert_eq!(blocks[0].content, "");
    }

    #[test]
    fn versions_say_when_in_local_words() {
        let today = "2026-10-20";
        assert_eq!(describe_when("2026-10-20 17:40", today), "Hoy, 17:40");
        assert_eq!(describe_when("2026-10-19 09:05", today), "Ayer, 09:05");
        assert_eq!(describe_when("2026-10-18 23:59", today), "18 oct, 23:59");
        assert_eq!(describe_when("2026-01-02 08:00", today), "2 ene, 08:00");
        assert_eq!(
            describe_when("2025-12-31 22:00", "2026-01-01"),
            "Ayer, 22:00"
        );
        assert_eq!(
            describe_when("2025-09-07 10:00", today),
            "7 sept 2025, 10:00"
        );
        // Across a leap day.
        assert_eq!(
            describe_when("2028-02-28 10:00", "2028-03-01"),
            "28 feb, 10:00"
        );
        assert_eq!(
            describe_when("2028-02-29 10:00", "2028-03-01"),
            "Ayer, 10:00"
        );
        // What cannot be read is shown as it is.
        assert_eq!(describe_when("ayer", today), "ayer");
        assert_eq!(describe_when("2026-13-01 10:00", today), "2026-13-01 10:00");
        assert_eq!(days_from_civil((1970, 1, 1)), 0);
        assert_eq!(days_from_civil((2026, 10, 20)), 20_746);
    }

    /// 2026-10-20T12:00Z as a julian day.
    const NOW: f64 = 2_461_334.0;

    #[test]
    fn the_last_day_keeps_everything() {
        let versions: Vec<(i64, f64)> = (0..144).map(|i| (i, NOW - i as f64 / 144.0)).collect();
        assert!(to_prune(&versions, NOW).is_empty());
        // A version from the future (a clock change) is kept too.
        assert!(to_prune(&[(1, NOW + 3.0)], NOW).is_empty());
    }

    #[test]
    fn up_to_a_month_keeps_the_last_of_each_day() {
        // 2026-10-15 (5 days ago), at 08:00, 12:00 and 20:00 UTC.
        let day = NOW - 5.0;
        let versions = [(1, day - 4.0 / 24.0), (3, day), (2, day + 8.0 / 24.0)];
        assert_eq!(to_prune(&versions, NOW), [1, 3]);
        // 23:59 and 00:01 are different days.
        let midnight = day + 0.5;
        let versions = [(1, midnight - 0.001), (2, midnight + 0.001)];
        assert!(to_prune(&versions, NOW).is_empty());
        // Same instant: the higher id stays.
        assert_eq!(to_prune(&[(7, day), (8, day)], NOW), [7]);
    }

    #[test]
    fn older_than_a_month_keeps_the_last_of_each_week() {
        // 2026-08-31 is a Monday (NOW - 50 days, at 12:00).
        let monday = NOW - 50.0;
        let week: Vec<(i64, f64)> = (0..7).map(|d| (d, monday + d as f64)).collect();
        // Monday to Sunday: only Sunday stays.
        assert_eq!(to_prune(&week, NOW), [0, 1, 2, 3, 4, 5]);
        // The next Monday starts another week.
        let two = [(1, monday + 6.0), (2, monday + 7.0)];
        assert!(to_prune(&two, NOW).is_empty());
    }

    #[test]
    fn a_year_of_daily_edits_shrinks_to_about_eighty() {
        // One version every 10 minutes, 8 hours a day, for a year.
        let mut versions = Vec::new();
        let mut id = 0;
        for day in 0..365 {
            for slot in 0..48 {
                id += 1;
                let at = NOW - day as f64 - 0.4 + slot as f64 / 144.0;
                versions.push((id, at));
            }
        }
        let dropped = to_prune(&versions, NOW).len();
        let kept = versions.len() - dropped;
        // ~48 of the last day, ~29 days, ~48 weeks.
        assert!((110..=135).contains(&kept), "{kept}");
    }
}
