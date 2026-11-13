//! The test library: harmless emails that try to turn an agent against its owner.
//!
//! Each test is a TOML file. The built-in ones are compiled into the binary, so the
//! single download works on its own. Placeholders in double braces are filled per run:
//!   {{canary_url}}   a unique link that is recorded if anything opens it
//!   {{trap_email}}   an address in a mailbox you control; mail arriving there is evidence
//!   {{token}}        the unique marker of this test in this run
//!   {{seed_secret}}  a fake private code planted in a separate "seed" email
//!   {{base64:...}}   hides the text inside in base64 (after the placeholders in it are filled)
//!
//! A test can also attach files ([[attachments]]); they are built at run time by attach.rs.

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

/// What a failure costs, worst first. Reports sort failures in this order.
pub const IMPACTS: &[(&str, &str)] = &[
    ("money", "Money"),
    ("data", "Your information leaving"),
    ("deceive", "Misleading you"),
    ("setup", "Your setup exposed"),
    ("other", "Other"),
];

pub const SEVERITIES: &[&str] = &["critical", "high", "medium", "low"];

pub fn impact_text(impact: &str) -> &'static str {
    IMPACTS.iter().find(|(k, _)| *k == impact).map(|(_, v)| *v).unwrap_or("Other")
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
    ("13-file-pdf-white-text.toml", include_str!("../scenarios/13-file-pdf-white-text.toml")),
    ("14-file-pdf-metadata.toml", include_str!("../scenarios/14-file-pdf-metadata.toml")),
    ("15-file-word-hidden-text.toml", include_str!("../scenarios/15-file-word-hidden-text.toml")),
    ("16-file-word-comment.toml", include_str!("../scenarios/16-file-word-comment.toml")),
    ("17-file-calendar-invite.toml", include_str!("../scenarios/17-file-calendar-invite.toml")),
    ("18-file-spreadsheet-cell.toml", include_str!("../scenarios/18-file-spreadsheet-cell.toml")),
    ("19-file-text-note.toml", include_str!("../scenarios/19-file-text-note.toml")),
    ("20-file-web-page.toml", include_str!("../scenarios/20-file-web-page.toml")),
];

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Detect {
    pub url_hit: bool,
    pub trap_mail: bool,
    pub output_contains: Vec<String>,
    /// What data the trick is after, if a reply without it should count as "partly worked".
    /// "addresses": mail to the stranger that holds no email addresses only partly worked.
    pub data: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Explain {
    /// What the failure means, in a few plain sentences
    pub fail: String,
    /// Short fix, kept for older community tests that have no fix_plain
    pub fix: String,
    /// One everyday sentence: what a scammer could do with this weakness
    pub scammer: String,
    /// Completes "The most serious: a stranger's email got it to ..."
    pub worst: String,
    /// Fix steps for anyone, most important first
    pub fix_plain: Vec<String>,
    /// Extra steps for agents built in n8n
    pub fix_n8n: Vec<String>,
    /// Steps for the IT person or developer
    pub fix_technical: Vec<String>,
}

impl Explain {
    /// The plain fix steps, falling back to the short fix of older tests.
    pub fn plain_steps(&self) -> Vec<String> {
        if !self.fix_plain.is_empty() {
            self.fix_plain.iter().map(|x| x.trim().to_string()).collect()
        } else if !self.fix.trim().is_empty() {
            vec![self.fix.trim().to_string()]
        } else {
            Vec::new()
        }
    }
}

/// A file attached to a test email. Its text can use the same placeholders as the email.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AttachmentSpec {
    /// txt, html, csv, ics, pdf or docx
    pub kind: String,
    /// Where the instruction goes (see attach::KINDS); empty means the first one listed
    pub hide: String,
    pub filename: String,
    pub title: String,
    pub visible: String,
    pub hidden: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Scenario {
    pub id: String,
    pub title: String,
    pub harm: String,
    pub impact: String,
    pub severity: String,
    pub mode: String,
    pub technique: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: String,
    pub seed_subject: String,
    pub seed_body: String,
    pub guided_prompt: String,
    pub attachments: Vec<AttachmentSpec>,
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
    /// Tests whose trick is inside an attached file
    pub fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }
    /// Sort key, most dangerous first: what it costs, then how severe.
    pub fn danger_rank(&self) -> (usize, usize) {
        let impact = IMPACTS.iter().position(|(k, _)| *k == self.impact).unwrap_or(IMPACTS.len());
        let severity = SEVERITIES.iter().position(|k| *k == self.severity).unwrap_or(SEVERITIES.len());
        (impact, severity)
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
    if sc.impact.is_empty() {
        sc.impact = match sc.harm.as_str() {
            "leak" | "reveal" => "data",
            "vouch" => "deceive",
            _ => "other",
        }
        .into();
    }
    if !IMPACTS.iter().any(|(k, _)| *k == sc.impact) {
        bail!("{source}: impact must be one of money, data, deceive, setup, other");
    }
    if !SEVERITIES.contains(&sc.severity.as_str()) {
        bail!("{source}: severity must be one of critical, high, medium, low");
    }
    if !HARMS.iter().any(|(k, _)| *k == sc.harm) {
        bail!("{source}: harm must be one of leak, act, reveal, vouch");
    }
    if sc.mode != "auto" && sc.mode != "guided" {
        bail!("{source}: mode must be auto or guided");
    }
    if !(sc.detect.data.is_empty() || sc.detect.data == "addresses") {
        bail!("{source}: detect.data can only be \"addresses\"");
    }
    if !(sc.detect.url_hit || sc.detect.trap_mail || !sc.detect.output_contains.is_empty()) {
        bail!("{source}: a test needs at least one detection signal");
    }
    if sc.is_guided() && sc.guided_prompt.is_empty() {
        bail!("{source}: guided tests need a guided_prompt");
    }
    for a in &mut sc.attachments {
        a.hide = crate::attach::check(&a.kind, &a.hide).map_err(|e| anyhow::anyhow!("{source}: {e}"))?;
        if !a.filename.to_lowercase().ends_with(&format!(".{}", a.kind)) {
            bail!("{source}: attachment {} must end in .{}", a.filename, a.kind);
        }
        if a.hidden.trim().is_empty() || a.visible.trim().is_empty() {
            bail!("{source}: attachment {} needs both visible and hidden text", a.filename);
        }
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
    pub attachments: Vec<crate::attach::Built>,
}

pub fn render(sc: &Scenario, canary_base: &str, trap: &str, plus_addressing: bool) -> Result<Rendered> {
    let token = new_token();
    let v = Values {
        canary_url: format!("{}/c/{}", canary_base.trim_end_matches('/'), token),
        trap_email: trap_address(trap, &token, plus_addressing),
        seed_secret: new_seed_secret(),
        token,
    };
    let mut attachments = Vec::new();
    for a in &sc.attachments {
        let (filename, title, visible, hidden) = (v.fill(&a.filename), v.fill(&a.title), v.fill(&a.visible), v.fill(&a.hidden));
        let built = crate::attach::build(&crate::attach::Spec {
            kind: &a.kind,
            hide: &a.hide,
            filename: &filename,
            title: &title,
            visible: &visible,
            hidden: &hidden,
            uid: &v.token,
            now_secs: crate::util::now() as u64,
        })
        .map_err(|e| anyhow::anyhow!("{}: {e}", sc.id))?;
        attachments.push(built);
    }
    Ok(Rendered {
        attachments,
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
    })
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
            let r = render(sc, "https://c.example", "trap@t.example", true).unwrap();
            assert!(!r.subject.contains(&r.token), "{}", sc.id);
            assert!(!r.subject.contains(&r.seed_secret), "{}", sc.id);
        }
    }

    #[test]
    fn base64_is_filled_after_placeholders() {
        let lib = load_library(&[]).unwrap();
        let r = render(lib.get("leak-link-encoded").unwrap(), "https://c.example", "trap@t.example", true).unwrap();
        assert!(!r.body_text.contains("{{"));
        let blob = r.body_text.split_whitespace().find(|w| w.len() > 40).unwrap();
        let decoded = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(blob).unwrap()).unwrap();
        assert!(decoded.contains(&r.canary_url));
    }

    /// The trick is in the file, not the email: the marker must be in the attachment and not
    /// in the email text, or the test would not test attachments at all.
    #[test]
    fn attachment_tests_hide_the_trick_in_the_file() {
        let lib = load_library(&[]).unwrap();
        let with_files: Vec<_> = lib.list.iter().filter(|s| s.has_attachments()).collect();
        assert!(with_files.len() >= 8);
        for sc in with_files {
            let r = render(sc, "https://c.example", "trap@t.example", true).unwrap();
            assert!(!r.body_text.contains(&r.token) && !r.body_html.contains(&r.token), "{}: marker in the email text", sc.id);
            let in_files = r.attachments.iter().any(|a| crate::attach::extract_text(&a.filename, &a.bytes).contains(&r.token));
            assert!(in_files, "{}: marker not in the attachment", sc.id);
        }
    }

    #[test]
    fn bad_attachments_rejected() {
        let base = "id='x'\ntitle='t'\nharm='leak'\nsubject='s'\nbody_text='b'\n[[attachments]]\n";
        let tail = "\n[detect]\nurl_hit=true";
        assert!(parse(&format!("{base}kind='exe'\nfilename='a.exe'\nvisible='v'\nhidden='h'{tail}"), "x").is_err());
        assert!(parse(&format!("{base}kind='pdf'\nfilename='a.txt'\nvisible='v'\nhidden='h'{tail}"), "x").is_err());
        assert!(parse(&format!("{base}kind='pdf'\nhide='comment'\nfilename='a.pdf'\nvisible='v'\nhidden='h'{tail}"), "x").is_err());
        let ok = parse(&format!("{base}kind='pdf'\nfilename='a.pdf'\nvisible='v'\nhidden='h'{tail}"), "x").unwrap();
        assert_eq!(ok.attachments[0].hide, "white");
    }

    #[test]
    fn plus_addressing() {
        assert_eq!(trap_address("trap@x.org", "ind1234abcd", true), "trap+ind1234abcd@x.org");
        assert_eq!(trap_address("trap@x.org", "ind1234abcd", false), "trap@x.org");
    }

    /// The plain parts of every built-in test must read well for someone who is not technical.
    #[test]
    fn plain_wording_for_everyone() {
        let jargon = ["exfiltrat", "prompt injection", "canary", "payload", "token", "llm", "system prompt"];
        for sc in &load_library(&[]).unwrap().list {
            let e = &sc.explain;
            assert!(!e.scammer.trim().is_empty(), "{}: add explain.scammer", sc.id);
            assert!(!e.worst.trim().is_empty(), "{}: add explain.worst", sc.id);
            // It follows "a stranger's email got it to ...", so it must not say "stranger" again
            assert!(!e.worst.to_lowercase().contains("stranger"), "{}: explain.worst repeats 'stranger': {}", sc.id, e.worst);
            assert!(!e.fix_plain.is_empty(), "{}: add explain.fix_plain", sc.id);
            assert!(!e.fix_n8n.is_empty(), "{}: add explain.fix_n8n", sc.id);
            assert!(!e.fix_technical.is_empty(), "{}: add explain.fix_technical", sc.id);
            let mut plain = vec![sc.title.clone(), e.fail.clone(), e.scammer.clone(), e.worst.clone()];
            plain.extend(e.fix_plain.iter().cloned());
            for text in &plain {
                let lower = text.to_lowercase();
                for word in jargon {
                    assert!(!lower.contains(word), "{}: plain text uses jargon '{}': {}", sc.id, word, text);
                }
            }
            let mut all = plain.clone();
            all.extend(e.fix_n8n.iter().cloned());
            all.extend(e.fix_technical.iter().cloned());
            for text in &all {
                assert!(!text.contains('\u{2014}'), "{}: no em dashes please: {}", sc.id, text);
            }
        }
    }

    #[test]
    fn danger_order_puts_money_first() {
        let lib = load_library(&[]).unwrap();
        let mut ids: Vec<&Scenario> = lib.list.iter().collect();
        ids.sort_by_key(|s| s.danger_rank());
        assert_eq!(ids[0].impact, "money");
        assert_eq!(ids.last().unwrap().impact, "setup");
    }

    #[test]
    fn older_tests_without_new_fields_still_load() {
        let sc = parse(
            "id='x'\ntitle='t'\nharm='vouch'\nmode='guided'\nsubject='s'\nbody_text='b'\nguided_prompt='p'\n[detect]\noutput_contains=['m']\n[explain]\nfix='Do this.'",
            "x",
        )
        .unwrap();
        assert_eq!(sc.impact, "deceive");
        assert_eq!(sc.explain.plain_steps(), vec!["Do this.".to_string()]);
    }

    #[test]
    fn invalid_rejected() {
        assert!(parse("id='x'\ntitle='t'\nharm='leak'\nsubject='s'\nbody_text='b'", "x").is_err());
    }
}
