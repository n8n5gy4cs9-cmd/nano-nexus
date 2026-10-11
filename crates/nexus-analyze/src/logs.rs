//! Log parsing and template clustering (M08 design notes): levels, a time range, the most common message
//! templates (numbers, hex, UUIDs, IPs, paths, and quoted strings masked), and a timeline with spikes.

use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use time::{Date, Month, PrimitiveDateTime, Time};

const MAX_TEMPLATES: usize = 20;
/// Distinct templates kept while counting (a safety valve for logs where every line is unique).
const MAX_DISTINCT: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct LogLine {
    pub line_no: u64,
    pub level: String,
    pub text: String,
    pub ts: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Template {
    pub template: String,
    pub count: u64,
    pub level: String,
    pub example: String,
    pub first_line: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Bucket {
    /// Unix milliseconds at the start of the bucket.
    pub start: i64,
    pub count: u64,
    pub errors: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct LogReport {
    pub lines: u64,
    pub first_ts: Option<i64>,
    pub last_ts: Option<i64>,
    /// `FATAL, ERROR, WARN, INFO, DEBUG, TRACE, OTHER` → count.
    pub levels: BTreeMap<String, u64>,
    pub templates: Vec<Template>,
    pub first_error: Option<LogLine>,
    pub last_error: Option<LogLine>,
    /// Bucket size in milliseconds (a minute, an hour, or a day).
    pub bucket_ms: i64,
    pub timeline: Vec<Bucket>,
    pub spikes: Vec<Bucket>,
}

struct Patterns {
    level: Regex,
    ts_iso: Regex,
    ts_clf: Regex,
    uuid: Regex,
    ip: Regex,
    hex: Regex,
    quoted: Regex,
    path: Regex,
    num: Regex,
}

fn pats() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let re = |s: &str| Regex::new(s).unwrap_or_else(|_| Regex::new("$^").unwrap_or_else(|_| unreachable!()));
        Patterns {
            level: re(r"(?i)(?:^|[\s\[(<|:])(fatal|critical|error|err|warn(?:ing)?|info|debug|trace)(?:$|[\s\]):>|])"),
            ts_iso: re(r"(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})(?:[.,](\d{1,9}))?\s*(Z|[+-]\d{2}:?\d{2})?"),
            ts_clf: re(r"(\d{1,2})/([A-Za-z]{3})/(\d{4}):(\d{2}):(\d{2}):(\d{2})(?: ([+-]\d{4}))?"),
            uuid: re(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b"),
            ip: re(r"\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?\b"),
            hex: re(r"\b(?:0x)?[0-9a-fA-F]{8,}\b"),
            quoted: re(r#""[^"\n]*"|'[^'\n]*'"#),
            path: re(r"(?:[A-Za-z]:\\|/)[\w.\-]+(?:[/\\][\w.\-]+)+"),
            num: re(r"\b\d+(?:\.\d+)?\b"),
        }
    })
}

fn month(s: &str) -> Option<Month> {
    Some(match s.to_ascii_lowercase().as_str() {
        "jan" => Month::January,
        "feb" => Month::February,
        "mar" => Month::March,
        "apr" => Month::April,
        "may" => Month::May,
        "jun" => Month::June,
        "jul" => Month::July,
        "aug" => Month::August,
        "sep" => Month::September,
        "oct" => Month::October,
        "nov" => Month::November,
        "dec" => Month::December,
        _ => return None,
    })
}

fn to_ms(date: Date, h: u8, mi: u8, s: u8, nanos: u32, offset_min: i32) -> Option<i64> {
    let t = Time::from_hms_nano(h, mi, s, nanos).ok()?;
    let dt = PrimitiveDateTime::new(date, t).assume_utc();
    Some(dt.unix_timestamp() * 1000 + i64::from(nanos / 1_000_000) - i64::from(offset_min) * 60_000)
}

fn offset_minutes(tz: &str) -> i32 {
    if tz == "Z" || tz.is_empty() {
        return 0;
    }
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let d: String = tz.chars().filter(char::is_ascii_digit).collect();
    if d.len() != 4 {
        return 0;
    }
    sign * (d[..2].parse::<i32>().unwrap_or(0) * 60 + d[2..].parse::<i32>().unwrap_or(0))
}

/// Parse the first timestamp in `line` (ISO 8601 or Apache/nginx style). Returns its text span too.
pub fn parse_ts(line: &str) -> Option<(i64, std::ops::Range<usize>)> {
    let p = pats();
    if let Some(c) = p.ts_iso.captures(line) {
        let g = |i: usize| c.get(i).map_or("", |m| m.as_str());
        let date = Date::from_calendar_date(
            g(1).parse().ok()?,
            Month::try_from(g(2).parse::<u8>().ok()?).ok()?,
            g(3).parse().ok()?,
        )
        .ok()?;
        let frac = g(7);
        let nanos = if frac.is_empty() {
            0
        } else {
            format!("{frac:0<9}")[..9].parse::<u32>().ok()?
        };
        let ms = to_ms(
            date,
            g(4).parse().ok()?,
            g(5).parse().ok()?,
            g(6).parse().ok()?,
            nanos,
            offset_minutes(g(8)),
        )?;
        return Some((ms, c.get(0)?.range()));
    }
    if let Some(c) = p.ts_clf.captures(line) {
        let g = |i: usize| c.get(i).map_or("", |m| m.as_str());
        let date = Date::from_calendar_date(g(3).parse().ok()?, month(g(2))?, g(1).parse().ok()?).ok()?;
        let ms = to_ms(
            date,
            g(4).parse().ok()?,
            g(5).parse().ok()?,
            g(6).parse().ok()?,
            0,
            offset_minutes(g(7)),
        )?;
        return Some((ms, c.get(0)?.range()));
    }
    None
}

/// `FATAL | ERROR | WARN | INFO | DEBUG | TRACE`, or `OTHER`.
pub fn detect_level(line: &str) -> &'static str {
    if let Some(c) = pats().level.captures(line) {
        return match c[1].to_ascii_lowercase().as_str() {
            "fatal" | "critical" => "FATAL",
            "error" | "err" => "ERROR",
            "warn" | "warning" => "WARN",
            "info" => "INFO",
            "debug" => "DEBUG",
            _ => "TRACE",
        };
    }
    if line.contains("panic") || line.contains("Exception") || line.starts_with("Traceback") {
        return "ERROR";
    }
    "OTHER"
}

/// Mask the parts of a message that vary between occurrences.
pub fn template_of(message: &str) -> String {
    let p = pats();
    let s = p.quoted.replace_all(message, "<STR>");
    let s = p.uuid.replace_all(&s, "<UUID>");
    let s = p.ip.replace_all(&s, "<IP>");
    let s = p.path.replace_all(&s, "<PATH>");
    let s = p.hex.replace_all(&s, "<HEX>");
    let s = p.num.replace_all(&s, "<N>");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Stack frames and wrapped lines belong to the entry above them.
fn is_continuation(line: &str, level: &str) -> bool {
    line.starts_with("at ")
        || line.starts_with("Caused by")
        || line.starts_with("Traceback")
        || line.starts_with("...")
        || line.starts_with("File \"")
        || (line.starts_with(char::is_whitespace) && level == "OTHER")
}

/// Strip the timestamp and level token so equal messages cluster together.
fn message_of(line: &str, ts: Option<&std::ops::Range<usize>>) -> String {
    let mut s = match ts {
        Some(r) => format!("{}{}", &line[..r.start], &line[r.end..]),
        None => line.to_owned(),
    };
    if let Some(m) = pats().level.captures(&s).and_then(|c| c.get(1)) {
        s.replace_range(m.range(), "");
    }
    s.trim_matches(|c: char| c.is_whitespace() || "[]|:-<>".contains(c))
        .to_owned()
}

/// Analyze a log. Reads line by line; memory stays bounded for huge files.
pub fn analyze<R: BufRead>(reader: R) -> LogReport {
    let mut levels: BTreeMap<String, u64> = BTreeMap::new();
    let mut templates: HashMap<(String, &'static str), Template> = HashMap::new();
    let mut lines = 0u64;
    let (mut first_ts, mut last_ts): (Option<i64>, Option<i64>) = (None, None);
    let (mut first_error, mut last_error): (Option<LogLine>, Option<LogLine>) = (None, None);
    let mut stamps: Vec<(i64, bool)> = Vec::new();
    for (i, raw) in reader.lines().map_while(Result::ok).enumerate() {
        lines += 1;
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        let ts = parse_ts(line);
        let level = detect_level(line);
        let continuation = ts.is_none() && is_continuation(line, level);
        if continuation {
            // Stack frames belong to the entry above; they do not count as separate messages.
            continue;
        }
        *levels.entry(level.into()).or_default() += 1;
        let ts_ms = ts.as_ref().map(|t| t.0);
        if let Some(t) = ts_ms {
            first_ts = Some(first_ts.map_or(t, |f| f.min(t)));
            last_ts = Some(last_ts.map_or(t, |l| l.max(t)));
            stamps.push((t, matches!(level, "ERROR" | "FATAL")));
        }
        let is_error = matches!(level, "ERROR" | "FATAL");
        let entry = || LogLine {
            line_no: i as u64 + 1,
            level: level.into(),
            text: line.chars().take(300).collect(),
            ts: ts_ms,
        };
        if is_error {
            if first_error.is_none() {
                first_error = Some(entry());
            }
            last_error = Some(entry());
        }
        let msg = message_of(line, ts.as_ref().map(|t| &t.1));
        let tpl = template_of(&msg);
        if tpl.is_empty() {
            continue;
        }
        let key = (tpl.clone(), level);
        if templates.len() >= MAX_DISTINCT && !templates.contains_key(&key) {
            continue;
        }
        templates
            .entry(key)
            .and_modify(|t| t.count += 1)
            .or_insert_with(|| Template {
                template: tpl,
                count: 1,
                level: level.into(),
                example: line.chars().take(200).collect(),
                first_line: i as u64 + 1,
            });
    }
    let mut templates: Vec<Template> = templates.into_values().collect();
    templates.sort_by(|a, b| b.count.cmp(&a.count).then(a.first_line.cmp(&b.first_line)));
    templates.truncate(MAX_TEMPLATES);

    // Timeline: a minute, an hour, or a day per bucket depending on the span.
    let span = match (first_ts, last_ts) {
        (Some(a), Some(b)) => b - a,
        _ => 0,
    };
    let bucket_ms: i64 = if span <= 3 * 3_600_000 {
        60_000
    } else if span <= 7 * 86_400_000 {
        3_600_000
    } else {
        86_400_000
    };
    let mut buckets: BTreeMap<i64, (u64, u64)> = BTreeMap::new();
    for (t, err) in &stamps {
        let e = buckets.entry(t.div_euclid(bucket_ms) * bucket_ms).or_default();
        e.0 += 1;
        e.1 += u64::from(*err);
    }
    let timeline: Vec<Bucket> = buckets
        .into_iter()
        .map(|(start, (count, errors))| Bucket { start, count, errors })
        .collect();
    let spikes = find_spikes(&timeline);
    LogReport {
        lines,
        first_ts,
        last_ts,
        levels,
        templates,
        first_error,
        last_error,
        bucket_ms,
        timeline,
        spikes,
    }
}

/// Buckets whose count is more than two standard deviations above the mean (and at least 5).
fn find_spikes(t: &[Bucket]) -> Vec<Bucket> {
    if t.len() < 4 {
        return Vec::new();
    }
    let n = t.len() as f64;
    let mean = t.iter().map(|b| b.count as f64).sum::<f64>() / n;
    let var = t.iter().map(|b| (b.count as f64 - mean).powi(2)).sum::<f64>() / n;
    let limit = mean + 2.0 * var.sqrt();
    let mut s: Vec<Bucket> = t
        .iter()
        .filter(|b| b.count as f64 > limit && b.count >= 5)
        .cloned()
        .collect();
    s.sort_by_key(|b| std::cmp::Reverse(b.count));
    s.truncate(5);
    s
}

/// Facts for `log_triage.md`: time range, level counts, top templates, first error, and spikes.
pub fn facts(r: &LogReport) -> String {
    use crate::fmt::{count, date_time};
    let mut o = String::from("<facts>\n");
    match (r.first_ts, r.last_ts) {
        (Some(a), Some(b)) => o.push_str(&format!(
            "time range: {} to {} (UTC)   lines: {}\n",
            date_time(a),
            date_time(b),
            count(r.lines)
        )),
        _ => o.push_str(&format!("lines: {}   (no timestamps found)\n", count(r.lines))),
    }
    let lv: Vec<String> = ["FATAL", "ERROR", "WARN", "INFO", "DEBUG", "TRACE", "OTHER"]
        .iter()
        .filter_map(|k| {
            r.levels
                .get(*k)
                .filter(|n| **n > 0)
                .map(|n| format!("{k} {}", count(*n)))
        })
        .collect();
    o.push_str(&format!("levels: {}\n", lv.join(" · ")));
    o.push_str("top message templates:\n");
    for (i, t) in r.templates.iter().take(10).enumerate() {
        o.push_str(&format!(
            "{}) [{}] ×{} {}\n   example: {}\n",
            i + 1,
            t.level,
            count(t.count),
            t.template,
            t.example
        ));
    }
    if let Some(e) = &r.first_error {
        o.push_str(&format!("first error (line {}): {}\n", e.line_no, e.text));
    }
    if let Some(e) = r
        .last_error
        .as_ref()
        .filter(|e| r.first_error.as_ref().map(|f| f.line_no) != Some(e.line_no))
    {
        o.push_str(&format!("last error (line {}): {}\n", e.line_no, e.text));
    }
    if !r.spikes.is_empty() {
        let s: Vec<String> = r
            .spikes
            .iter()
            .map(|b| format!("{} ({} lines, {} errors)", date_time(b.start), b.count, b.errors))
            .collect();
        o.push_str(&format!("spikes: {}\n", s.join(" · ")));
    }
    o.push_str("</facts>");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> LogReport {
        analyze(std::io::Cursor::new(text.to_owned()))
    }

    #[test]
    fn timestamps() {
        let (ms, r) = parse_ts("2026-10-11T14:02:33.250Z ERROR boom").unwrap();
        assert_eq!(ms, 1_791_727_353_250);
        assert_eq!(r.start, 0);
        let (a, _) = parse_ts("2026-10-11 14:02:33").unwrap();
        let (b, _) = parse_ts("2026-10-11T16:02:33+02:00").unwrap();
        assert_eq!(a, b);
        let (c, _) = parse_ts("127.0.0.1 - - [11/Oct/2026:14:02:33 +0000] \"GET / HTTP/1.1\" 200").unwrap();
        assert_eq!(c, a);
        assert!(parse_ts("no time here").is_none());
        assert!(parse_ts("2026-13-45 99:99:99").is_none());
    }

    #[test]
    fn levels() {
        for (l, want) in [
            ("2026-01-01 00:00:00 ERROR failed", "ERROR"),
            ("[warn] disk almost full", "WARN"),
            ("INFO: started", "INFO"),
            ("level=debug msg=hi", "OTHER"),
            ("thread 'main' panicked at src/main.rs", "ERROR"),
            ("java.lang.NullPointerException: x", "ERROR"),
            ("FATAL out of memory", "FATAL"),
            ("information about errors", "OTHER"),
        ] {
            assert_eq!(detect_level(l), want, "{l}");
        }
    }

    #[test]
    fn templates_mask_variable_parts() {
        let a = template_of("User 4812 logged in from 10.0.0.7 session 3f2b9c1a-0d4e-4c5b-8a6f-1234567890ab");
        let b =
            template_of("User 77 logged in from 192.168.1.20 session aaaaaaaa-0000-4000-8000-bbbbbbbbbbbb");
        assert_eq!(a, b);
        assert_eq!(a, "User <N> logged in from <IP> session <UUID>");
        assert_eq!(
            template_of("open '/var/log/app.log' failed: code 13"),
            "open <STR> failed: code <N>"
        );
        assert_eq!(template_of("read /var/log/app.log"), "read <PATH>");
        assert_eq!(template_of("hash deadbeef1234 ok"), "hash <HEX> ok");
    }

    #[test]
    fn clustering_counts_and_ranks() {
        let mut log = String::new();
        for i in 0..50 {
            log.push_str(&format!(
                "2026-10-11 14:00:{:02} INFO request {i} served in {} ms\n",
                i % 60,
                10 + i
            ));
        }
        for i in 0..8 {
            log.push_str(&format!(
                "2026-10-11 14:01:{:02} ERROR db connection {i} refused\n",
                i
            ));
        }
        log.push_str("2026-10-11 14:02:00 WARN cache miss\n");
        let r = run(&log);
        assert_eq!(r.lines, 59);
        assert_eq!(r.levels["INFO"], 50);
        assert_eq!(r.levels["ERROR"], 8);
        assert_eq!(r.templates[0].count, 50);
        assert_eq!(r.templates[0].template, "request <N> served in <N> ms");
        assert_eq!(r.templates[1].template, "db connection <N> refused");
        assert_eq!(r.first_error.as_ref().unwrap().line_no, 51);
        assert_eq!(r.last_error.as_ref().unwrap().line_no, 58);
        assert_eq!(
            (r.first_ts.unwrap(), r.last_ts.unwrap()),
            (1_791_727_200_000, 1_791_727_320_000)
        );
        assert_eq!(r.bucket_ms, 60_000);
        assert_eq!(r.timeline.len(), 3);
        assert_eq!(r.timeline[1].errors, 8);
    }

    #[test]
    fn stack_traces_do_not_create_templates() {
        let log = "2026-10-11 14:00:00 ERROR Unhandled exception\n  at app.Main(Main.java:10)\n  at app.Run(Run.java:20)\nCaused by: java.io.IOException\n2026-10-11 14:00:01 INFO ok\n";
        let r = run(log);
        assert_eq!(r.levels.get("ERROR"), Some(&1));
        assert!(
            r.templates.iter().all(|t| !t.template.contains("Main.java")),
            "{:?}",
            r.templates
        );
        assert_eq!(r.templates.len(), 2);
    }

    #[test]
    fn spikes_are_found() {
        let mut log = String::new();
        for minute in 0..30 {
            let n = if minute == 17 { 60 } else { 3 };
            for s in 0..n {
                log.push_str(&format!("2026-10-11 14:{minute:02}:{:02} INFO tick\n", s % 60));
            }
        }
        let r = run(&log);
        assert_eq!(r.spikes.len(), 1);
        assert_eq!(r.spikes[0].count, 60);
    }

    #[test]
    fn timeline_scales_with_the_span_and_missing_timestamps_are_fine() {
        let r = run("2026-10-01 00:00:00 INFO a\n2026-10-03 00:00:00 INFO b\n");
        assert_eq!(r.bucket_ms, 3_600_000);
        let r = run("2026-01-01 00:00:00 INFO a\n2026-10-03 00:00:00 INFO b\n");
        assert_eq!(r.bucket_ms, 86_400_000);
        let r = run("just words\nmore words\n");
        assert_eq!((r.first_ts, r.timeline.len(), r.lines), (None, 0, 2));
        assert_eq!(run("").lines, 0);
    }

    #[test]
    fn facts_are_compact_and_complete() {
        let mut log = String::new();
        for i in 0..40 {
            log.push_str(&format!(
                "2026-10-11 14:00:{:02} ERROR payment {i} declined for order {}\n",
                i % 60,
                1000 + i
            ));
        }
        let f = facts(&run(&log));
        assert!(
            f.contains("levels: ERROR 40")
                && f.contains("×40 payment <N> declined for order <N>")
                && f.contains("first error (line 1)"),
            "{f}"
        );
        assert!(f.len() < 4000);
    }
}
