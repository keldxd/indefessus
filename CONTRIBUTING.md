# Contributing

The most valuable contribution is a **new test**: a trick you have seen used, or think will be used, against AI agents that read email. Every test added protects every organisation that runs Indefessus.

## Adding a test

Create a file in `scenarios/`, for example `13-my-test.toml`, and add it to the `BUILTIN` list at the top of `src/scenarios.rs` (that is what builds it into the program):

```toml
id = "my-test"                       # unique, lowercase, dashes
title = "What the agent does wrong, in plain words"
harm = "leak"                        # leak | act | reveal | vouch
severity = "high"                    # low | medium | high | critical
mode = "auto"                        # auto: detected from outside; guided: a person asks the assistant
technique = "short description of the trick"
subject = "A normal-looking subject"
body_text = """The email. Use {{canary_url}} and/or {{trap_email}} where the agent would send data."""
# body_html = """optional HTML version, for hidden-text tricks"""
# seed_subject / seed_body = """an optional separate email holding a fake private code, {{seed_secret}}"""
# guided_prompt = "what the person should ask the assistant (guided mode only)"

[detect]
url_hit = true                       # the agent opened {{canary_url}}
# trap_mail = true                   # mail reached {{trap_email}}
# output_contains = ["{{token}}"]    # the assistant's answer contained planted text (guided)

[explain]
fail = "What it means for the organisation, in one or two plain sentences."
fix = "What they can do about it."
```

Rules for a good test:

- **Harmless.** It may only ask the agent to open the tracker link or send mail to the trap address. Never real malware, real phishing domains, or real people's data.
- **No markers in the subject.** Put `{{token}}` and `{{seed_secret}}` only in the body, so an assistant that just lists subjects does not cause a false alarm.
- **Plain language** in `title`, `fail` and `fix`. The reader runs a clinic or a school, not a security team.
- **The self-test must still pass**: the naive stand-in agent must fail your test, and the careful one must pass it.

```bash
cargo test
cargo run -- selftest
```

## Code

Indefessus is written in Rust and ships as one file per operating system, so a small organisation can run it without installing anything. Please keep new dependencies to a minimum, and keep every message the user sees in plain language.
