//! Plain-language reports, for people who are not security experts.

use crate::clock::fmt_local as fmt_time;
use crate::evaluate::{liveness, Change, Details, FAIL, INCONCLUSIVE, NEEDS_ANSWER, PASS};
use crate::scenarios::{harm_text, Library, Scenario};
use crate::store::Store;
use crate::util::html_escape as e;

const ALIVE_TEXT: &str = "Agent check: your agent replied to an ordinary email during this run, so it was switched on and working.";
const DEAD_TEXT: &str = "Agent check: your agent did NOT reply to an ordinary email during this run. It may have been switched off, \
broken, overloaded or out of quota, so tests without evidence are marked inconclusive, not passed. Check the agent is working, then run again.";
use anyhow::{anyhow, Result};

fn label(status: &str) -> &'static str {
    match status {
        FAIL => "FAILED",
        PASS => "passed",
        NEEDS_ANSWER => "waiting for your answer",
        INCONCLUSIVE => "inconclusive",
        _ => "not checked yet",
    }
}

fn rows<'a>(store: &Store, run_id: &str, lib: &'a Library) -> Result<Vec<(&'a Scenario, String, Details)>> {
    let order = |s: &str| match s {
        FAIL => 0,
        NEEDS_ANSWER => 1,
        INCONCLUSIVE => 2,
        PASS => 4,
        _ => 3,
    };
    let mut rows: Vec<_> = store
        .items(run_id)?
        .into_iter()
        .filter_map(|i| lib.get(&i.scenario_id).map(|sc| (sc, i.status.clone(), serde_json::from_str(&i.details).unwrap_or_default())))
        .collect();
    rows.sort_by(|a, b| order(a.1.as_str()).cmp(&order(b.1.as_str())).then(a.0.id.cmp(&b.0.id)));
    Ok(rows)
}

pub fn text_report(store: &Store, run_id: &str, lib: &Library, changes: &[Change]) -> Result<String> {
    let run = store.run(run_id)?.ok_or_else(|| anyhow!("unknown run {run_id}"))?;
    let rows = rows(store, run_id, lib)?;
    let count = |s: &str| rows.iter().filter(|r| r.1 == s).count();
    let mut out = vec![
        format!("==== Indefessus report: run {} ({}, started {}) ====", run.id, run.kind, fmt_time(run.started_at)),
        format!(
            "tests: {}   failed: {}   passed: {}   inconclusive: {}   waiting for your answer: {}",
            rows.len(),
            count(FAIL),
            count(PASS),
            count(INCONCLUSIVE),
            count(NEEDS_ANSWER)
        ),
    ];
    match liveness(store, run_id)? {
        Some(true) => out.push(ALIVE_TEXT.into()),
        Some(false) => out.push(DEAD_TEXT.into()),
        None => {}
    }
    if !changes.is_empty() {
        out.push(String::new());
        out.push("!! BEHAVIOUR CHANGED SINCE THE LAST RUN !!".into());
        for c in changes {
            let dir = if c.worse { "got WORSE" } else { "got better" };
            out.push(format!("   {}: {} ({} -> {})", c.scenario, dir, c.before, c.now));
        }
    }
    let failed: Vec<_> = rows.iter().filter(|r| r.1 == FAIL).collect();
    if !failed.is_empty() {
        out.push(String::new());
        out.push("What went wrong:".into());
        for (sc, _, d) in failed {
            out.push(String::new());
            out.push(format!("  [{}] {}", sc.severity.to_uppercase(), sc.title));
            out.push(format!("    Risk: {}", harm_text(&sc.harm)));
            if d.leaked_private_data {
                out.push("    Private data from another email reached the stranger.".into());
            }
            for ev in &d.evidence {
                let when = ev.at.map(fmt_time).unwrap_or_default();
                match ev.kind.as_str() {
                    "link-opened" => match ev.data_sent.as_deref().filter(|s| !s.is_empty()) {
                        Some(data) => {
                            let short: String = data.chars().take(120).collect();
                            out.push(format!("    Evidence: the agent opened the stranger's link at {when}, sending: {short}"));
                        }
                        None => out.push(format!("    Evidence: the agent opened the stranger's link at {when} (it attached no data this time)")),
                    },
                    "mail-sent-to-stranger" => {
                        out.push(format!("    Evidence: mail reached the stranger's address at {when}: \"{}\"", ev.subject.as_deref().unwrap_or("")));
                        if let Some(text) = ev.excerpt.as_deref() {
                            out.push(format!("      What the agent wrote: \"{text}\""));
                        }
                    }
                    "planted-text-in-answer" => out.push(format!(
                        "    Evidence: the agent repeated text planted by the stranger: \"{}\"",
                        ev.marker.as_deref().unwrap_or("")
                    )),
                    _ => {}
                }
            }
            if !sc.explain.fail.is_empty() {
                out.push(format!("    What it means: {}", sc.explain.fail.trim()));
            }
            if !sc.explain.fix.is_empty() {
                out.push(format!("    What to do: {}", sc.explain.fix.trim()));
            }
        }
    }
    let waiting: Vec<_> = rows.iter().filter(|r| r.1 == NEEDS_ANSWER).collect();
    if !waiting.is_empty() {
        out.push(String::new());
        out.push("Waiting for you (guided tests). Ask your assistant, then record its answer with".into());
        out.push("  indefessus answer <test id>".into());
        for (sc, _, _) in waiting {
            out.push(format!("  - {}: {}", sc.id, sc.title));
        }
    }
    let inconclusive: Vec<_> = rows.iter().filter(|r| r.1 == INCONCLUSIVE).collect();
    if !inconclusive.is_empty() {
        out.push(String::new());
        out.push("Inconclusive (no evidence, but the agent was not confirmed to be working):".into());
        for (sc, _, _) in inconclusive {
            out.push(format!("  - {}", sc.title));
        }
    }
    let passed: Vec<_> = rows.iter().filter(|r| r.1 == PASS).collect();
    if !passed.is_empty() {
        out.push(String::new());
        out.push("Passed (no sign the agent obeyed the stranger):".into());
        for (sc, _, _) in passed {
            out.push(format!("  - {}", sc.title));
        }
    }
    out.push(String::new());
    out.push("A pass means this agent resisted these tests today. It does not prove the agent is safe.".into());
    out.push("AI agents do not always behave the same way: the same agent can obey a test in one run and refuse it in the next. Run the tests more than once before drawing conclusions.".into());
    Ok(out.join("\n"))
}

pub fn html_report(store: &Store, run_id: &str, lib: &Library, changes: &[Change]) -> Result<String> {
    let run = store.run(run_id)?.ok_or_else(|| anyhow!("unknown run {run_id}"))?;
    let agent_html = match liveness(store, run_id)? {
        Some(true) => format!("<p>{}</p>", e(ALIVE_TEXT)),
        Some(false) => format!("<div class='alert'><p><b>{}</b></p></div>", e(DEAD_TEXT)),
        None => String::new(),
    };
    let mut cards = String::new();
    for (sc, status, d) in rows(store, run_id, lib)? {
        let mut ev = String::new();
        for x in &d.evidence {
            let what = match (&x.subject, &x.excerpt) {
                (Some(s), Some(t)) => format!("\"{s}\": {t}"),
                _ => x.data_sent.clone().or(x.subject.clone()).or(x.marker.clone()).unwrap_or_default(),
            };
            let what = what.as_str();
            ev.push_str(&format!(
                "<li>{}{}{}</li>",
                e(&x.kind.replace('-', " ")),
                x.at.map(|t| format!(" at {}", e(&fmt_time(t)))).unwrap_or_default(),
                if what.is_empty() { String::new() } else { format!(": {}", e(what)) }
            ));
        }
        cards.push_str(&format!(
            "<section class='{}'><h3>{} <span>{}</span></h3><p class='meta'>{} &middot; severity {} &middot; test <code>{}</code></p>{}{}{}</section>\n",
            e(&status),
            e(&sc.title),
            label(&status),
            e(harm_text(&sc.harm)),
            e(&sc.severity),
            e(&sc.id),
            if d.leaked_private_data { "<p><b>Private data from another email reached the stranger.</b></p>" } else { "" },
            if ev.is_empty() { String::new() } else { format!("<ul>{ev}</ul>") },
            if status == FAIL {
                format!("<p>{}</p><p><b>What to do:</b> {}</p>", e(sc.explain.fail.trim()), e(sc.explain.fix.trim()))
            } else {
                String::new()
            }
        ));
    }
    let change_html = if changes.is_empty() {
        String::new()
    } else {
        let items: String = changes
            .iter()
            .map(|c| {
                format!(
                    "<li><code>{}</code> got {} ({} &rarr; {})</li>",
                    e(&c.scenario),
                    if c.worse { "<b>worse</b>" } else { "better" },
                    e(&c.before),
                    e(&c.now)
                )
            })
            .collect();
        format!("<div class='alert'><h2>Behaviour changed since the last run</h2><ul>{items}</ul></div>")
    };
    Ok(format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Indefessus report {id}</title>
<style>
body{{font-family:system-ui,sans-serif;max-width:52rem;margin:2rem auto;padding:0 1rem;line-height:1.5;color:#1d1d1f;background:#fff}}
section{{border:1px solid #ddd;border-left:6px solid #999;border-radius:6px;padding:.6rem 1rem;margin:.8rem 0}}
section.fail{{border-left-color:#c62828}} section.pass{{border-left-color:#2e7d32}} section.needs-answer{{border-left-color:#f9a825}} section.inconclusive{{border-left-color:#757575}}
h3 span{{font-size:.8rem;font-weight:600;padding:.1rem .5rem;border-radius:1rem;background:#eee;margin-left:.4rem}}
.meta{{color:#555;font-size:.9rem;margin:.2rem 0}} .alert{{background:#fff3e0;border:1px solid #ffb74d;padding:.5rem 1rem;border-radius:6px}}
</style></head><body>
<h1>Indefessus report</h1>
<p>Run <code>{id}</code> ({kind}), started {when}.</p>
{agent_html}
{change_html}
{cards}
<p><small>A pass means this agent resisted these tests today. It does not prove the agent is safe. AI agents do not always behave the same way, so run the tests more than once before drawing conclusions.</small></p>
</body></html>"#,
        id = e(&run.id),
        kind = e(&run.kind),
        when = e(&fmt_time(run.started_at)),
    ))
}
