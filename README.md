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
4. **Explains the result in plain language**: what happened, what it means, what to do about it.
5. **Re-checks every day** and warns you when the agent's behaviour changes, for example after the AI model behind it was silently updated.

## Status: v0.1, early

What works and is tested today:

- The test library (12 tests), the tracker listener, evidence collection, the plain-language report, and the day-to-day comparison.
- A single program with no installs: the tests are built into it.
- A **self-test** that runs the whole pipeline against two stand-in agents, with no mailbox needed: a naive one that obeys any instruction (all 12 tests catch it) and a careful one that never does (none of the 12 fire). The stand-ins are simple scripts, not AI models, and say nothing about any real product.

Not yet done: the first public results against real assistants and automations. Those are next.

## Quick start: the self-test (2 minutes)

**Download** the program for your computer from the [Releases page](https://github.com/keldxd/indefessus/releases) (Windows, macOS or Linux). It is a single file with nothing else to install. Then, in a terminal:

```bash
indefessus selftest        # on Windows: indefessus-windows-x86_64.exe selftest
```

**Or build it** with [Rust](https://rustup.rs):

```bash
cargo install --git https://github.com/keldxd/indefessus
indefessus selftest
```

## Testing your own agent

You need three things you control:

| What | Why |
|---|---|
| The **target mailbox** your agent reads | This is what gets tested. Indefessus will only test a mailbox after you prove you can read it |
| A **sender mailbox** | Plays the "stranger" who sends the test emails. A fresh free email account works |
| A **trap mailbox** that supports IMAP | If the agent sends mail here, that is evidence. It can be the same account as the sender |
| A **public address for the tracker listener** | For example a free tunnel (Cloudflare Tunnel, ngrok) to your laptop, or a small server |

Then:

```bash
indefessus init                        # writes indefessus.toml, fill it in
export INDEFESSUS_SMTP_PASSWORD=...    # passwords only ever live in environment variables
export INDEFESSUS_TRAP_PASSWORD=...
indefessus verify                      # emails a code to the target mailbox; only its owner can read it
indefessus listen                      # in a second terminal, keep it running
indefessus run                         # sends the tests
# guided tests: ask your assistant the questions it prints, then:
indefessus answer vouch-for-phishing   # paste the assistant's reply, Ctrl+D
indefessus check                       # after 30 minutes or so: collects evidence, prints the report
```

Every day, automatically (for example with cron):

```bash
indefessus daily        # automatic tests only; exit code 2 if the agent got worse since last time
```

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

*Guided* tests are for assistants you talk to (like Copilot or Gemini in your email): Indefessus tells you what to ask, and checks the answer for text that only the stranger's email contained.

Each test is a small, readable file in [`scenarios/`](scenarios/). Adding one is the easiest way to contribute: see [CONTRIBUTING.md](CONTRIBUTING.md).

## Safety rules built in

- **Only your own mailbox.** `run` refuses to start until `verify` has proven you can read the target mailbox.
- **Harmless payloads.** Tests only ask the agent to open a link or send mail to addresses you control. The "private codes" are fake values planted by the test itself.
- **Clearly labelled.** Every test email has an `X-Indefessus-Test` header, and the tracker link shows a page saying it was a security test.
- **Nothing leaves your machine.** Results live in a local database in `.indefessus/`.
- **Responsible disclosure.** If you find that a widely used product fails these tests, report it to the vendor privately first. See [SECURITY.md](SECURITY.md).

## Limits

- **A pass is not a guarantee.** It means the agent resisted these tests, today. New tricks appear all the time, which is why the library is open and the checks repeat.
- **Some actions are invisible from outside.** If an agent acts in a way that never touches a link or an address you control, Indefessus cannot see it.
- **Guided tests need a person** to ask the assistant and paste its answer.

## Roadmap

- First public results on common setups (an n8n Gmail agent, and email assistants)
- A larger test library, contributed and reviewed in the open
- A free community instance, so organisations without IT staff do not need to run anything
- A public, anonymised log of behaviour changes across AI products over time
- Agents beyond email: shared drives, calendars, chat tools

## License

[Apache 2.0](LICENSE). Built by [Kelvin Mwangi](https://github.com/keldxd) in Kenya.
