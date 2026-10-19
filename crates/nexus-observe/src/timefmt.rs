//! Tiny RFC 3339 reader (`2026-10-19T19:28:50.471Z`, `+02:00` offsets). Avoids enabling the `time`
//! crate's `parsing` feature for one field.

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse an RFC 3339 timestamp into Unix milliseconds. `None` for anything that does not look like one.
pub fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |a: usize, z: usize| -> Option<i64> { s.get(a..z)?.parse::<i64>().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut i = 19;
    let mut millis = 0i64;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let frac = &s[start..i];
        let three: String = frac.chars().chain("000".chars()).take(3).collect();
        millis = three.parse().ok()?;
    }
    let offset_s = match b.get(i)? {
        b'Z' | b'z' => 0,
        sign @ (b'+' | b'-') => {
            let oh = num(i + 1, i + 3)?;
            let om = num(i + 4, i + 6).unwrap_or(0);
            let v = oh * 3600 + om * 60;
            if *sign == b'+' { v } else { -v }
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo, d);
    Some(((days * 86_400 + h * 3600 + mi * 60 + sec - offset_s) * 1000) + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_zulu_with_millis() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:01.250Z"), Some(1250));
        assert_eq!(
            parse_rfc3339_ms("2026-10-19T19:28:50.471Z"),
            Some(1_792_438_130_471)
        );
    }

    #[test]
    fn parses_offsets_and_missing_fraction() {
        assert_eq!(
            parse_rfc3339_ms("2026-10-19T21:28:50+02:00"),
            Some(1_792_438_130_000)
        );
        assert_eq!(
            parse_rfc3339_ms("2026-10-19T19:28:50.4Z"),
            Some(1_792_438_130_400)
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_rfc3339_ms("yesterday"), None);
        assert_eq!(parse_rfc3339_ms("2026-13-19T19:28:50Z"), None);
        assert_eq!(parse_rfc3339_ms(""), None);
    }
}
