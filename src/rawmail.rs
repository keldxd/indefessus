//! Minimal parsing of raw emails from the trap mailbox: just enough to find which test a
//! message belongs to. Standard library only.

use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct ParsedMail {
    pub sender: String,
    pub subject: String,
    pub message_id: String,
    pub body: String,
    /// Tokens that identify which test this mail belongs to (address first, body as fallback)
    pub tokens: BTreeSet<String>,
    /// Tokens found only in the address and subject headers
    pub header_tokens: BTreeSet<String>,
    /// When the mail was sent, from its Date header (seconds since 1970, UTC)
    pub date: Option<f64>,
}

/// Parses an RFC 2822 date such as "Thu, 5 Nov 2026 10:29:27 +0300" into seconds since 1970 (UTC).
pub fn parse_date(value: &str) -> Option<f64> {
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let value = value.split('(').next()?.trim();
    let value = match value.split_once(',') {
        Some((_, rest)) => rest.trim(),
        None => value,
    };
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    let day: i64 = parts[0].parse().ok()?;
    let month = MONTHS.iter().position(|m| parts[1].to_ascii_lowercase().starts_with(m))? as i64 + 1;
    let mut year: i64 = parts[2].parse().ok()?;
    if year < 100 {
        year += if year < 50 { 2000 } else { 1900 };
    }
    let hms: Vec<i64> = parts[3].split(':').map(|x| x.parse().unwrap_or(0)).collect();
    let (h, mi, s) = (hms.first().copied()?, hms.get(1).copied().unwrap_or(0), hms.get(2).copied().unwrap_or(0));
    let offset = parts.get(4).map(|z| zone_offset(z)).unwrap_or(0);
    // days from civil (Howard Hinnant)
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + h * 3600 + mi * 60 + s - offset) as f64)
}

fn zone_offset(zone: &str) -> i64 {
    let z = zone.trim();
    if (z.starts_with('+') || z.starts_with('-')) && z.len() >= 5 {
        let sign = if z.starts_with('-') { -1 } else { 1 };
        let hh: i64 = z[1..3].parse().unwrap_or(0);
        let mm: i64 = z[3..5].parse().unwrap_or(0);
        return sign * (hh * 3600 + mm * 60);
    }
    0 // "UT", "GMT", "Z" and unknown names are treated as UTC
}

/// Finds every test token ("ind" followed by 8 lowercase hex characters) in a text.
pub fn find_tokens(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut found = BTreeSet::new();
    let mut i = 0;
    while i + 11 <= bytes.len() {
        if &bytes[i..i + 3] == b"ind" && bytes[i + 3..i + 11].iter().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b)) {
            found.insert(text[i..i + 11].to_string());
            i += 11;
        } else {
            i += 1;
        }
    }
    found
}

fn split_headers(raw: &str) -> (&str, &str) {
    for sep in ["\r\n\r\n", "\n\n"] {
        if let Some(pos) = raw.find(sep) {
            return (&raw[..pos], &raw[pos + sep.len()..]);
        }
    }
    (raw, "")
}

/// Header lines, with folded (continued) lines joined back together.
fn headers(section: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in section.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    out
}

pub fn parse_raw(raw: &[u8]) -> ParsedMail {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = split_headers(&text);
    let hs = headers(head);
    let get = |name: &str| hs.iter().filter(|(n, _)| n == name).map(|(_, v)| v.as_str()).collect::<Vec<_>>().join(" ");
    // Quoted-printable soft line breaks can split a token in two; join them back
    let body = body.replace("=\r\n", "").replace("=\n", "");
    // The address the mail was sent to (trap+token@...) says which test it belongs to.
    // Only fall back to tokens in the body when the address carries none, because a
    // forwarded email can quote many other test emails.
    let address_part = ["to", "cc", "delivered-to", "x-original-to", "subject"].map(get).join(" ");
    let header_tokens = find_tokens(&address_part);
    let tokens = if header_tokens.is_empty() { find_tokens(&body) } else { header_tokens.clone() };
    let message_id = get("message-id");
    let date = parse_date(&get("date"));
    ParsedMail {
        sender: get("from"),
        subject: get("subject"),
        message_id: if message_id.is_empty() { format!("noid-{}", raw.len()) } else { message_id },
        body,
        tokens,
        header_tokens,
        date,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_found() {
        let t = find_tokens("x indabc12345 y ind1234 zindffffffff");
        assert_eq!(t.into_iter().collect::<Vec<_>>(), vec!["indabc12345", "indffffffff"]);
    }

    #[test]
    fn address_token_wins_over_quoted_tokens() {
        let raw = b"From: a@b.example\r\nTo: trap+ind00000001@t.example\r\nSubject: Fwd: x\r\nMessage-ID: <1@b>\r\n\r\nquoted ind00000002 and ind00000003\r\n";
        let m = parse_raw(raw);
        assert_eq!(m.tokens.into_iter().collect::<Vec<_>>(), vec!["ind00000001"]);
        assert_eq!(m.subject, "Fwd: x");
    }

    #[test]
    fn body_fallback_and_soft_breaks() {
        let raw = b"From: a@b.example\nTo: trap@t.example\nSubject: hi\n\nthe code ind0000=\nabcd is here\n";
        let m = parse_raw(raw);
        assert_eq!(m.tokens.into_iter().collect::<Vec<_>>(), vec!["ind0000abcd"]);
    }

    #[test]
    fn dates_parse() {
        // 2026-11-05 10:29:27 UTC
        assert_eq!(parse_date("Thu, 5 Nov 2026 10:29:27 +0000"), Some(1_793_874_567.0));
        assert_eq!(parse_date("Thu, 05 Nov 2026 13:29:27 +0300 (EAT)"), Some(1_793_874_567.0));
        assert_eq!(parse_date("5 Nov 2026 10:29:27 GMT"), Some(1_793_874_567.0));
        assert_eq!(parse_date("not a date"), None);
        let raw = b"From: a@b\r\nTo: trap+ind0000beef@t\r\nDate: Thu, 5 Nov 2026 10:29:27 +0000\r\nSubject: s\r\n\r\nbody";
        assert_eq!(parse_raw(raw).date, Some(1_793_874_567.0));
    }

    #[test]
    fn folded_headers() {
        let raw = b"To: someone@x.example,\r\n trap+ind0000beef@t.example\r\nSubject: s\r\n\r\nbody";
        assert!(parse_raw(raw).tokens.contains("ind0000beef"));
    }
}
