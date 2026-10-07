//! The `stale-annotations` cutoff: one instant, given exactly one way.
//!
//! `last_verified` is Unix seconds, so the cutoff is too — but a human asking "what needs
//! re-verification?" thinks in dates and windows (#205). Three spellings resolve to the same
//! instant, and exactly one may be given:
//!
//! * `<unix-seconds>` — the raw clock, e.g. `1767225600`.
//! * `<YYYY-MM-DD>` — that date at 00:00:00 **UTC**, so "stale as of 2026-01-01" means
//!   `last_verified < 2026-01-01T00:00:00Z` on every machine regardless of local time zone.
//! * `--older-than <N><unit>` — `now - N·unit`, unit one of `s m h d w`: "not verified in the
//!   last 90 days" is `--older-than 90d`. The unit is required; a bare `90` is refused rather than
//!   guessed as seconds or days.
//!
//! [`parse_verified`] reads the same instant spellings (plus `now`) for `annotate --last-verified`,
//! so the clock a row is written with and the clock it is judged by are spelled identically.
//!
//! No date crate: the only calendar arithmetic needed is proleptic-Gregorian days-from-civil
//! (Howard Hinnant's algorithm), plus its inverse to echo the resolved instant in human output.

const DAY: i64 = 86_400;

/// Resolve the cutoff from the command's operands (non-flag argv) and `--older-than`. `now` is
/// injected so the window form is testable. The error is a reason line; the caller adds usage.
pub fn resolve(operands: &[&str], older_than: Option<&str>, now: i64) -> Result<i64, String> {
    match (operands, older_than) {
        ([], None) => Err("a cutoff is required".into()),
        ([_, ..], Some(_)) => Err("give a <cutoff> operand or --older-than, not both".into()),
        ([], Some(window)) => {
            let secs = parse_window(window)?;
            now.checked_sub(secs)
                .ok_or_else(|| format!("--older-than {window:?} is out of range"))
        }
        ([one], None) => parse_instant(one),
        (many, None) => Err(format!(
            "expected one <cutoff> operand, got {}: {many:?}",
            many.len()
        )),
    }
}

/// `annotate --last-verified`: `now`, `<unix-seconds>` or `<YYYY-MM-DD>` (UTC midnight). `0` is
/// the explicit "never verified"; a negative instant is refused — no fact was verified before 1970,
/// and a negative value would sort as never-verified-but-older on every freshness read.
pub fn parse_verified(s: &str, now: i64) -> Result<i64, String> {
    let at = if s == "now" {
        now
    } else {
        parse_instant(s).map_err(|_| {
            format!("--last-verified {s:?}: expected now, Unix seconds or a YYYY-MM-DD date")
        })?
    };
    if at < 0 {
        return Err(format!(
            "--last-verified {s:?}: must not be before 1970-01-01"
        ));
    }
    Ok(at)
}

/// `<unix-seconds>` or `<YYYY-MM-DD>` (UTC midnight). Seconds are ASCII digits only:
/// `i64::from_str` also takes a leading `+` / `-`, so `+100` ran at 100 (PR #259 review).
fn parse_instant(s: &str) -> Result<i64, String> {
    if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) {
        if let Ok(secs) = s.parse::<i64>() {
            return Ok(secs);
        }
    }
    parse_date(s)
        .ok_or_else(|| format!("cutoff {s:?} is neither Unix seconds nor a YYYY-MM-DD date"))
}

/// Strict `YYYY-MM-DD`: four-digit year, two-digit month and day, the day valid for the month
/// (leap years included). Anything looser — `2026-1-1`, `2026-02-30`, a time suffix — is `None`.
fn parse_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = &s[r];
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    Some(days_from_civil(y, m, d) * DAY)
}

/// `<N><unit>`, N ≥ 1, unit in `s m h d w`.
fn parse_window(s: &str) -> Result<i64, String> {
    let bad =
        || format!("--older-than {s:?}: expected <N><unit> with unit s, m, h, d or w (e.g. 90d)");
    // `strip_suffix`, not `split_at(len - 1)`: a byte offset lands inside a multibyte last char
    // (`90é`) and `split_at` panics — exit 101 instead of a usage error (PR #259 review).
    let (n, unit, per) = [
        ("s", 1),
        ("m", 60),
        ("h", 3_600),
        ("d", DAY),
        ("w", 7 * DAY),
    ]
    .into_iter()
    .find_map(|(unit, per)| s.strip_suffix(unit).map(|n| (n, unit, per)))
    .ok_or_else(bad)?;
    if n.is_empty() || !n.bytes().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let n: i64 = n.parse().map_err(|_| bad())?;
    if n == 0 {
        return Err(format!(
            "--older-than {s:?}: the window must be at least 1{unit}"
        ));
    }
    n.checked_mul(per).ok_or_else(bad)
}

/// Render a Unix-seconds instant as `YYYY-MM-DDTHH:MM:SSZ` for the human line.
pub fn format_utc(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(DAY), secs.rem_euclid(DAY));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 for a proleptic-Gregorian date (Hinnant, `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`] (Hinnant, `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_791_306_916;

    #[test]
    fn unix_seconds_pass_through() {
        assert_eq!(resolve(&["1767225600"], None, NOW), Ok(1_767_225_600));
        assert_eq!(resolve(&["0"], None, NOW), Ok(0));
        for signed in ["+100", "-100", " 100", "１００"] {
            assert!(
                resolve(&[signed], None, NOW).is_err(),
                "{signed:?} accepted"
            );
        }
    }

    #[test]
    fn iso_date_is_utc_midnight() {
        assert_eq!(resolve(&["1970-01-01"], None, NOW), Ok(0));
        assert_eq!(resolve(&["2026-01-01"], None, NOW), Ok(1_767_225_600));
        assert_eq!(resolve(&["2024-02-29"], None, NOW), Ok(1_709_164_800));
        assert_eq!(resolve(&["2000-03-01"], None, NOW), Ok(951_868_800));
    }

    #[test]
    fn malformed_or_impossible_dates_are_refused() {
        for bad in [
            "2026-1-1",
            "2026-02-30",
            "2023-02-29",
            "1900-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-01-01T00:00:00Z",
            "+026-01-01",
            "soon",
            "",
        ] {
            assert!(resolve(&[bad], None, NOW).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn older_than_subtracts_the_window_from_now() {
        assert_eq!(resolve(&[], Some("90d"), NOW), Ok(NOW - 90 * DAY));
        assert_eq!(resolve(&[], Some("2w"), NOW), Ok(NOW - 14 * DAY));
        assert_eq!(resolve(&[], Some("12h"), NOW), Ok(NOW - 12 * 3_600));
        assert_eq!(resolve(&[], Some("30m"), NOW), Ok(NOW - 1_800));
        assert_eq!(resolve(&[], Some("45s"), NOW), Ok(NOW - 45));
    }

    #[test]
    fn older_than_requires_a_positive_count_and_a_unit() {
        for bad in [
            "90",
            "d",
            "0d",
            "-5d",
            "1.5d",
            "90y",
            "90D",
            "",
            "99999999999999999999d",
            "+5d",
            // Multibyte last / only char: once a `split_at` panic, now a usage error.
            "90é",
            "é",
            "9日",
            "90d\u{301}",
            "٩٠d",
        ] {
            assert!(resolve(&[], Some(bad), NOW).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn exactly_one_spelling() {
        assert!(resolve(&[], None, NOW).is_err(), "nothing given");
        assert!(resolve(&["100", "200"], None, NOW).is_err(), "two operands");
        assert!(
            resolve(&["soon", "100"], None, NOW).is_err(),
            "stray operand"
        );
        assert!(
            resolve(&["100"], Some("90d"), NOW).is_err(),
            "operand and window"
        );
    }

    #[test]
    fn last_verified_takes_now_seconds_or_a_date() {
        assert_eq!(parse_verified("now", NOW), Ok(NOW));
        assert_eq!(parse_verified("0", NOW), Ok(0), "explicit never-verified");
        assert_eq!(parse_verified("1767225600", NOW), Ok(1_767_225_600));
        assert_eq!(parse_verified("2026-01-01", NOW), Ok(1_767_225_600));
        for bad in [
            "-1",
            "+100",
            "1969-12-31",
            "yesterday",
            "2026-02-30",
            "NOW",
            "",
            "1é",
        ] {
            assert!(parse_verified(bad, NOW).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn format_round_trips_dates() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_767_225_600), "2026-01-01T00:00:00Z");
        assert_eq!(format_utc(1_709_164_800 + 3_661), "2024-02-29T01:01:01Z");
        assert_eq!(format_utc(-1), "1969-12-31T23:59:59Z");
        for days in [-800_000i64, -1, 0, 59, 60, 11_016, 20_454, 800_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "round trip at day {days}");
        }
    }
}
