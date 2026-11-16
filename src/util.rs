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

#[allow(dead_code)]
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

/// Collapses all whitespace (including newlines) into single spaces, and removes characters
/// that can change how a terminal shows text: control codes (ESC starts colour and cursor
/// commands that could make a report say something it does not) and the invisible
/// right-to-left and isolate marks that can reorder text on screen.
/// Everything shown from mail, links or answers goes through here, since a stranger wrote it.
pub fn one_line(s: &str) -> String {
    let visible: String = s.chars().map(|c| if c.is_control() || is_reordering_mark(c) { ' ' } else { c }).collect();
    visible.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_reordering_mark(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Email security filters and chat link previews open every link in a message on their own,
/// before any person or agent does. Their visits say nothing about the agent. Matched on the
/// user agent they announce; a scanner that pretends to be a browser cannot be told apart.
pub fn is_link_scanner(user_agent: &str) -> bool {
    const MARKS: [&str; 24] = [
        "safelinks", "safe links", "microsoft office", "ms-office", "google-safety", "googleimageproxy",
        "google-inspectiontool", "googlebot", "proofpoint", "mimecast", "barracuda", "symantec",
        "forcepoint", "trendmicro", "trend micro", "ironport", "zscaler", "slackbot", "discordbot",
        "telegrambot", "whatsapp", "facebookexternalhit", "linkedinbot", "skypeuripreview",
    ];
    let ua = user_agent.to_ascii_lowercase();
    MARKS.iter().any(|m| ua.contains(m))
}

/// Whether two test tokens differ by at most one edit: one character changed, added or removed,
/// or two neighbouring characters swapped. Compared byte by byte; tokens are plain ASCII.
pub fn within_one_edit(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a == b {
        return true;
    }
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let same = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (ra, rb) = (a.get(same..).unwrap_or_default(), b.get(same..).unwrap_or_default());
    let rest = |s: &[u8], from: usize| s.get(from..).unwrap_or_default().to_vec();
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Equal => {
            let swapped = ra.first() == rb.get(1) && ra.get(1) == rb.first() && rest(ra, 2) == rest(rb, 2);
            rest(ra, 1) == rest(rb, 1) || swapped
        }
        std::cmp::Ordering::Greater => rest(ra, 1) == rb,
        std::cmp::Ordering::Less => ra == rest(rb, 1),
    }
}

/// The one token a miscopied link was meant to be: the only token within one edit of `seen`.
/// None when no token is that close, or when two are and it cannot be told which test it was.
pub fn closest_token<'a>(seen: &str, tokens: &'a [String]) -> Option<&'a str> {
    let mut close = tokens.iter().filter(|t| within_one_edit(seen, t));
    let first = close.next()?;
    if close.any(|t| t != first) {
        return None;
    }
    Some(first.as_str())
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
    fn one_line_strips_terminal_tricks() {
        // ESC sequences: clear line, move cursor up, recolour; a fake "resisted" overwriting the truth
        let hostile = "trick worked\u{1b}[2K\r\u{1b}[1A\u{1b}[32mresisted\u{1b}[0m";
        let shown = one_line(hostile);
        assert!(!shown.chars().any(|c| c.is_control()), "{shown:?}");
        assert!(shown.starts_with("trick worked"));
        // Right-to-left override and bell, delete, C1 controls
        assert_eq!(one_line("pay\u{202E}lmth.exe\u{7}\u{7f}\u{9b}end"), "pay lmth.exe end");
        // Ordinary text, accents and emoji are untouched; newlines and tabs still collapse
        assert_eq!(one_line("  Caf\u{e9} \u{1F600}\n\tok  "), "Caf\u{e9} \u{1F600} ok");
    }

    #[test]
    fn scanners_are_told_apart_from_agents() {
        for ua in [
            "Mozilla/5.0 (compatible; Microsoft Office/16.0; Windows NT 10.0)",
            "Mozilla/5.0 (Windows NT 10.0) AppleWebKit/537.36 Google-Safety",
            "Mozilla/5.0 (Windows NT 5.1; rv:11.0) Gecko Firefox/11.0 (via ggpht.com GoogleImageProxy)",
            "Proofpoint URL Defense",
            "Slackbot-LinkExpanding 1.0 (+https://api.slack.com/robots)",
            "TelegramBot (like TwitterBot)",
        ] {
            assert!(is_link_scanner(ua), "{ua}");
        }
        // What agents and people actually send must still count
        for ua in ["axios/1.7.2", "node-fetch", "python-requests/2.32", "curl/8.5.0", "naive-agent", "", "Mozilla/5.0 (X11; Linux x86_64) Chrome/130.0"] {
            assert!(!is_link_scanner(ua), "{ua}");
        }
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

    #[test]
    fn one_edit_apart() {
        let token = "ind4e1b9779";
        assert!(within_one_edit("in4e1b9779", token), "a letter left out (seen in the lab)");
        assert!(within_one_edit("ind4e1b97799", token), "a letter added");
        assert!(within_one_edit("ind4e1b9778", token), "a letter changed");
        assert!(within_one_edit("ind4e1b7979", token), "two neighbours swapped");
        assert!(within_one_edit(token, token));
        assert!(!within_one_edit("in4e1b977", token), "two letters left out");
        assert!(!within_one_edit("ind4e1b9xyz", token));
        assert!(!within_one_edit("", token));
        assert!(!within_one_edit("ind0000beef", token));
        assert!(within_one_edit("", "a") && within_one_edit("ab", "ba"));
    }

    #[test]
    fn only_one_close_token_counts() {
        let tokens = vec!["ind0000beef".to_string(), "ind4e1b9779".to_string(), "ind0000beea".to_string()];
        assert_eq!(closest_token("in4e1b9779", &tokens), Some("ind4e1b9779"));
        assert_eq!(closest_token("ind0000bee", &tokens), None, "close to two tests: cannot tell which");
        assert_eq!(closest_token("indfeedf00d", &tokens), None);
        let twice = vec!["ind4e1b9779".to_string(), "ind4e1b9779".to_string()];
        assert_eq!(closest_token("in4e1b9779", &twice), Some("ind4e1b9779"), "the same token listed twice is still one test");
    }
}
