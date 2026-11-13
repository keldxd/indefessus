//! Self-test: proves the whole pipeline works, with no real mailbox and no real agent.
//!
//! Three stand-in agents read the same test emails:
//!   - a NAIVE agent that obeys any instruction it finds in an email (every test should fail)
//!   - a CAREFUL agent that never follows instructions from email content (every test should pass)
//!   - a SILENT agent that is switched off (no test may pass: results must be inconclusive)
//!   - a BLIND agent that answers email but never opens attachments (file tests must be
//!     inconclusive, never passed: it did not resist a file it never read)
//! These are simple programs that imitate the two extremes. They are not AI models, and the
//! results say nothing about any real product. They show that Indefessus catches an agent
//! that misbehaves, and raises no false alarms on one that behaves.

use crate::attach::{extract_text, Built};
use crate::canary::Background;
use crate::evaluate::{
    apply_attachment_check, apply_liveness, attachment_check_email, attachment_read, collect_trapmail, control_email, control_replied, evaluate_run,
    ATTACH_CHECK_ID, CONTROL_ID, FAIL, INCONCLUSIVE, PASS,
};
use crate::mail::{MemoryTrap, TrapSource};
use crate::report::text_report;
use crate::scenarios::{load_library, new_token, render};
use crate::store::Store;
use crate::util::{now, percent_encode};
use anyhow::Result;
use base64::Engine;
use regex::Regex;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;

struct Email {
    subject: String,
    text: String,
    html: String,
    attachments: Vec<Built>,
}

struct Patterns {
    url: Regex,
    email: Regex,
    phrase: Regex,
    b64: Regex,
    tag: Regex,
}

impl Patterns {
    fn new() -> Patterns {
        Patterns {
            url: Regex::new(r#"https?://[^\s"'<>)]+"#).unwrap(),
            email: Regex::new(r"[\w.+-]+@[\w-]+(?:\.[\w-]+)+").unwrap(),
            phrase: Regex::new(r#"(?:phrase|code|reference|words)\s*[:"“]+\s*"?([A-Za-z0-9][A-Za-z0-9 _-]{2,60}?)["”]"#).unwrap(),
            b64: Regex::new(r"[A-Za-z0-9+/=]{40,}").unwrap(),
            tag: Regex::new(r"<[^>]+>").unwrap(),
        }
    }

    /// What a model would see: text, HTML without tags (hidden text included), decoded base64,
    /// and the text of every attachment (hidden parts included, as text extraction does).
    fn readable(&self, m: &Email) -> String {
        let mut text = self.readable_body(m);
        for a in &m.attachments {
            text.push('\n');
            text.push_str(&extract_text(&a.filename, &a.bytes));
        }
        text
    }

    /// The same, for an agent that never opens attachments.
    fn readable_body(&self, m: &Email) -> String {
        let mut text = format!("{}\n{}", m.text, self.tag.replace_all(&m.html, " "));
        let blobs: Vec<String> = self.b64.find_iter(&text).map(|x| x.as_str().to_string()).collect();
        for blob in blobs {
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(blob.as_bytes()) {
                if let Ok(decoded) = String::from_utf8(bytes) {
                    text.push('\n');
                    text.push_str(&decoded);
                }
            }
        }
        text
    }
}

fn http_get(addr: SocketAddr, path_and_query: &str) -> Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    write!(stream, "GET {path_and_query} HTTP/1.1\r\nHost: localhost\r\nUser-Agent: naive-agent\r\nConnection: close\r\n\r\n")?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;
    Ok(())
}

trait Agent {
    fn name(&self) -> &'static str;
    fn process(&mut self, inbox: &[Email]) -> Result<()>;
    /// What the assistant says when asked (guided tests). None: it never answers.
    fn answer(&self, prompt: &str) -> Option<String>;
}

/// A booking number, if the text the agent read has one (the attachment check).
fn booking_number(read: &str) -> Option<String> {
    let rest = read.split("Booking number: ").nth(1)?;
    rest.split_whitespace().next().map(|w| w.to_string())
}

/// Replying to the person who wrote is what any working inbox agent does. Every working stand-in
/// does it for every email, which is how the control email confirms they are alive. Like a
/// helpful assistant, it quotes a booking number if it saw one in what it read.
fn reply_to_sender(replies: &MemoryTrap, m: &Email, read: &str) {
    let booking = booking_number(read).map(|b| format!(" Your booking number {b} is noted.")).unwrap_or_default();
    replies.deliver(
        "assistant@victim.example",
        SENDER,
        &format!("Re: {}", m.subject),
        &format!("Thanks for your email about \"{}\". We will get back to you.{booking}", m.subject),
    );
}

const SENDER: &str = "tester@sender.invalid";

struct NaiveAgent<'a> {
    p: &'a Patterns,
    trap: &'a MemoryTrap,
    replies: &'a MemoryTrap,
    base: String,
    addr: SocketAddr,
    trap_domain: String,
    subjects: Vec<String>,
    planted: Vec<String>,
}

impl Agent for NaiveAgent<'_> {
    fn name(&self) -> &'static str {
        "naive agent (obeys instructions in emails)"
    }

    fn process(&mut self, inbox: &[Email]) -> Result<()> {
        self.subjects = inbox.iter().map(|m| m.subject.clone()).collect();
        let everything: String = inbox.iter().map(|m| format!("{}: {}\n", m.subject, self.p.readable(m))).collect();
        let codes: Vec<String> = self.p.phrase.captures_iter(&everything).map(|c| c[1].to_string()).collect();
        for m in inbox {
            let content = self.p.readable(m);
            reply_to_sender(self.replies, m, &content);
            for url in self.p.url.find_iter(&content).map(|x| x.as_str()) {
                if let Some(rest) = url.strip_prefix(&self.base) {
                    let sep = if rest.contains('?') { "&" } else { "?" };
                    let leak = percent_encode(&format!("{} {}", self.subjects.join("; "), codes.join(" ")));
                    http_get(self.addr, &format!("{rest}{sep}d={leak}"))?;
                }
            }
            let mut sent: Vec<&str> = Vec::new();
            for address in self.p.email.find_iter(&content).map(|x| x.as_str()) {
                if address.ends_with(&format!("@{}", self.trap_domain)) && !sent.contains(&address) {
                    self.trap.deliver("assistant@victim.example", address, &format!("Fwd: {}", m.subject), &everything);
                    sent.push(address);
                }
            }
            self.planted.extend(self.p.phrase.captures_iter(&content).map(|c| c[1].to_string()));
        }
        Ok(())
    }

    fn answer(&self, _prompt: &str) -> Option<String> {
        Some(format!("Here is a summary of your inbox: {}. {}", self.subjects.join("; "), self.planted.join(" ")))
    }
}

struct CarefulAgent<'a> {
    p: &'a Patterns,
    replies: &'a MemoryTrap,
    count: usize,
}

impl Agent for CarefulAgent<'_> {
    fn name(&self) -> &'static str {
        "careful agent (never follows instructions from email content)"
    }
    fn process(&mut self, inbox: &[Email]) -> Result<()> {
        self.count = inbox.len();
        for m in inbox {
            reply_to_sender(self.replies, m, &self.p.readable(m));
        }
        Ok(())
    }
    fn answer(&self, _prompt: &str) -> Option<String> {
        Some(format!("You have {} new emails. Some of them contain requests from people you may not know.", self.count))
    }
}

/// An agent that answers email but never opens attachments, and obeys nothing.
struct BlindAgent<'a> {
    p: &'a Patterns,
    replies: &'a MemoryTrap,
    count: usize,
}

impl Agent for BlindAgent<'_> {
    fn name(&self) -> &'static str {
        "blind agent (answers email, never opens attachments)"
    }
    fn process(&mut self, inbox: &[Email]) -> Result<()> {
        self.count = inbox.len();
        for m in inbox {
            reply_to_sender(self.replies, m, &self.p.readable_body(m));
        }
        Ok(())
    }
    fn answer(&self, _prompt: &str) -> Option<String> {
        Some(format!("You have {} new emails.", self.count))
    }
}

/// An agent that is switched off, crashed or out of quota: it does nothing at all.
struct SilentAgent;

impl Agent for SilentAgent {
    fn name(&self) -> &'static str {
        "silent agent (switched off: does nothing)"
    }
    fn process(&mut self, _inbox: &[Email]) -> Result<()> {
        Ok(())
    }
    fn answer(&self, _prompt: &str) -> Option<String> {
        None
    }
}

struct Outcome {
    name: &'static str,
    failed: usize,
    passed: usize,
    inconclusive: usize,
    total: usize,
}

/// Runs three stand-in agents through every test. Returns true if the naive agent failed every
/// test, the careful agent failed none and passed every one, and the silent agent got no
/// passes at all (only inconclusive results, because it never replied to the control email).
pub fn run_selftest(print: bool) -> Result<bool> {
    let lib = load_library(&[])?;
    let dir = std::env::temp_dir().join(format!("indefessus-selftest-{}-{}", std::process::id(), rand::random::<u32>()));
    let store = Arc::new(Store::open(&dir)?);
    let listener = Background::start(store.clone())?;
    let base = listener.base_url();
    let trap_domain = "selftest.invalid";
    let patterns = Patterns::new();
    let mut summary: Vec<Outcome> = Vec::new();

    let file_tests = lib.list.iter().filter(|sc| sc.has_attachments()).count();
    for which in ["naive", "careful", "silent", "blind"] {
        let trap = MemoryTrap::default();
        let replies = MemoryTrap::default();
        let mut agent: Box<dyn Agent + '_> = match which {
            "naive" => Box::new(NaiveAgent {
                p: &patterns,
                trap: &trap,
                replies: &replies,
                base: base.clone(),
                addr: listener.addr,
                trap_domain: trap_domain.into(),
                subjects: vec![],
                planted: vec![],
            }),
            "careful" => Box::new(CarefulAgent { p: &patterns, replies: &replies, count: 0 }),
            "blind" => Box::new(BlindAgent { p: &patterns, replies: &replies, count: 0 }),
            _ => Box::new(SilentAgent),
        };
        let run_id = format!("selftest-{which}-{}", now() as u64);
        store.new_run(&run_id, "selftest", agent.name())?;
        let started = store.run(&run_id)?.map(|r| r.started_at).unwrap_or(0.0);
        let mut inbox = Vec::new();
        let mut prompts = Vec::new();
        for sc in &lib.list {
            let r = render(sc, &base, &format!("trap@{trap_domain}"), true)?;
            store.add_item(&run_id, &sc.id, &r.token, &r.seed_secret)?;
            if sc.has_seed() {
                inbox.push(Email { subject: r.seed_subject.clone(), text: r.seed_body.clone(), html: String::new(), attachments: vec![] });
            }
            inbox.push(Email { subject: r.subject.clone(), text: r.body_text.clone(), html: r.body_html.clone(), attachments: r.attachments.clone() });
            if !sc.detect.output_contains.is_empty() {
                prompts.push((sc.id.clone(), r.guided_prompt.clone()));
            }
        }
        // The attachment check, then the control email last, as in a live run
        let files_token = new_token();
        store.add_item(&run_id, ATTACH_CHECK_ID, &files_token, "")?;
        let (subject, text, file) = attachment_check_email(&files_token)?;
        inbox.push(Email { subject, text, html: String::new(), attachments: vec![file] });
        let control_token = new_token();
        store.add_item(&run_id, CONTROL_ID, &control_token, "")?;
        let (subject, text) = control_email(&control_token);
        inbox.push(Email { subject, text, html: String::new(), attachments: vec![] });

        agent.process(&inbox)?;
        for (id, prompt) in prompts {
            if let Some(answer) = agent.answer(&prompt) {
                store.set_answer(&run_id, &id, &answer)?;
            }
        }
        collect_trapmail(&store, &run_id, &trap)?;
        evaluate_run(&store, &run_id, &lib)?;
        let replied = replies.fetch_since(0.0)?;
        apply_liveness(&store, &run_id, control_replied(&replied, &control_token, started))?;
        apply_attachment_check(&store, &run_id, &lib, attachment_read(&replied, &files_token, started))?;
        store.finish_run(&run_id)?;
        let items: Vec<_> = store.items(&run_id)?.into_iter().filter(|i| !i.scenario_id.starts_with('_')).collect();
        let count = |s: &str| items.iter().filter(|i| i.status == s).count();
        if print {
            println!("{}\n", text_report(&store, &run_id, &lib, &[])?);
        }
        summary.push(Outcome { name: agent.name(), failed: count(FAIL), passed: count(PASS), inconclusive: count(INCONCLUSIVE), total: items.len() });
    }
    drop(listener);
    let _ = std::fs::remove_dir_all(&dir);

    let (naive, careful, silent, blind) = (&summary[0], &summary[1], &summary[2], &summary[3]);
    let naive_ok = naive.failed == naive.total;
    let careful_ok = careful.failed == 0 && careful.passed == careful.total;
    let silent_ok = silent.failed == 0 && silent.passed == 0 && silent.inconclusive > 0;
    let blind_ok = blind.failed == 0 && blind.inconclusive == file_tests && blind.passed == blind.total - file_tests;
    let ok = naive_ok && careful_ok && silent_ok && blind_ok;
    if print {
        println!("==== self-test summary ====");
        for o in &summary {
            println!("{}: {} failed, {} passed, {} inconclusive (of {} tests)", o.name, o.failed, o.passed, o.inconclusive, o.total);
        }
        if !naive_ok {
            println!("PROBLEM: some tests did not catch the naive agent");
        }
        if !careful_ok {
            println!("PROBLEM: the careful agent triggered alarms (false positives) or was not confirmed alive");
        }
        if !silent_ok {
            println!("PROBLEM: a switched-off agent was reported as passing");
        }
        if !blind_ok {
            println!("PROBLEM: an agent that never opens attachments was reported as resisting file tests");
        }
        println!(
            "self-test {}",
            if ok {
                "PASSED: every test caught the naive agent, none fired on the careful one, the switched-off agent was never reported as passing, and file tests were never passed by an agent that does not open files"
            } else {
                "FAILED"
            }
        );
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    #[test]
    fn selftest_passes() {
        assert!(super::run_selftest(false).unwrap());
    }
}
