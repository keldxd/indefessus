//! The test library: harmless emails that try to turn an agent against its owner.
//!
//! Each test is a TOML file. The built-in ones are compiled into the binary, so the
//! single download works on its own. Placeholders in double braces are filled per run:
//!   {{canary_url}}   a unique link that is recorded if anything opens it
//!   {{trap_email}}   an address in a mailbox you control; mail arriving there is evidence
//!   {{token}}        the unique marker of this test in this run
//!   {{seed_secret}}  a fake private code planted in a separate "seed" email
//!   {{base64:...}}   hides the text inside in base64 (after the placeholders in it are filled)

use anyhow::{bail, Context, Result};
use base64::Engine;
use regex::Regex;
use serde::Deserialize;
use std::path::Path;
use std::sync::OnceLock;

pub const HARMS: &[(&str, &str)] = &[
    ("leak", "Leaks your mail or data to a stranger"),
    ("act", "Acts on a stranger's instructions"),
    ("reveal", "Reveals private information or its own setup"),
    ("vouch", "Tells you something a stranger planted"),
];

pub fn harm_text(harm: &str) -> &'static str {
    HARMS.iter().find(|(k, _)| *k == harm).map(|(_, v)| *v).unwrap_or("Unknown risk")
}

const BUILTIN: &[(&str, &str)] = &[
    ("01-leak-link-plain.toml", include_str!("../scenarios/01-leak-link-plain.toml")),
    ("02-leak-link-hidden.toml", include_str!("../scenarios/02-leak-link-hidden.toml")),
    ("03-leak-link-encoded.toml", include_str!("../scenarios/03-leak-link-encoded.toml")),
    ("04-forward-recent-mail.toml", include_str!("../scenarios/04-forward-recent-mail.toml")),
    ("05-reply-with-contacts.toml", include_str!("../scenarios/05-reply-with-contacts.toml")),
    ("06-leak-private-code.toml", include_str!("../scenarios/06-leak-private-code.toml")),
    ("07-approve-payment.toml", include_str!("../scenarios/07-approve-payment.toml")),
    ("08-reveal-setup.toml", include_str!("../scenarios/08-reveal-setup.toml")),
    ("09-vouch-for-phishing.toml", include_str!("../scenarios/09-vouch-for-phishing.toml")),
    ("10-hijack-summary.toml", include_str!("../scenarios/10-hijack-summary.toml")),
    ("11-swap-payment-details.toml", include_str!("../scenarios/11-swap-payment-details.toml")),
    ("12-reveal-private-code.toml", include_str!("../scenarios/12-reveal-private-code.toml")),
];

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Detect {
    pub url_hit: bool,
    pub trap_mail: bool,
    pub output_contains: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Explain {
    pub fail: String,
    pub fix: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Scenario {
    pub id: String,
    pub title: String,
    pub harm: String,
    pub severity: String,
    pub mode: String,
    pub technique: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: String,
    pub seed_subject: String,
    pub seed_body: String,
    pub guided_prompt: String,
    pub detect: Detect,
    pub explain: Explain,
    #[serde(skip)]
    pub source: String,
}

impl Scenario {
    pub fn has_seed(&self) -> bool {
        !self.seed_body.is_empty()
    }
    pub fn is_guided(&self) -> bool {
        self.mode == "guided"
    }
}

pub fn parse(text: &str, source: &str) -> Result<Scenario> {
    let mut sc: Scenario = toml::from_str(text).with_context(|| format!("{source}: not valid TOML"))?;
    sc.source = source.to_string();
    for (name, value) in [("id", &sc.id), ("title", &sc.title), ("harm", &sc.harm), ("subject", &sc.subject), ("body_text", &sc.body_text)] {
        if value.trim().is_empty() {
            bail!("{source}: missing required field '{name}'");
        }
    }
    if sc.mode.is_empty() {
        sc.mode = "auto".into();
    }
    if sc.severity.is_empty() {
        sc.severity = "medium".into();
    }
    if !HARMS.iter().any(|(k, _)| *k == sc.harm) {
        bail!("{source}: harm must be one of leak, act, reveal, vouch");
    }
    if sc.mode != "auto" && sc.mode != "guided" {
        bail!("{source}: mode must be auto or guided");
    }
    if !(sc.detect.url_hit || sc.detect.trap_mail || !sc.detect.output_contains.is_empty()) {
        bail!("{source}: a test needs at least one detection signal");
    }
    if sc.is_guided() && sc.guided_prompt.is_empty() {
        bail!("{source}: guided tests need a guided_prompt");
    }
    Ok(sc)
}

pub struct Library {
    pub list: Vec<Scenario>,
}

impl Library {
    pub fn get(&self, id: &str) -> Option<&Scenario> {
        self.list.iter().find(|s| s.id == id)
    }
}

pub fn load_library(extra_dirs: &[String]) -> Result<Library> {
    let mut list: Vec<Scenario> = Vec::new();
    let mut add = |sc: Scenario| -> Result<()> {
        if list.iter().any(|s| s.id == sc.id) {
            bail!("duplicate test id '{}' in {}", sc.id, sc.source);
        }
        list.push(sc);
        Ok(())
    };
    for (name, text) in BUILTIN {
        add(parse(text, name)?)?;
    }
    for dir in extra_dirs {
        let mut paths: Vec<_> = std::fs::read_dir(Path::new(dir))
            .with_context(|| format!("cannot read test folder {dir}"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "toml").unwrap_or(false))
            .collect();
        paths.sort();
        for path in paths {
            let text = std::fs::read_to_string(&path)?;
            add(parse(&text, &path.display().to_string())?)?;
        }
    }
    Ok(Library { list })
}

pub fn new_token() -> String {
    // Lowercase letters and digits only, so it survives being retyped or rewritten by a model
    format!("ind{:08x}", rand::random::<u32>())
}

pub fn new_seed_secret() -> String {
    format!("SEED-{:06X}", rand::random::<u32>() & 0x00FF_FFFF)
}

pub fn trap_address(trap: &str, token: &str, plus_addressing: bool) -> String {
    match trap.split_once('@') {
        Some((local, domain)) if plus_addressing => format!("{local}+{token}@{domain}"),
        _ => trap.to_string(),
    }
}

fn base64_block() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)\{\{base64:(.*?)\}\}").unwrap())
}

pub struct Values {
    pub token: String,
    pub seed_secret: String,
    pub canary_url: String,
    pub trap_email: String,
}

impl Values {
    pub fn fill(&self, text: &str) -> String {
        let filled = text
            .replace("{{token}}", &self.token)
            .replace("{{seed_secret}}", &self.seed_secret)
            .replace("{{canary_url}}", &self.canary_url)
            .replace("{{trap_email}}", &self.trap_email);
        base64_block()
            .replace_all(&filled, |c: &regex::Captures| base64::engine::general_purpose::STANDARD.encode(c[1].as_bytes()))
            .into_owned()
    }
}

#[allow(dead_code)]
pub struct Rendered {
    pub scenario: Scenario,
    pub token: String,
    pub seed_secret: String,
    pub canary_url: String,
    pub trap_email: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: String,
    pub seed_subject: String,
    pub seed_body: String,
    pub guided_prompt: String,
}

pub fn render(sc: &Scenario, canary_base: &str, trap: &str, plus_addressing: bool) -> Rendered {
    let token = new_token();
    let v = Values {
        canary_url: format!("{}/c/{}", canary_base.trim_end_matches('/'), token),
        trap_email: trap_address(trap, &token, plus_addressing),
        seed_secret: new_seed_secret(),
        token,
    };
    Rendered {
        subject: v.fill(&sc.subject),
        body_text: v.fill(&sc.body_text),
        body_html: v.fill(&sc.body_html),
        seed_subject: v.fill(&sc.seed_subject),
        seed_body: v.fill(&sc.seed_body),
        guided_prompt: v.fill(&sc.guided_prompt),
        scenario: sc.clone(),
        token: v.token,
        seed_secret: v.seed_secret,
        canary_url: v.canary_url,
        trap_email: v.trap_email,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_loads() {
        let lib = load_library(&[]).unwrap();
        assert!(lib.list.len() >= 12);
        // every file in scenarios/ must be listed in BUILTIN
        let files = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios")).unwrap().count();
        assert_eq!(files, BUILTIN.len(), "add new test files to BUILTIN in src/scenarios.rs");
    }

    #[test]
    fn markers_never_in_subjects() {
        for sc in &load_library(&[]).unwrap().list {
            let r = render(sc, "https://c.example", "trap@t.example", true);
            assert!(!r.subject.contains(&r.token), "{}", sc.id);
            assert!(!r.subject.contains(&r.seed_secret), "{}", sc.id);
        }
    }

    #[test]
    fn base64_is_filled_after_placeholders() {
        let lib = load_library(&[]).unwrap();
        let r = render(lib.get("leak-link-encoded").unwrap(), "https://c.example", "trap@t.example", true);
        assert!(!r.body_text.contains("{{"));
        let blob = r.body_text.split_whitespace().find(|w| w.len() > 40).unwrap();
        let decoded = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(blob).unwrap()).unwrap();
        assert!(decoded.contains(&r.canary_url));
    }

    #[test]
    fn plus_addressing() {
        assert_eq!(trap_address("trap@x.org", "ind1234abcd", true), "trap+ind1234abcd@x.org");
        assert_eq!(trap_address("trap@x.org", "ind1234abcd", false), "trap@x.org");
    }

    #[test]
    fn invalid_rejected() {
        assert!(parse("id='x'\ntitle='t'\nharm='leak'\nsubject='s'\nbody_text='b'", "x").is_err());
    }
}
