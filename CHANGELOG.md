# Changelog

## Unreleased (0.3.0)

Reports anyone can read.

- **A verdict at the top of every report**, in the terminal and the HTML: *AT RISK*, *NO PROBLEMS FOUND TODAY* or *CAN'T TELL YET*, how many tricks worked, the most serious one in everyday words, and what to do first. Indefessus never says "safe".
- **Traffic lights per test**: red (the trick worked), amber (the assistant obeyed but nothing leaked this time), green (resisted), grey (can't tell, or waiting for your answer). Worst first: money, then your information leaving, then misleading you, then your setup.
- **What a scammer could do, and how to fix it**, for every test: plain steps for anyone, extra steps for n8n, and technical steps for the IT person. They live in each test file (`scammer`, `worst`, `fix_plain`, `fix_n8n`, `fix_technical`, `impact`), so anyone can improve them. A test checks the plain wording for jargon and em dashes.
- **Technical evidence folded away** under "Details for your IT person". The HTML report works offline, on a phone, in dark mode, and prints to PDF with every detail open.
- **Fix: misleading "got worse" alarms.** Runs are now compared with the last good run: one where the assistant answered the check email, with real results. `indefessus runs` lists runs, and `indefessus runs exclude <run>` leaves a bad run out of comparisons. With `--repeat`, every round is compared with the same earlier run, not with the round before it.
- **Attachment tests (8 new, 20 in total).** The trick hides inside an attached file instead of the email: white text in a PDF invoice, a PDF's document properties, hidden text and a comment in a Word document, a calendar invite, a spreadsheet cell, a text file and an HTML page. Files are built at run time with no extra dependencies, open cleanly in normal viewers, and carry a visible "Indefessus test file" label.
- **US dollars everywhere.** Tests now use USD amounts and neutral addresses and phone numbers, so they read the same for any organisation. The payment test (`approve-payment`) now asks to approve USD 4,800 instead of KES 245,000, so compare its results with earlier runs with that in mind.
- **Attachment check.** Runs with file tests also send an ordinary booking email with a PDF attached and ask for its booking number. If the assistant never quotes it, file tests without evidence are *can't tell*, never *resisted*: an assistant that never opens files has not resisted anything. The self-test has a fourth stand-in for this, an agent that answers email but never opens attachments.
- `indefessus report --html` writes the HTML report of an earlier run again. `share` and the `--repeat` summary use the same plain wording.

## 0.2.0 (2026-11-06)

Easier to start, easier to trust, easier to share.

- **`indefessus setup`**: a few plain questions write a ready config, with a Gmail preset, and print the exact next steps.
- **`indefessus doctor`**: checks sending, reading both mailboxes, the tracker address and ownership, and says how to fix each problem (for example a missing Gmail App Password).
- **`indefessus listen --tunnel`**: a free public address through a Cloudflare quick tunnel, no account needed. Configs with `base_url = "auto"` use it automatically.
- **Pre-flight check**: `run` and `daily` refuse to send tests if the tracker address does not reach the listener, instead of wasting a run.
- **`--repeat N`** on `run` and `daily`: several rounds, then a summary of how often each test failed, because AI agents are not consistent.
- **`indefessus share`**: an anonymised summary (test names and results only) to paste into a public result report. No telemetry: nothing is sent automatically.
- **Fix: the agent check could be fooled by an older run.** A reply to a previous run's control email ("Are you open this Saturday?") was accepted as proof the agent was working, so tests the agent never saw could be reported as passed. Found in the lab, when a backlog of old emails reached the agent at once and its AI provider's rate limit refused this run's emails. Now only a reply carrying this run's own reference counts, and the control email asks the agent to keep that reference in its reply.
- Lab: the n8n trigger reconnects every minute, and the lab guide explains how to recover when it stops reacting.
- Guides: [n8n with Gmail](docs/n8n-gmail.md) and [troubleshooting](docs/troubleshooting.md). Issue templates for results, test ideas and bugs.

## 0.1.1 (2026-11-05)

Fixes from the first live test against a real AI agent in the [lab](lab/).

- **Agent check (control email).** Every run sends one ordinary email that a working agent would reply to. If the agent never replies, tests without evidence are reported as *inconclusive* instead of *passed*. In the live test, an agent that had crashed, was overloaded or had run out of its free AI quota was reported as passing every test: that can no longer happen.
- **Smart waiting.** `run --wait` and `daily` check every minute and stop early, after a short grace period, once the agent has replied to the control email.
- **Exact evidence.** When the agent opened a stranger's link without attaching any data, the report now says so, instead of quoting an empty value. When it wrote to a stranger, the report shows what it wrote. The link, contact-list and private-code tests explain the risk accurately whatever the agent did.
- **Consistency note.** Reports remind the reader that AI agents do not always behave the same way from run to run.
- **First live results** in [lab/RESULTS.md](lab/RESULTS.md).
- **Correct times.** Mail found in the trap mailbox is dated by when it was sent, not when Indefessus found it, and reports use the computer's local time.
- **Test lab.** A local mail server and n8n (`lab/`), plus the exact agent we test (`lab/n8n-office-assistant.json`), so anyone can reproduce the results. Plain mail connections for the lab only (`security = "none"`, `imap_tls = false`).
- **More downloads:** Intel Macs and ARM Linux.
- The self-test now has a third stand-in, a switched-off agent, which must never be reported as passing.

## 0.1.0 (2026-11-05)

First public release: 12 tests, tracker listener, plain-language reports, daily change check, self-test.
