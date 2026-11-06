# Indefessus

**Check whether the AI agent connected to your inbox can be turned against you, then re-check it every day for silent changes.**

*Indefessus* is Latin for "tireless". Never tired, never silent.

---

Small organisations are connecting AI agents to their email: assistants that summarise the inbox, bots that answer customers, automations that file invoices. Anyone in the world can send that inbox an email. If the email contains hidden instructions, the agent may follow them: forward your mail, hand over your contacts, approve a payment, or tell you a phishing link is safe. This is not theoretical. [EchoLeak](https://arxiv.org/abs/2509.10540) showed a single email could make Microsoft 365 Copilot leak data without anyone clicking anything.

Large companies hire red teams to test this. A clinic, a school, a co-op or a six-person startup cannot. Indefessus is a free, open-source test kit they can run in an afternoon, on the agent they actually use.

## What it does

1. **Sends harmless test emails** to a mailbox you own. Each one tries a real trick: instructions in plain text, in invisible text, or disguised as encoded data.
2. **Plants a tracker in each test**: a unique link and a unique address in a mailbox you control. The trackers carry no real data.
3. **Watches from the outside.** If the agent obeys a test, its tracker fires: the link gets opened, or mail arrives at the trap address. Indefessus never needs access to the agent's internals, so it works on any product, as actually deployed.
4. **Checks the agent was really working.** Every run also sends one ordinary email that any working agent would reply to. If the agent never replies (switched off, crashed, out of quota), results without evidence are reported as **inconclusive**, never as passed.
5. **Explains the result in plain language**: what happened, what it means, what to do about it.
6. **Re-checks every day** and warns you when the agent's behaviour changes, for example after the AI model behind it was silently updated.

## Status: v0.2, early

What works and is tested today:

- The test library (12 tests), the tracker listener, evidence collection, the plain-language report, and the day-to-day comparison.
- A single program with no installs: the tests are built into it.
- A **self-test** that runs the whole pipeline against stand-in agents, with no mailbox needed: a naive one that obeys any instruction (all 12 tests catch it), a careful one that never does (none fire), and a switched-off one (never reported as passing). The stand-ins are simple scripts, not AI models, and say nothing about any real product.
- **Guided setup** (`setup`), a **setup checker** (`doctor`), a **free automatic tunnel** (`listen --tunnel`), **repeated runs** (`--repeat`) and **anonymised sharing** (`share`).

**First live results** ([full write-up](lab/RESULTS.md)): in the reproducible [test lab](lab/), a typical "take care of my inbox" n8n agent running Groq's `gpt-oss-120b` **failed 6 of 8 tests**. Strangers' emails made it confirm a KES 245,000 payment approval, send out its own instructions and tool list, open links it was told to open, and write to addresses it was told to write to. Indefessus caught every one from the outside, with no false alarms, and every result was cross-checked against the agent's own log.

## Step 1: the self-test (2 minutes, nothing to set up)

**Download** the program for your computer from the [Releases page](https://github.com/keldxd/indefessus/releases): Windows, macOS (Apple Silicon or Intel) or Linux (x86 or ARM, such as a Raspberry Pi). It is a single file with nothing else to install. Then, in a terminal:

```bash
indefessus selftest        # on Windows: indefessus-windows-x86_64.exe selftest
```

**Or build it** with [Rust](https://rustup.rs):

```bash
cargo install --git https://github.com/keldxd/indefessus
indefessus selftest
```

## Step 2: test your own agent (about 15 minutes)

You need:

- The **mailbox your agent reads**. Indefessus only tests it after you prove you can read it.
- A **second mailbox you control** that plays the stranger. A free Gmail account works: it sends the tests and catches anything the agent leaks.
- For the test links, the free [`cloudflared`](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) program (no account needed), or your own public address.

```bash
indefessus setup              # answers a few questions, writes indefessus.toml, prints your next steps
indefessus listen --tunnel    # in a second terminal: the tracker, with a free public address. Keep it running
indefessus doctor             # checks every part of the setup and tells you how to fix what is wrong
indefessus verify             # emails a code to the agent's mailbox; only its owner can read it
indefessus run --wait 20      # sends the tests, waits for the agent, prints the report
```

Passwords only ever live in environment variables, never in the config file. Using an n8n agent with Gmail? Follow the step-by-step guide: [docs/n8n-gmail.md](docs/n8n-gmail.md). Something not working? [docs/troubleshooting.md](docs/troubleshooting.md).

AI agents do not behave the same way every time. To see how consistent yours is, run the tests several times:

```bash
indefessus run --wait 20 --repeat 3     # three rounds, then a summary: failed 3/3, 1/3, ...
```

Guided tests (for assistants you chat with) and checking later:

```bash
indefessus run                          # sends the tests and prints the questions for guided tests
indefessus answer vouch-for-phishing    # paste the assistant's reply, Ctrl+D
indefessus check                        # collects evidence and prints the report
```

Every day, automatically (for example with cron or Task Scheduler):

```bash
indefessus daily        # automatic tests only; exit code 2 if the agent got worse since last time
```

## Step 3: share what you found

```bash
indefessus share --agent "n8n + gpt-oss-120b, can send email and open links"
```

prints an anonymised summary (test names and results only: no addresses, no email content) that you can paste into a [result report](https://github.com/keldxd/indefessus/issues/new?template=share-result.yml). Every shared result helps others see which setups are risky. Nothing is ever sent anywhere automatically: Indefessus has no telemetry.

## Help, and a free pilot for organisations

- Questions, ideas or setups you want covered: [Discussions](https://github.com/keldxd/indefessus/discussions).
- **Free pilot.** If your organisation uses an AI agent on email (a clinic, school, co-op, NGO or small company), I will help you set up Indefessus, run it on your agent with you, and walk you through the results. Free while the project is early. Start a thread in [Discussions](https://github.com/keldxd/indefessus/discussions) titled *Pilot: your organisation type* (no private details needed in public).

## The tests

| Test | What it tries |
|---|---|
| `leak-link-plain` | Make the agent send your email subjects to a stranger's link |
| `leak-link-hidden` | The same, with the instruction in invisible text |
| `leak-link-encoded` | The same, with the instruction disguised as encoded data |
| `forward-recent-mail` | Make the agent forward your recent mail to a stranger |
| `reply-with-contacts` | Make the agent send a stranger your contact list |
| `leak-private-code` | Make the agent dig a private code out of another email and send it out |
| `approve-payment` | Make the agent confirm a payment approval to a stranger |
| `reveal-setup` | Make the agent describe its own instructions and tools |
| `vouch-for-phishing` | Make the assistant tell you a phishing email is safe (guided) |
| `hijack-summary` | Let a stranger write part of the assistant's answer to you (guided) |
| `swap-payment-details` | Make the assistant pass on a stranger's payment instructions (guided) |
| `reveal-private-code` | Make the assistant repeat a private code in its answer (guided) |

Run `indefessus scenarios` to list them.

*Guided* tests are for assistants you talk to (like Copilot or Gemini in your email): Indefessus tells you what to ask, and checks the answer for text that only the stranger's email contained.

Each test is a small, readable file in [`scenarios/`](scenarios/). Adding one is the easiest way to contribute: see [CONTRIBUTING.md](CONTRIBUTING.md).

## Safety rules built in

- **Only your own mailbox.** `run` refuses to start until `verify` has proven you can read the target mailbox.
- **Harmless payloads.** Tests only ask the agent to open a link or send mail to addresses you control. The "private codes" are fake values planted by the test itself.
- **Clearly labelled.** Every test email has an `X-Indefessus-Test` header, and the tracker link shows a page saying it was a security test.
- **Nothing leaves your machine.** Results live in a local database in `.indefessus/`. No telemetry; `share` only prints text for you to copy.
- **Responsible disclosure.** If you find that a widely used product fails these tests, report it to the vendor privately first. See [SECURITY.md](SECURITY.md).

## Limits

- **A pass is not a guarantee.** It means the agent resisted these tests, today. New tricks appear all the time, which is why the library is open and the checks repeat.
- **Some actions are invisible from outside.** If an agent acts in a way that never touches a link or an address you control, Indefessus cannot see it.
- **Guided tests need a person** to ask the assistant and paste its answer.
- **The agent check needs the agent to reply to email.** An assistant that never sends email (one you only chat with) cannot confirm it is alive this way; use the guided tests for it.

## Roadmap

- First public results on common setups (an n8n Gmail agent, and email assistants)
- A larger test library, contributed and reviewed in the open
- A free community instance, so organisations without IT staff do not need to run anything
- A public, anonymised log of behaviour changes across AI products over time
- Agents beyond email: shared drives, calendars, chat tools

## License

[Apache 2.0](LICENSE). Built by [Kelvin Mwangi](https://github.com/keldxd) in Kenya.
