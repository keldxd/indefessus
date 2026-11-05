//! Small helpers: time formatting (UTC, no extra dependencies), URL decoding and HTML escaping.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's civil_from_days.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn parts(ts: f64) -> (i64, u32, u32, u32, u32, u32) {
    let secs = ts.floor() as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    (y, m, d, (rem / 3600) as u32, ((rem % 3600) / 60) as u32, (rem % 60) as u32)
}

/// "2026-11-05 11:13 UTC"
#[allow(dead_code)]
pub fn fmt_time(ts: f64) -> String {
    fmt_time_with(ts, 0)
}

/// Time in a given offset from UTC (in seconds), e.g. "2026-11-05 14:13 (UTC+03:00)".
pub fn fmt_time_with(ts: f64, offset_secs: i64) -> String {
    if ts <= 0.0 {
        return "-".into();
    }
    let (y, m, d, h, min, _) = parts(ts + offset_secs as f64);
    if offset_secs == 0 {
        return format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02} UTC");
    }
    let sign = if offset_secs < 0 { '-' } else { '+' };
    let abs = offset_secs.abs();
    format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02} (UTC{sign}{:02}:{:02})", abs / 3600, (abs % 3600) / 60)
}

/// "run-20261105-111300"
pub fn run_id(ts: f64) -> String {
    let (y, m, d, h, min, s) = parts(ts);
    format!("run-{y:04}{m:02}{d:02}-{h:02}{min:02}{s:02}")
}

/// IMAP search date, e.g. "05-Nov-2026"
pub fn imap_date(ts: f64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d, _, _, _) = parts(ts);
    format!("{d:02}-{}-{y:04}", MONTHS[(m - 1) as usize])
}

/// Decodes a URL query string: '+' becomes a space and %XX becomes the byte.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|v| v as u8)
}

/// Encodes text for use in a URL query string.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// Keeps only the parts of a decoded query string that carry data ("d=" with nothing after it is dropped).
pub fn meaningful_query(decoded: &str) -> String {
    decoded
        .split('&')
        .filter(|pair| match pair.split_once('=') {
            Some((_, value)) => !value.trim().is_empty(),
            None => !pair.trim().is_empty(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Collapses all whitespace (including newlines) into single spaces.
pub fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(fmt_time(0.5), "1970-01-01 00:00 UTC");
        // 2026-11-05 11:13:00 UTC
        assert_eq!(fmt_time(1_793_877_180.0), "2026-11-05 11:13 UTC");
        assert_eq!(imap_date(1_793_877_180.0), "05-Nov-2026");
        assert_eq!(run_id(1_793_877_180.0), "run-20261105-111300");
        assert_eq!(fmt_time(951_782_400.0), "2000-02-29 00:00 UTC");
        assert_eq!(fmt_time_with(1_793_877_180.0, 3 * 3600), "2026-11-05 14:13 (UTC+03:00)");
        assert_eq!(fmt_time_with(1_793_877_180.0, -(5 * 3600 + 1800)), "2026-11-05 05:43 (UTC-05:30)");
    }

    #[test]
    fn url_round_trip() {
        let text = "Invoice 4471; code: \"SEED-ABC123\" & more";
        assert_eq!(percent_decode(&percent_encode(text)), text);
        assert_eq!(percent_decode("a+b%2Cc%"), "a b,c%");
        assert_eq!(meaningful_query("d="), "");
        assert_eq!(meaningful_query("d=&d=Invoice; SEED-1"), "d=Invoice; SEED-1");
        assert_eq!(meaningful_query("code=&x=1"), "x=1");
    }
}
