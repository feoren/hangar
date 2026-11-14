//! Calendar arithmetic without a date crate: Unix days to a proleptic Gregorian date and back
//! (H. Hinnant's `civil_from_days` and `days_from_civil`), and the RFC 3339 UTC time stamps agent
//! CLIs write into their transcripts.

/// Proleptic Gregorian date (year, month 1..=12, day 1..=31) of a day count since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // Both are in range by construction (1..=31 and 1..=12).
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

/// Days since 1970-01-01 of a proleptic Gregorian date; the inverse of [`civil_from_days`].
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let month = i64::from(month);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix milliseconds of an RFC 3339 time stamp such as `2026-11-14T10:05:00.123Z` or
/// `2026-11-14T11:05:00+01:00`. `None` for anything else, including impossible dates.
pub fn parse_rfc3339_millis(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't' | b' ')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year = number(0..4)?;
    let month = u32::try_from(number(5..7)?).ok()?;
    let day = u32::try_from(number(8..10)?).ok()?;
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = &text[19..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let shown: String = fraction[..digits]
            .chars()
            .chain("000".chars())
            .take(3)
            .collect();
        millis = shown.parse::<i64>().ok()?;
        rest = &fraction[digits..];
    }
    let offset_minutes = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first() {
                Some(b'+') => 1,
                Some(b'-') => -1,
                _ => return None,
            };
            if rest.len() != 6 || rest.as_bytes()[3] != b':' {
                return None;
            }
            let hours = rest[1..3].parse::<i64>().ok()?;
            let minutes = rest[4..6].parse::<i64>().ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 60 + minutes)
        }
    };
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_minutes * 60;
    Some(seconds * 1000 + millis)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_dates_round_trip() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        for days in [-800_000, -1, 0, 59, 60, 11_016, 20_771, 2_000_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "{days}");
        }
        assert_eq!(days_from_civil(2000, 2, 29), 11_016);
    }

    #[test]
    fn transcript_time_stamps_parse() {
        assert_eq!(parse_rfc3339_millis("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_millis("2026-11-14T10:05:00.123Z"),
            Some(1_794_650_700_123)
        );
        assert_eq!(
            parse_rfc3339_millis("2026-11-14T11:05:00.1+01:00"),
            Some(1_794_650_700_100)
        );
        assert_eq!(
            parse_rfc3339_millis("2026-11-14T10:05:00.123456Z"),
            Some(1_794_650_700_123)
        );
        for bad in [
            "",
            "2026-11-14",
            "2026-13-14T10:05:00Z",
            "2026-02-29T10:05:00Z",
            "2026-11-14T25:05:00Z",
            "2026-11-14T10:05:00",
            "2026-11-14T10:05:00.Z",
            "2026-11-14T10:05:00+0100",
            "20x6-11-14T10:05:00Z",
            "2026-11-14T10:05:00\u{e4}",
        ] {
            assert_eq!(parse_rfc3339_millis(bad), None, "{bad}");
        }
        assert!(parse_rfc3339_millis("2024-02-29T00:00:00Z").is_some());
    }
}
