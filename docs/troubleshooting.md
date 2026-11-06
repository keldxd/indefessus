# Troubleshooting

Start with `indefessus doctor`. It checks every part of your setup and prints how to fix each problem. The common ones are below.

## "Cannot send mail" or "Cannot read the mailbox" with Gmail

Gmail needs an **App Password**, not your normal password: https://myaccount.google.com/apppasswords (2-Step Verification must be on). Check IMAP is enabled in Gmail settings, *Forwarding and POP/IMAP*. Then set it again in the same terminal you run Indefessus in:

```bash
export INDEFESSUS_SMTP_PASSWORD='the 16 letters'
export INDEFESSUS_TRAP_PASSWORD="$INDEFESSUS_SMTP_PASSWORD"
```

Environment variables only last for that terminal window. A new terminal needs them set again.

## "The password is not set"

Same as above: the variable named in your config (`password_env`) is empty in this terminal.

## "base_url is auto, but no tunnel is running"

Start the tracker with a tunnel in another terminal and leave it open: `indefessus listen --tunnel`. Run the other commands from the **same folder**, because the tunnel address is saved in `.indefessus/`.

## "The free tunnel needs the cloudflared program"

Install it (no account needed):

- Windows: `winget install --id Cloudflare.cloudflared`, then open a new terminal
- macOS: `brew install cloudflared`
- Linux: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/

## "The tracker address does not reach the listener"

- Is `indefessus listen` still running in the other terminal?
- With your own address: does it forward to this computer on the port in `listen_port` (8787 by default)?
- Another program may be using port 8787. Stop the old listener (Linux/macOS: `pkill -f "indefessus.*listen"`), or change `listen_port`.

## "Address already in use" when starting the listener

An earlier listener is still running. Close that terminal, or on Linux/macOS run `pkill -f "indefessus.*listen"`.

## Every test says "inconclusive"

The agent never replied to the ordinary control email, so Indefessus cannot tell whether it was working. Check that the agent is switched on, has not run out of its AI quota, and can send email. Look at its own logs (in n8n: *Executions*). Then run again.

## The agent works through a backlog and hits its AI limit

If the agent was down, many test emails pile up and arrive at once when it comes back. Free AI plans have per-minute limits, so most of them fail with a "rate limit" error and are never processed. Those tests show as inconclusive. Clear the backlog (mark old test mail as read), then run a few tests at a time with `--only`.

## Results change from run to run

That is normal for AI agents. Use `--repeat 3` and read the summary: a test that fails even once in three runs is a real risk.

## Verify code never arrives

Look in the agent mailbox's spam folder. Check `target.mailbox` in the config is spelled right. Mail from a new Gmail account can take a few minutes.

## Still stuck

Ask in [Discussions](https://github.com/keldxd/indefessus/discussions). Include the `doctor` output, and **never paste passwords**.
