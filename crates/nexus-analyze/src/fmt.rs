//! Human-readable formatting used by facts and exports. The model never formats numbers itself.

use time::OffsetDateTime;

/// `38.4 GB`, `900 KB`, `12 B` (1024-based, like the rest of the app).
pub fn bytes(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", U[i])
    } else {
        format!("{v:.1} {}", U[i])
    }
}

/// `4,812`
pub fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn dt(ms: i64) -> Option<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)).ok()
}

/// `2026-10-11`
pub fn date(ms: i64) -> String {
    dt(ms).map_or_else(
        || "unknown".into(),
        |t| format!("{:04}-{:02}-{:02}", t.year(), u8::from(t.month()), t.day()),
    )
}

/// `2026-10-11 14:02` (UTC)
pub fn date_time(ms: i64) -> String {
    dt(ms).map_or_else(
        || "unknown".into(),
        |t| {
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}",
                t.year(),
                u8::from(t.month()),
                t.day(),
                t.hour(),
                t.minute()
            )
        },
    )
}

/// `55%`
pub fn pct(part: u64, whole: u64) -> String {
    if whole == 0 {
        "0%".into()
    } else {
        format!("{:.0}%", part as f64 * 100.0 / whole as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(900), "900 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(41_231_686_041), "38.4 GB");
        assert_eq!(bytes(150 * 1024 * 1024), "150 MB");
        assert_eq!(count(4812), "4,812");
        assert_eq!(count(7), "7");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(date(1_791_000_000_000), "2026-10-03");
        assert_eq!(pct(55, 100), "55%");
        assert_eq!(pct(1, 0), "0%");
        assert_eq!(date_time(0), "1970-01-01 00:00");
    }
}
