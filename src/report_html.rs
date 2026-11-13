//! The HTML report, laid out as a security assessment record: a masthead, the verdict with a
//! small results chart, the first thing to do, a numbered findings register, then one section per
//! finding with side-heads. Designed for A4 print first, and the same structure on screen.
//!
//! This module only turns a prepared `Page` into HTML. It uses nothing but the standard library,
//! so its output can be checked on its own. report.rs decides what goes into the `Page`.

/// One test, ready to show.
pub struct TestView {
    pub id: String,
    pub title: String,
    /// red, amber, grey or green
    pub light: &'static str,
    /// "Trick worked", "Partly worked", "Resisted", "Can't tell", "Waiting for you"
    pub label: String,
    /// What it puts at risk, in plain words ("Money")
    pub impact: String,
    pub severity: String,
    pub scammer: String,
    pub said: String,
    /// The assistant's own words to the stranger, if it wrote any
    pub quote: Option<String>,
    pub means: String,
    pub steps: Vec<String>,
    pub n8n: Vec<String>,
    pub technique: String,
    pub it: Vec<String>,
    pub technical: Vec<String>,
}

impl TestView {
    fn problem(&self) -> bool {
        self.light == "red" || self.light == "amber"
    }
}

pub struct Page {
    pub run_id: String,
    pub started: String,
    pub version: String,
    /// risk, ok or unknown
    pub level: &'static str,
    /// "At risk", "No problems found today", "Can't tell yet"
    pub headline: String,
    pub lines: Vec<String>,
    pub first_step: Option<String>,
    /// "Replied", "Did not reply", "Switched off"
    pub agent_short: String,
    pub agent_sentence: String,
    pub excluded: bool,
    pub changes: Vec<String>,
    pub change_note: String,
    /// Worst first
    pub tests: Vec<TestView>,
    pub limits: String,
    pub consistent: String,
    pub share: String,
    pub help: String,
}

fn e(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn list(items: &[String], ordered: bool) -> String {
    let tag = if ordered { "ol" } else { "ul" };
    let inner: String = items.iter().map(|s| format!("<li>{}</li>", e(s.trim()))).collect();
    format!("<{tag}>{inner}</{tag}>")
}

/// The result mark: a shape as well as a colour, so it reads in black-and-white print and for
/// colour-blind readers. Solid = worked, half = partly, outline = resisted, dashed = can't tell.
fn mark(light: &str) -> String {
    format!("<span class=\"mark {}\" aria-hidden=\"true\"></span>", e(light))
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Horizontal bars, one per outcome. The largest bar takes about 75% of the bar area, every bar is
/// labelled with its value, and only "Trick worked" is drawn in the accent colour.
fn chart(tests: &[TestView]) -> String {
    let count = |l: &str| tests.iter().filter(|t| t.light == l).count();
    let rows = [("red", "Trick worked", count("red")), ("amber", "Partly worked", count("amber")), ("green", "Resisted", count("green")), ("grey", "Can't tell", count("grey"))];
    let max = rows.iter().map(|r| r.2).max().unwrap_or(0).max(1) as f64;
    let (bar_x, area) = (136.0, 150.0);
    let row_h = 30.0;
    let mut svg = String::new();
    for (i, (light, label, n)) in rows.iter().enumerate() {
        let y = 6.0 + i as f64 * row_h;
        let cy = y + 9.0;
        // the mark, drawn in SVG so it prints exactly
        let m = match *light {
            "red" => format!("<rect class=\"m-red\" x=\"0\" y=\"{y}\" width=\"14\" height=\"14\"/>", y = cy - 7.0),
            "amber" => format!(
                "<rect class=\"m-amber-o\" x=\"1\" y=\"{y1}\" width=\"12\" height=\"12\"/><polygon class=\"m-amber\" points=\"1,{a} 13,{a} 1,{b}\"/>",
                y1 = cy - 6.0,
                a = cy - 6.0,
                b = cy + 6.0
            ),
            "green" => format!("<rect class=\"m-green\" x=\"1\" y=\"{y}\" width=\"12\" height=\"12\"/>", y = cy - 6.0),
            _ => format!("<rect class=\"m-grey\" x=\"1\" y=\"{y}\" width=\"12\" height=\"12\"/>", y = cy - 6.0),
        };
        let w = if *n == 0 { 0.0 } else { (*n as f64 / max) * area * 0.75 };
        let bar = if *n == 0 {
            String::new()
        } else {
            format!("<rect class=\"{cls}\" x=\"{bar_x}\" y=\"{y}\" width=\"{w:.1}\" height=\"14\"/>", cls = if *light == "red" { "bar key" } else { "bar" }, y = cy - 7.0)
        };
        svg.push_str(&format!(
            "{m}<text class=\"cl\" x=\"22\" y=\"{ty}\">{label}</text>{bar}<text class=\"cv\" x=\"{vx:.1}\" y=\"{ty}\">{n}</text>",
            ty = cy + 4.5,
            label = e(label),
            vx = bar_x + w + 7.0,
        ));
    }
    let h = 6.0 + rows.len() as f64 * row_h;
    format!(
        "<figure class=\"chart\"><svg viewBox=\"0 0 320 {h}\" role=\"img\" aria-label=\"Results: {} worked, {} partly worked, {} resisted, {} can't tell\">{svg}</svg><figcaption>Partly worked: the assistant obeyed the stranger, but nothing from your mailbox went with it this time.</figcaption></figure>",
        rows[0].2, rows[1].2, rows[2].2, rows[3].2
    )
}

fn register(tests: &[TestView]) -> String {
    let rows: String = tests
        .iter()
        .enumerate()
        .map(|(i, t)| {
            format!(
                "<tr class=\"{l}\"><td class=\"num\"><a href=\"#t-{id}\">{n}</a></td><td><a href=\"#t-{id}\">{title}</a></td><td class=\"outcome\">{mark}{label}</td><td class=\"risk\">{impact}</td></tr>",
                l = t.light,
                id = e(&t.id),
                n = i + 1,
                title = e(&t.title),
                mark = mark(t.light),
                label = e(&t.label),
                impact = e(&t.impact),
            )
        })
        .collect();
    format!(
        "<table class=\"register\"><thead><tr><th class=\"num\">No.</th><th>Test</th><th>Outcome</th><th class=\"risk\">Puts at risk</th></tr></thead><tbody>{rows}</tbody></table>"
    )
}

fn side(head: &str, body: &str) -> String {
    side_with(head, body, "side")
}

/// A short side-head row that should never split across pages in print.
fn side_keep(head: &str, body: &str) -> String {
    side_with(head, body, "side keep")
}

fn side_with(head: &str, body: &str, class: &str) -> String {
    format!("<div class=\"{class}\"><h4>{}</h4><div class=\"sbody\">{body}</div></div>", e(head))
}

fn finding(n: usize, t: &TestView) -> String {
    let mut out = String::new();
    if t.problem() && !t.scammer.is_empty() {
        out.push_str(&side_keep("What a scammer could do", &format!("<p>{}</p>", e(&t.scammer))));
    }
    let mut happened = format!("<p>{}</p>", e(&t.said));
    if let Some(q) = &t.quote {
        happened.push_str(&format!("<blockquote class=\"{}\"><p>{}</p></blockquote>", t.light, e(q)));
    }
    out.push_str(&side_keep("What happened", &happened));
    if t.problem() {
        if !t.means.is_empty() {
            out.push_str(&side_keep("What it means", &format!("<p>{}</p>", e(&t.means))));
        }
        if !t.steps.is_empty() {
            let mut fix = list(&t.steps, true);
            if !t.n8n.is_empty() {
                fix.push_str(&format!("<details class=\"n8n\"><summary>If your assistant is built with n8n</summary>{}</details>", list(&t.n8n, false)));
            }
            out.push_str(&side("How to fix it", &fix));
        }
    }
    let technical = if t.problem() && !t.technical.is_empty() { format!("<p class=\"sub\">Technical fixes</p>{}", list(&t.technical, false)) } else { String::new() };
    out.push_str(&side(
        "For your IT person",
        &format!("<details class=\"it\"><summary>Evidence and technical details</summary>{}{technical}</details>", list(&t.it, false)),
    ));
    format!(
        "<section class=\"finding {l}\" id=\"t-{id}\"><header class=\"fhead\"><span class=\"fnum\">{n}</span><div><h3>{title}</h3><p class=\"facts\"><span class=\"outcome {l}\">{mark}{label}</span><span>Puts at risk: {impact}</span><span>Severity: {sev}</span><span class=\"tid\">{id}</span></p></div></header>{out}</section>\n",
        l = t.light,
        id = e(&t.id),
        title = e(&t.title),
        mark = mark(t.light),
        label = e(&t.label),
        impact = e(&t.impact),
        sev = e(&cap(&t.severity)),
    )
}

fn resisted_table(tests: &[(usize, &TestView)]) -> String {
    let rows: String = tests
        .iter()
        .map(|(n, t)| {
            format!(
                "<tr id=\"t-{id}\"><td class=\"num\">{n}</td><td>{title}<span class=\"tid\">{id}</span></td><td class=\"tech\">{tech}</td></tr>",
                id = e(&t.id),
                title = e(&t.title),
                tech = e(&t.technique),
            )
        })
        .collect();
    format!("<table class=\"plain\"><thead><tr><th class=\"num\">No.</th><th>Test</th><th>How the trick was hidden</th></tr></thead><tbody>{rows}</tbody></table>")
}

pub fn render(p: &Page) -> String {
    let mut lines = p.lines.iter();
    let count = lines.next().map(|l| format!("<p class=\"count\">{}</p>", e(l))).unwrap_or_default();
    let rest: String = lines.map(|l| format!("<p>{}</p>", e(l))).collect();
    let first = p
        .first_step
        .as_ref()
        .map(|s| format!("<section class=\"first\">{}</section>", side("Do this first", &format!("<p>{}</p>", e(s)))))
        .unwrap_or_default();
    let excluded = if p.excluded {
        format!("<p class=\"note\">This run is left out of comparisons. To undo: <code>indefessus runs include {}</code></p>", e(&p.run_id))
    } else {
        String::new()
    };
    let changes = if p.changes.is_empty() {
        String::new()
    } else {
        format!(
            "<section class=\"changed\">{}</section>",
            side("Changed since the last good run", &format!("{}<p class=\"note\">{}</p>", list(&p.changes, false), e(&p.change_note)))
        )
    };
    let numbered: Vec<(usize, &TestView)> = p.tests.iter().enumerate().map(|(i, t)| (i + 1, t)).collect();
    let problems: String = numbered.iter().filter(|(_, t)| t.problem()).map(|(n, t)| finding(*n, t)).collect();
    let unclear: String = numbered.iter().filter(|(_, t)| t.light == "grey").map(|(n, t)| finding(*n, t)).collect();
    let resisted: Vec<(usize, &TestView)> = numbered.iter().filter(|(_, t)| t.light == "green").copied().collect();
    let mut body = String::new();
    if !problems.is_empty() {
        body.push_str(&format!("<h2>Findings</h2><p class=\"lede\">The tricks that worked, worst first.</p>{problems}"));
    }
    if !unclear.is_empty() {
        body.push_str(&format!("<h2>Not judged</h2><p class=\"lede\">Tests where we could not tell what the assistant did.</p>{unclear}"));
    }
    if !resisted.is_empty() {
        body.push_str(&format!("<h2>Resisted</h2><p class=\"lede\">The assistant did not do what the stranger asked in these tests.</p>{}", resisted_table(&resisted)));
    }
    let tests_n = p.tests.len();

    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light dark">
<title>Indefessus report {run}</title>
<style>
:root{{--paper:#ffffff;--ink:#1b1f24;--muted:#5b6470;--rule:#d8dce2;--faint:#f4f5f7;--navy:#1d3c6e;
--red:#b42318;--amber:#9a5205;--amber-mark:#dc6803;--green:#08703f;--grey:#5d6675;
--serif:Charter,"Bitstream Charter","Sitka Text",Cambria,Georgia,serif;
--sans:system-ui,-apple-system,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif;
--mono:ui-monospace,"Cascadia Mono",Consolas,"Liberation Mono",Menlo,monospace}}
@media screen and (prefers-color-scheme:dark){{:root{{--paper:#17191d;--ink:#e6e8eb;--muted:#a3abb7;--rule:#363c45;--faint:#20242a;--navy:#a8c0ea;
--red:#f97066;--amber:#fdb022;--amber-mark:#f79009;--green:#5fd39b;--grey:#9aa3b2}}}}
@page{{size:A4;margin:18mm 17mm 20mm;
@bottom-left{{content:"Indefessus report, {run}";font:8pt system-ui,sans-serif;color:#5b6470}}
@bottom-right{{content:"Page " counter(page) " of " counter(pages);font:8pt system-ui,sans-serif;color:#5b6470}}}}
*{{box-sizing:border-box}}
html{{background:var(--paper)}}
body{{margin:0 auto;max-width:56rem;padding:2.2rem 2rem 3rem;color:var(--ink);background:var(--paper);font:1.0625rem/1.6 var(--serif);-webkit-print-color-adjust:exact;print-color-adjust:exact}}
a{{color:inherit;text-decoration-color:var(--rule);text-underline-offset:.15em}}
a:focus-visible,summary:focus-visible{{outline:3px solid var(--navy);outline-offset:2px}}
code,.tid{{font-family:var(--mono);font-size:.82em}}

/* masthead */
.mast{{display:flex;justify-content:space-between;align-items:baseline;gap:1rem;flex-wrap:wrap;border-bottom:3px solid var(--ink);padding-bottom:.5rem}}
.brand{{font:700 1.15rem/1 var(--sans);letter-spacing:-.01em}}
.doc{{font:1rem/1.2 var(--sans);color:var(--muted)}}
.runinfo{{display:grid;grid-template-columns:repeat(4,auto);justify-content:start;gap:.15rem 2.2rem;margin:.7rem 0 0;font:.85rem/1.45 var(--sans)}}
.runinfo dt{{color:var(--muted)}} .runinfo dd{{margin:0;font-weight:600}}

/* verdict */
.verdict{{display:grid;grid-template-columns:minmax(0,1fr) 20rem;gap:2.5rem;align-items:start;margin:2.4rem 0 1.6rem}}
.lead{{font:1rem/1.2 var(--sans);color:var(--muted);margin:0 0 .3rem}}
.verdict h1{{font:700 3.6rem/1 var(--serif);letter-spacing:-.02em;margin:0 0 1rem}}
.risk h1{{color:var(--red)}} .ok h1{{color:var(--green)}} .unknown h1{{color:var(--grey)}}
.count{{font-size:1.3rem;line-height:1.4;margin:0 0 .6rem}}
.verdict p{{margin:0 0 .5rem}}
.agent{{font:.88rem/1.5 var(--sans);color:var(--muted);margin-top:.9rem!important}}
.chart{{margin:.4rem 0 0;padding-top:.8rem;border-top:1px solid var(--rule)}}
.chart svg{{width:100%;height:auto;display:block;overflow:visible}}
.chart .cl{{font:13px var(--sans);fill:var(--ink)}} .chart .cv{{font:600 13px var(--sans);fill:var(--ink)}}
.chart .bar{{fill:none;stroke:var(--muted);stroke-width:1.2}} .chart .bar.key{{fill:var(--red);stroke:none}}
.m-red{{fill:var(--red)}} .m-amber{{fill:var(--amber-mark)}} .m-amber-o{{fill:none;stroke:var(--amber-mark);stroke-width:2}}
.m-green{{fill:none;stroke:var(--green);stroke-width:2}} .m-grey{{fill:none;stroke:var(--grey);stroke-width:2;stroke-dasharray:3 2}}
.chart figcaption{{font:.8rem/1.45 var(--sans);color:var(--muted);margin-top:.5rem}}

/* side-head rows: one grid for the whole document */
.side{{display:grid;grid-template-columns:10rem minmax(0,1fr);gap:1.6rem;padding:.55rem 0}}
.side h4{{font:600 .85rem/1.45 var(--sans);color:var(--muted);margin:.15rem 0 0}}
.sbody>:first-child{{margin-top:0}} .sbody>:last-child{{margin-bottom:0}}
.sbody p{{margin:0 0 .5rem}}
.first{{border-top:2px solid var(--ink);border-bottom:1px solid var(--rule);padding:.5rem 0;margin:0 0 2.2rem}}
.first h4{{color:var(--navy)}} .first p{{font-size:1.3rem;line-height:1.45}}
.changed{{border-left:4px solid var(--amber-mark);padding-left:.9rem;margin:0 0 2rem}}
.changed ul{{margin:0 0 .4rem;padding-left:1.1rem}}
.note{{font:.85rem/1.5 var(--sans);color:var(--muted)}}

/* sections */
h2{{font:700 1.75rem/1.2 var(--serif);margin:2.8rem 0 .2rem;padding-top:.6rem;border-top:3px solid var(--ink)}}
.lede{{font:.95rem/1.5 var(--sans);color:var(--muted);margin:0 0 1.1rem}}
h2.reg{{font-size:1.25rem;border-top:0;margin:0 0 .4rem;padding:0}}

/* tables */
table{{width:100%;border-collapse:collapse;font:.92rem/1.45 var(--sans)}}
th{{text-align:left;font-weight:600;color:var(--muted);border-bottom:1.5px solid var(--ink);padding:.35rem .6rem .35rem 0}}
td{{border-bottom:1px solid var(--rule);padding:.5rem .6rem .5rem 0;vertical-align:top}}
.num{{width:2.6rem;font-variant-numeric:tabular-nums}}
td.num{{font-weight:600}}
.register a{{text-decoration:none}} .register a:hover{{text-decoration:underline}}
td.outcome{{white-space:nowrap;font-weight:600}} td.risk{{color:var(--muted);white-space:nowrap}}
tr.red td.outcome{{color:var(--red)}} tr.amber td.outcome{{color:var(--amber)}} tr.green td.outcome{{color:var(--green)}} tr.grey td.outcome{{color:var(--grey)}}
.plain .tid{{display:block;color:var(--muted)}} td.tech{{color:var(--muted)}}

/* the mark */
.mark{{display:inline-block;width:.8rem;height:.8rem;margin-right:.45rem;vertical-align:-.05rem;flex:none}}
.mark.red{{background:var(--red)}}
.mark.amber{{border:2px solid var(--amber-mark);background:linear-gradient(135deg,var(--amber-mark) 50%,transparent 50%)}}
.mark.green{{border:2px solid var(--green)}}
.mark.grey{{border:2px dashed var(--grey)}}

/* findings */
.finding{{padding:1.4rem 0 .6rem;border-bottom:1px solid var(--rule)}}
.fhead{{display:grid;grid-template-columns:10rem minmax(0,1fr);gap:1.6rem;margin-bottom:.4rem}}
.fnum{{font:700 2.4rem/1 var(--serif);color:var(--rule);text-align:left}}
.finding.red .fnum{{color:var(--red)}} .finding.amber .fnum{{color:var(--amber-mark)}}
.fhead h3{{font:700 1.3rem/1.3 var(--serif);margin:.15rem 0 .35rem}}
.facts{{display:flex;flex-wrap:wrap;gap:.2rem 1.4rem;font:.85rem/1.4 var(--sans);color:var(--muted);margin:0}}
.facts .outcome{{font-weight:700}} .outcome.red{{color:var(--red)}} .outcome.amber{{color:var(--amber)}} .outcome.grey{{color:var(--grey)}} .outcome.green{{color:var(--green)}}
blockquote{{margin:.2rem 0 .7rem;padding:.1rem 0 .1rem 1rem;border-left:3px solid var(--rule);font:.95rem/1.55 var(--sans);color:var(--ink)}}
blockquote.red{{border-left-color:var(--red)}} blockquote.amber{{border-left-color:var(--amber-mark)}}
blockquote p{{margin:0}}
ol,ul{{margin:0 0 .5rem;padding-left:1.25rem}} li{{margin:.2rem 0}} ol li::marker{{font:600 .9em var(--sans);color:var(--muted)}}
details{{margin:.3rem 0 0}}
summary{{font:600 .88rem/1.5 var(--sans);color:var(--navy);cursor:pointer;width:fit-content}}
details[open]>summary{{margin-bottom:.35rem}}
details.it{{font:.85rem/1.5 var(--sans);color:var(--muted)}} details.it li{{overflow-wrap:anywhere}} details.it ul{{margin-bottom:.3rem}}
.sub{{font-weight:600;color:var(--ink);margin:.4rem 0 .1rem!important}}

/* about */
.about{{margin-top:3rem;border-top:3px solid var(--ink);padding-top:.6rem;font-size:.95rem;color:var(--muted)}}
.about h2{{font-size:1.1rem;border:0;padding:0;margin:0 0 .5rem;color:var(--ink)}}
.about p{{margin:0 0 .6rem;max-width:42rem}}
.about .note{{margin-top:1rem}}

@media (max-width:44rem){{
body{{padding:1.2rem 1rem 2rem}}
.runinfo{{grid-template-columns:auto auto;gap:.1rem 1.4rem}}
.verdict{{grid-template-columns:1fr;gap:1rem;margin-top:1.6rem}}
.verdict h1{{font-size:2.7rem}}
.side,.fhead{{grid-template-columns:1fr;gap:.15rem}}
.fnum{{font-size:1.6rem}}
.register .risk{{display:none}}
}}
@media print{{
body{{max-width:none;padding:0;font-size:10.5pt}}
.finding{{padding:.9rem 0 .3rem}} .side{{padding:.35rem 0}} h2{{margin-top:1.8rem}} .about{{margin-top:1.8rem}}
.verdict{{margin:1.6rem 0 1.2rem}} .first{{margin-bottom:1.4rem}}
.verdict h1{{font-size:34pt}}
summary{{list-style:none;color:var(--ink)}} summary::-webkit-details-marker{{display:none}}
h2,.lede,h3,.fhead{{break-after:avoid}}
.fhead,.verdict,.first,blockquote,tr,.chart,.facts,.side.keep{{break-inside:avoid}}
.side,.fhead{{grid-template-columns:11.5rem minmax(0,1fr)}}
p,li{{orphans:3;widows:3}}
.register{{margin-bottom:0}}
a{{text-decoration:none}}
}}
</style></head><body>
<header class="mast"><span class="brand">Indefessus</span><span class="doc">Email assistant security check</span></header>
<dl class="runinfo">
<div><dt>Run</dt><dd><code>{run}</code></dd></div>
<div><dt>Started</dt><dd>{started}</dd></div>
<div><dt>Agent check</dt><dd>{agent_short}</dd></div>
<div><dt>Tests</dt><dd>{tests_n}, Indefessus {version}</dd></div>
</dl>
<section class="verdict {level}" aria-label="Verdict">
<div>
<p class="lead">Your email assistant</p>
<h1>{headline}</h1>
{count}{rest}
<p class="agent">{agent_sentence}</p>
{excluded}
</div>
{chart}
</section>
{first}
{changes}
<h2 class="reg">All tests, worst first</h2>
{register}
{body}
<section class="about">
<h2>About this check</h2>
<p>{limits}</p>
<p>{consistent}</p>
<p class="note">{share}<br>{help}</p>
</section>
<script>
// Open every folded section when printing, so the PDF carries all the details.
window.addEventListener("beforeprint", function () {{ document.querySelectorAll("details").forEach(function (d) {{ d.open = true; }}); }});
</script>
</body></html>
"#,
        run = e(&p.run_id),
        started = e(&p.started),
        agent_short = e(&p.agent_short),
        version = e(&p.version),
        level = p.level,
        headline = e(&p.headline),
        agent_sentence = e(&p.agent_sentence),
        chart = chart(&p.tests),
        register = register(&p.tests),
        limits = e(&p.limits),
        consistent = e(&p.consistent),
        share = e(&p.share),
        help = e(&p.help),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, light: &'static str, label: &str) -> TestView {
        TestView {
            id: id.into(),
            title: format!("Title of {id} & <more>"),
            light,
            label: label.into(),
            impact: "Money".into(),
            severity: "high".into(),
            scammer: "Could do a thing.".into(),
            said: "It sent an email.".into(),
            quote: Some("We confirm \"approved\"".into()),
            means: "It means a thing.".into(),
            steps: vec!["First step.".into(), "Second step.".into()],
            n8n: vec!["n8n step.".into()],
            technique: "plain".into(),
            it: vec!["Test: x".into()],
            technical: vec!["Tech step.".into()],
        }
    }

    fn page(tests: Vec<TestView>) -> Page {
        Page {
            run_id: "run-1".into(),
            started: "today".into(),
            version: "0.0.0".into(),
            level: "risk",
            headline: "At risk".into(),
            lines: vec!["1 of 3 tricks worked.".into(), "The most serious: x.".into()],
            first_step: Some("Do it.".into()),
            agent_short: "Replied".into(),
            agent_sentence: "Agent check: replied.".into(),
            excluded: true,
            changes: vec!["NOW WORKS: x".into()],
            change_note: "Compared.".into(),
            tests,
            limits: "Limits.".into(),
            consistent: "Consistent.".into(),
            share: "Share.".into(),
            help: "Help.".into(),
        }
    }

    #[test]
    fn every_part_of_the_report_is_there_and_safe() {
        let html = render(&page(vec![t("a", "red", "Trick worked"), t("b", "grey", "Can't tell"), t("c", "green", "Resisted")]));
        for id in ["a", "b", "c"] {
            assert!(html.contains(&format!("id=\"t-{id}\"")), "anchor for {id}");
            assert!(html.contains(&format!("href=\"#t-{id}\"")), "register link for {id}");
        }
        for must in ["At risk", "Do this first", "Changed since the last good run", "runs include run-1", "How to fix it", "If your assistant is built with n8n", "Technical fixes", "beforeprint", "About this check", "1 worked, 0 partly worked, 1 resisted, 1 can't tell"] {
            assert!(html.contains(must), "missing: {must}");
        }
        // escaped, offline, and in house style
        assert!(html.contains("Title of a &amp; &lt;more&gt;") && !html.contains("<more>"));
        assert!(!html.contains("http://") && !html.contains("src=\"http") && !html.contains("<link"));
        assert!(!html.contains('\u{2014}'));
    }

    #[test]
    fn largest_bar_is_three_quarters_of_the_area() {
        let svg = chart(&[t("a", "amber", "Partly worked"), t("b", "amber", "Partly worked"), t("c", "red", "Trick worked")]);
        assert!(svg.contains("width=\"112.5\""), "{svg}");
    }
}
