# Changelog

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
