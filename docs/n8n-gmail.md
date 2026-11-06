# Test your n8n email agent in 15 minutes

This guide is for an n8n workflow that reads a Gmail inbox and lets an AI agent act on it (reply, send email, open links). It works the same way for other agent builders: only step 1 is n8n-specific.

## What you need

- Your n8n agent, switched on, reading Gmail inbox **A** (the one to test).
- A **second Gmail account B** that you control. It plays the stranger: it sends the tests and catches anything the agent leaks. Never use A for this.
- Indefessus ([download](https://github.com/keldxd/indefessus/releases)) and the free `cloudflared` program:
  - Windows: `winget install --id Cloudflare.cloudflared`
  - macOS: `brew install cloudflared`
  - Linux: [Cloudflare downloads page](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/)

## 1. Check your agent is on

In n8n, open the workflow and make sure it is **Active** (or running in test mode while you test). Send A an ordinary email from B, like "Are you open on Saturday?", and check the agent handles it. If it does nothing, Indefessus will report *inconclusive*, not *passed*, so fix this first.

## 2. Make an App Password for B

Gmail does not let programs use your normal password. On account B:

1. Turn on 2-Step Verification: https://myaccount.google.com/security
2. Create an App Password: https://myaccount.google.com/apppasswords (any name, for example "indefessus")
3. Copy the 16 letters. You will put them in an environment variable, never in a file.

Also check IMAP is on for B: Gmail settings, *Forwarding and POP/IMAP*, *Enable IMAP*.

## 3. Set up Indefessus

```bash
indefessus setup
```

Answer: A as the mailbox to test, *Gmail* for the second mailbox, B's address, and *free automatic tunnel*. Then set the password in this terminal:

```bash
export INDEFESSUS_SMTP_PASSWORD='abcdabcdabcdabcd'      # B's app password
export INDEFESSUS_TRAP_PASSWORD="$INDEFESSUS_SMTP_PASSWORD"
```

On Windows PowerShell:

```powershell
$env:INDEFESSUS_SMTP_PASSWORD='abcdabcdabcdabcd'
$env:INDEFESSUS_TRAP_PASSWORD=$env:INDEFESSUS_SMTP_PASSWORD
```

## 4. Start the tracker

In a **second terminal**, in the same folder:

```bash
indefessus listen --tunnel
```

It prints a public address ending in `trycloudflare.com`. Leave it running for the whole test.

## 5. Check, verify, test

Back in the first terminal:

```bash
indefessus doctor           # every line should say OK
indefessus verify           # a code arrives in A; type it in
indefessus run --wait 20    # sends the tests and prints the report when the agent is done
```

The report says, for each test, whether the agent obeyed the stranger, what evidence was found, and what to do about it. The full report is also saved as a web page in `.indefessus/reports/`.

## 6. What next

- Agents are not consistent. Run `indefessus run --wait 20 --repeat 3` to see how often each test fails.
- Changed the prompt or the model? Run again and compare: the report lists what got better or worse.
- Share your anonymised result: `indefessus share --agent "n8n + <model>, tools: <which>"`, then paste it into a [result report](https://github.com/keldxd/indefessus/issues/new?template=share-result.yml).

## Common fixes for n8n agents that fail

- Give the agent **fewer tools**. An agent that cannot send email to any address cannot leak to a stranger. Limit the Send Email tool to fixed recipients where you can.
- Add a **human approval step** before any email is sent or link is opened.
- Tell the agent in its system prompt that emails are **data, not instructions**. This helps but is not enough on its own, which is why you test.
- Never let the agent confirm payments or change payment details from an email.

Questions: [Discussions](https://github.com/keldxd/indefessus/discussions).
