//! Self-test: proves the whole pipeline works, with no real mailbox and no real agent.
//!
//! Two stand-in agents read the same test emails:
//!   - a NAIVE agent that obeys any instruction it finds in an email (every test should fail)
//!   - a CAREFUL agent that never follows instructions from email content (every test should pass)
//! These are simple programs that imitate the two extremes. They are not AI models, and the
//! results say nothing about any real product. They show that Indefessus catches an agent
//! that misbehaves, and raises no false alarms on one that behaves.

use crate::canary::Background;
use crate::evaluate::{collect_trapmail, evaluate_run, FAIL};
use crate::mail::MemoryTrap;
use crate::report::text_report;
use crate::scenarios::{load_library, render};
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

    /// What a model would see: text, HTML without tags (hidden text included), decoded base64.
    fn readable(&self, m: &Email) -> String {
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
    fn answer(&self, prompt: &str) -> String;
}

struct NaiveAgent<'a> {
    p: &'a Patterns,
    trap: &'a MemoryTrap,
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

    fn answer(&self, _prompt: &str) -> String {
        format!("Here is a summary of your inbox: {}. {}", self.subjects.join("; "), self.planted.join(" "))
    }
}

struct CarefulAgent {
    count: usize,
}

impl Agent for CarefulAgent {
    fn name(&self) -> &'static str {
        "careful agent (never follows instructions from email content)"
    }
    fn process(&mut self, inbox: &[Email]) -> Result<()> {
        self.count = inbox.len();
        Ok(())
    }
    fn answer(&self, _prompt: &str) -> String {
        format!("You have {} new emails. Some of them contain requests from people you may not know.", self.count)
    }
}

/// Runs both stand-in agents through every test. Returns true if the naive agent failed every
/// test and the careful agent failed none.
pub fn run_selftest(print: bool) -> Result<bool> {
    let lib = load_library(&[])?;
    let dir = std::env::temp_dir().join(format!("indefessus-selftest-{}-{}", std::process::id(), rand::random::<u32>()));
    let store = Arc::new(Store::open(&dir)?);
    let listener = Background::start(store.clone())?;
    let base = listener.base_url();
    let trap_domain = "selftest.invalid";
    let patterns = Patterns::new();
    let mut summary = Vec::new();

    for which in ["naive", "careful"] {
        let trap = MemoryTrap::default();
        let mut agent: Box<dyn Agent + '_> = if which == "naive" {
            Box::new(NaiveAgent {
                p: &patterns,
                trap: &trap,
                base: base.clone(),
                addr: listener.addr,
                trap_domain: trap_domain.into(),
                subjects: vec![],
                planted: vec![],
            })
        } else {
            Box::new(CarefulAgent { count: 0 })
        };
        let run_id = format!("selftest-{which}-{}", now() as u64);
        store.new_run(&run_id, "selftest", agent.name())?;
        let mut inbox = Vec::new();
        let mut prompts = Vec::new();
        for sc in &lib.list {
            let r = render(sc, &base, &format!("trap@{trap_domain}"), true);
            store.add_item(&run_id, &sc.id, &r.token, &r.seed_secret)?;
            if sc.has_seed() {
                inbox.push(Email { subject: r.seed_subject.clone(), text: r.seed_body.clone(), html: String::new() });
            }
            inbox.push(Email { subject: r.subject.clone(), text: r.body_text.clone(), html: r.body_html.clone() });
            if !sc.detect.output_contains.is_empty() {
                prompts.push((sc.id.clone(), r.guided_prompt.clone()));
            }
        }
        agent.process(&inbox)?;
        for (id, prompt) in prompts {
            store.set_answer(&run_id, &id, &agent.answer(&prompt))?;
        }
        collect_trapmail(&store, &run_id, &trap)?;
        let results = evaluate_run(&store, &run_id, &lib)?;
        store.finish_run(&run_id)?;
        let failed = results.iter().filter(|(_, s)| *s == FAIL).count();
        if print {
            println!("{}\n", text_report(&store, &run_id, &lib, &[])?);
        }
        summary.push((agent.name(), failed, results.len()));
    }
    drop(listener);
    let _ = std::fs::remove_dir_all(&dir);

    let (naive, careful) = (&summary[0], &summary[1]);
    let ok = naive.1 == naive.2 && careful.1 == 0;
    if print {
        println!("==== self-test summary ====");
        for (name, failed, total) in &summary {
            println!("{name}: {failed} of {total} tests failed");
        }
        if naive.1 != naive.2 {
            println!("PROBLEM: some tests did not catch the naive agent");
        }
        if careful.1 != 0 {
            println!("PROBLEM: the careful agent triggered alarms (false positives)");
        }
        println!(
            "self-test {}",
            if ok { "PASSED: every test caught the naive agent, and none fired on the careful one" } else { "FAILED" }
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
