//! Sending test emails (SMTP) and collecting evidence from the trap mailbox (IMAP).
//!
//! Every test email carries an X-Indefessus-Test header with its token, so anyone reviewing
//! the mailbox later can see it was a test and not a real attack.

use crate::rawmail::{parse_raw, ParsedMail};
use crate::util::imap_date;
use anyhow::{anyhow, Context, Result};
use lettre::message::header::{ContentType, Header, HeaderName, HeaderValue};
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone)]
struct XIndefessusTest(String);

impl Header for XIndefessusTest {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("X-Indefessus-Test")
    }
    fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self(s.to_string()))
    }
    fn display(&self) -> HeaderValue {
        HeaderValue::new(Self::name(), self.0.clone())
    }
}

pub struct Outgoing<'a> {
    pub from: &'a str,
    pub display_name: &'a str,
    pub to: &'a str,
    pub subject: &'a str,
    pub text: &'a str,
    pub html: &'a str,
    pub token: &'a str,
    /// Files to attach (tests whose trick is inside a file)
    pub attachments: &'a [crate::attach::Built],
}

pub fn build_message(m: &Outgoing) -> Result<Message> {
    let from = Mailbox::new(Some(m.display_name.to_string()), m.from.parse().with_context(|| format!("bad address {}", m.from))?);
    let to: Mailbox = m.to.parse().with_context(|| format!("bad address {}", m.to))?;
    let mut builder = Message::builder().from(from).to(to).subject(m.subject).message_id(None);
    if !m.token.is_empty() {
        builder = builder.header(XIndefessusTest(m.token.to_string()));
    }
    if m.attachments.is_empty() {
        let msg = if m.html.is_empty() {
            builder.header(ContentType::TEXT_PLAIN).body(m.text.to_string())?
        } else {
            builder.multipart(MultiPart::alternative_plain_html(m.text.to_string(), m.html.to_string()))?
        };
        return Ok(msg);
    }
    let mut mixed = if m.html.is_empty() {
        MultiPart::mixed().singlepart(SinglePart::plain(m.text.to_string()))
    } else {
        MultiPart::mixed().multipart(MultiPart::alternative_plain_html(m.text.to_string(), m.html.to_string()))
    };
    for a in m.attachments {
        let content_type = ContentType::parse(&a.mime).map_err(|_| anyhow!("bad attachment type {}", a.mime))?;
        mixed = mixed.singlepart(Attachment::new(a.filename.clone()).body(a.bytes.clone(), content_type));
    }
    Ok(builder.multipart(mixed)?)
}

/// How to connect to a mail server. "none" is only for a local test lab, never real mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    StartTls,
    Tls,
    None,
}

impl Security {
    pub fn parse(s: &str) -> Result<Security> {
        match s.to_ascii_lowercase().as_str() {
            "starttls" | "" => Ok(Security::StartTls),
            "tls" | "ssl" => Ok(Security::Tls),
            "none" => Ok(Security::None),
            other => Err(anyhow!("unknown security '{other}': use starttls, tls or none")),
        }
    }
}

pub struct SmtpSender {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub security: Security,
}

impl SmtpSender {
    fn transport(&self) -> Result<SmtpTransport> {
        let builder = match self.security {
            Security::Tls => SmtpTransport::relay(&self.host)?,
            Security::StartTls => SmtpTransport::starttls_relay(&self.host)?,
            Security::None => SmtpTransport::builder_dangerous(&self.host),
        };
        let builder = builder.port(self.port);
        let builder = if self.password.is_empty() {
            builder
        } else {
            builder.credentials(Credentials::new(self.username.clone(), self.password.clone()))
        };
        Ok(builder.build())
    }

    /// Connects and logs in without sending anything (used by `indefessus doctor`).
    pub fn test(&self) -> Result<()> {
        match self.transport()?.test_connection() {
            Ok(true) => Ok(()),
            Ok(false) => Err(anyhow!("the server did not accept the connection")),
            Err(e) => Err(anyhow!("{e}")),
        }
    }

    pub fn send(&self, messages: &[Message]) -> Result<()> {
        let mailer = self.transport()?;
        for msg in messages {
            mailer.send(msg).map_err(|e| anyhow!("sending failed: {e}"))?;
            std::thread::sleep(std::time::Duration::from_secs(1)); // be gentle with small providers
        }
        Ok(())
    }
}

/// Where trap mail comes from: a real IMAP mailbox, or memory (self-test).
pub trait TrapSource {
    fn fetch_since(&self, since_ts: f64) -> Result<Vec<ParsedMail>>;
}

pub struct ImapTrap {
    pub host: String,
    pub port: u16,
    /// Usually one login (the trap mailbox). A local lab server without plus-addressing
    /// keeps each trap+token address as its own mailbox, so every one is checked.
    pub usernames: Vec<String>,
    pub password: String,
    pub folder: String,
    pub tls: bool,
}

/// Anyone can send mail to the trap address, so reading it is bounded: messages over this size
/// are skipped (their size is asked for first, so they are never downloaded),
const MAX_MESSAGE_BYTES: u32 = 10 * 1024 * 1024;
/// at most this many of the newest messages are read per check,
const MAX_MESSAGES: usize = 1000;
/// and a server that stops answering is given up on instead of waited for forever.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_WRITE_TIMEOUT: Duration = Duration::from_secs(60);

/// The newest `max` message numbers, oldest first, and how many older ones were left out.
fn newest(mut ids: Vec<u32>, max: usize) -> (Vec<u32>, usize) {
    ids.sort_unstable();
    let skipped = ids.len().saturating_sub(max);
    (ids.split_off(skipped), skipped)
}

/// The messages small enough to read, and how many were too big (or gave no size).
fn small_enough(sizes: &[(u32, Option<u32>)], max_bytes: u32) -> (Vec<u32>, usize) {
    let keep: Vec<u32> = sizes.iter().filter(|(_, size)| matches!(size, Some(s) if *s <= max_bytes)).map(|(id, _)| *id).collect();
    let too_big = sizes.len() - keep.len();
    (keep, too_big)
}

fn id_set(ids: &[u32]) -> String {
    ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")
}

fn fetch_with<T: std::io::Read + std::io::Write>(client: imap::Client<T>, trap: &ImapTrap, username: &str, since_ts: f64) -> Result<Vec<ParsedMail>> {
    let mut session = client.login(username, &trap.password).map_err(|e| anyhow!("trap mailbox login failed for {username}: {}", e.0))?;
    session.examine(&trap.folder)?;
    // One day of margin, because IMAP dates have no time of day
    let ids = session.search(format!("SINCE {}", imap_date(since_ts - 86_400.0)))?;
    let (ids, too_many) = newest(ids.into_iter().collect(), MAX_MESSAGES);
    let mut found = Vec::new();
    if !ids.is_empty() {
        let sizes: Vec<(u32, Option<u32>)> = session.fetch(id_set(&ids), "RFC822.SIZE")?.iter().map(|f| (f.message, f.size)).collect();
        let (wanted, too_big) = small_enough(&sizes, MAX_MESSAGE_BYTES);
        if too_many > 0 || too_big > 0 {
            eprintln!(
                "Note: in the trap mailbox {username}, skipped {too_many} older message(s) beyond the newest {MAX_MESSAGES} and {too_big} message(s) over {} MB.",
                MAX_MESSAGE_BYTES / (1024 * 1024)
            );
        }
        if !wanted.is_empty() {
            let messages = session.fetch(id_set(&wanted), "(RFC822 INTERNALDATE)")?;
            for m in messages.iter() {
                if let Some(body) = m.body() {
                    let mut mail = parse_raw(body);
                    // The time the trap mailbox's server received it; the Date line is the sender's word
                    mail.received = m.internal_date().map(|d| d.timestamp() as f64);
                    found.push(mail);
                }
            }
        }
    }
    let _ = session.logout();
    Ok(found)
}

/// Connects with a time limit, then puts one on every read and write.
fn connect_tcp(host: &str, port: u16) -> Result<TcpStream> {
    let mut last = None;
    for addr in (host, port).to_socket_addrs().with_context(|| format!("cannot find {host}"))? {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream.set_read_timeout(Some(READ_WRITE_TIMEOUT))?;
                stream.set_write_timeout(Some(READ_WRITE_TIMEOUT))?;
                return Ok(stream);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(match last {
        Some(e) => anyhow::Error::from(e),
        None => anyhow!("cannot find {host}"),
    })
}

impl ImapTrap {
    fn fetch_one(&self, username: &str, since_ts: f64) -> Result<Vec<ParsedMail>> {
        let target = format!("{}:{}", self.host, self.port);
        let stream = connect_tcp(&self.host, self.port).with_context(|| format!("cannot connect to {target}"))?;
        if self.tls {
            let tls = native_tls::TlsConnector::builder().build()?;
            let stream = tls.connect(&self.host, stream).map_err(|e| anyhow!("secure connection to {target} failed: {e}"))?;
            let mut client = imap::Client::new(stream);
            client.read_greeting().with_context(|| format!("no greeting from {target}"))?;
            fetch_with(client, self, username, since_ts)
        } else {
            // Plain connection: only for a local test lab
            let mut client = imap::Client::new(stream);
            client.read_greeting().with_context(|| format!("no greeting from {target}"))?;
            fetch_with(client, self, username, since_ts)
        }
    }
}

impl TrapSource for ImapTrap {
    fn fetch_since(&self, since_ts: f64) -> Result<Vec<ParsedMail>> {
        let mut all = Vec::new();
        for username in &self.usernames {
            all.extend(self.fetch_one(username, since_ts)?);
        }
        Ok(all)
    }
}

/// An in-memory trap mailbox, used by the self-test.
#[derive(Default)]
pub struct MemoryTrap {
    messages: Mutex<Vec<ParsedMail>>,
}

impl MemoryTrap {
    pub fn deliver(&self, from: &str, to: &str, subject: &str, body: &str) {
        let n = self.messages.lock().map(|m| m.len()).unwrap_or(0);
        let raw = format!("From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nMessage-ID: <{n}@selftest.invalid>\r\n\r\n{body}\r\n");
        if let Ok(mut m) = self.messages.lock() {
            m.push(parse_raw(raw.as_bytes()));
        }
    }
}

impl TrapSource for MemoryTrap {
    fn fetch_since(&self, _since_ts: f64) -> Result<Vec<ParsedMail>> {
        Ok(self.messages.lock().map(|m| m.clone()).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_messages_are_read() {
        assert_eq!(newest(vec![5, 1, 4, 2, 3], 3), (vec![3, 4, 5], 2));
        assert_eq!(newest(vec![2, 1], 10), (vec![1, 2], 0));
        assert_eq!(newest(Vec::new(), 10), (Vec::new(), 0));
    }

    #[test]
    fn huge_messages_are_never_downloaded() {
        let sizes = [(1, Some(500)), (2, Some(MAX_MESSAGE_BYTES)), (3, Some(MAX_MESSAGE_BYTES + 1)), (4, None)];
        assert_eq!(small_enough(&sizes, MAX_MESSAGE_BYTES), (vec![1, 2], 2));
    }
}
