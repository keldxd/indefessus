//! Sending test emails (SMTP) and collecting evidence from the trap mailbox (IMAP).
//!
//! Every test email carries an X-Indefessus-Test header with its token, so anyone reviewing
//! the mailbox later can see it was a test and not a real attack.

use crate::rawmail::{parse_raw, ParsedMail};
use crate::util::imap_date;
use anyhow::{anyhow, Context, Result};
use lettre::message::header::{ContentType, Header, HeaderName, HeaderValue};
use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use std::sync::Mutex;

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
}

pub fn build_message(m: &Outgoing) -> Result<Message> {
    let from = Mailbox::new(Some(m.display_name.to_string()), m.from.parse().with_context(|| format!("bad address {}", m.from))?);
    let to: Mailbox = m.to.parse().with_context(|| format!("bad address {}", m.to))?;
    let mut builder = Message::builder().from(from).to(to).subject(m.subject).message_id(None);
    if !m.token.is_empty() {
        builder = builder.header(XIndefessusTest(m.token.to_string()));
    }
    let msg = if m.html.is_empty() {
        builder.header(ContentType::TEXT_PLAIN).body(m.text.to_string())?
    } else {
        builder.multipart(MultiPart::alternative_plain_html(m.text.to_string(), m.html.to_string()))?
    };
    Ok(msg)
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
    pub fn send(&self, messages: &[Message]) -> Result<()> {
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
        let mailer = builder.build();
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

fn fetch_with<T: std::io::Read + std::io::Write>(client: imap::Client<T>, trap: &ImapTrap, username: &str, since_ts: f64) -> Result<Vec<ParsedMail>> {
    let mut session = client.login(username, &trap.password).map_err(|e| anyhow!("trap mailbox login failed for {username}: {}", e.0))?;
    session.examine(&trap.folder)?;
    // One day of margin, because IMAP dates have no time of day
    let ids = session.search(format!("SINCE {}", imap_date(since_ts - 86_400.0)))?;
    let mut found = Vec::new();
    if !ids.is_empty() {
        let set = ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        let messages = session.fetch(set, "RFC822")?;
        for m in messages.iter() {
            if let Some(body) = m.body() {
                found.push(parse_raw(body));
            }
        }
    }
    let _ = session.logout();
    Ok(found)
}

impl ImapTrap {
    fn fetch_one(&self, username: &str, since_ts: f64) -> Result<Vec<ParsedMail>> {
        let target = format!("{}:{}", self.host, self.port);
        if self.tls {
            let tls = native_tls::TlsConnector::builder().build()?;
            let client = imap::connect((self.host.as_str(), self.port), &self.host, &tls).with_context(|| format!("cannot connect to {target}"))?;
            fetch_with(client, self, username, since_ts)
        } else {
            // Plain connection: only for a local test lab
            let stream = std::net::TcpStream::connect((self.host.as_str(), self.port)).with_context(|| format!("cannot connect to {target}"))?;
            let mut client = imap::Client::new(stream);
            client.read_greeting()?;
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
