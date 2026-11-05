//! Minimal parsing of raw emails from the trap mailbox: just enough to find which test a
//! message belongs to. Standard library only.

use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct ParsedMail {
    pub sender: String,
    pub subject: String,
    pub message_id: String,
    pub body: String,
    pub tokens: BTreeSet<String>,
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
    let tokens = if header_tokens.is_empty() { find_tokens(&body) } else { header_tokens };
    let message_id = get("message-id");
    ParsedMail {
        sender: get("from"),
        subject: get("subject"),
        message_id: if message_id.is_empty() { format!("noid-{}", raw.len()) } else { message_id },
        body,
        tokens,
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
    fn folded_headers() {
        let raw = b"To: someone@x.example,\r\n trap+ind0000beef@t.example\r\nSubject: s\r\n\r\nbody";
        assert!(parse_raw(raw).tokens.contains("ind0000beef"));
    }
}
