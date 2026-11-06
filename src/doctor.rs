//! `indefessus doctor`: checks every part of a setup and says exactly how to fix what is wrong.

use crate::config::Config;
use crate::mail::{ImapTrap, Security, SmtpSender, TrapSource};
use crate::net;
use crate::store::Store;
use crate::tunnel;
use crate::util::now;
use std::path::Path;

struct Report {
    problems: usize,
}

impl Report {
    fn ok(&mut self, what: &str) {
        println!("  OK    {what}");
    }
    fn bad(&mut self, what: &str, fix: &str) {
        self.problems += 1;
        println!("  FIX   {what}");
        for line in fix.lines() {
            println!("        {line}");
        }
    }
    fn note(&mut self, what: &str) {
        println!("  NOTE  {what}");
    }
}

fn is_gmail(host: &str) -> bool {
    host.contains("gmail.com") || host.contains("googlemail.com")
}

fn login_fix(host: &str, env: &str, err: &str) -> String {
    let lower = err.to_lowercase();
    let mut fix = String::new();
    if is_gmail(host) && (lower.contains("auth") || lower.contains("credential") || lower.contains("password") || lower.contains("535") || lower.contains("login")) {
        fix.push_str("Gmail refuses normal passwords here. Create an App Password (needs 2-Step Verification):\n");
        fix.push_str("https://myaccount.google.com/apppasswords\n");
        fix.push_str(&format!("then set it: export {env}='the 16-letter app password'"));
    } else if lower.contains("connect") || lower.contains("timed out") || lower.contains("resolve") || lower.contains("dns") {
        fix.push_str(&format!("Could not reach {host}. Check the server name and port in the config, and your internet connection."));
    } else {
        fix.push_str(&format!("Check the username, the password in {env}, and the server settings.\nDetails: {err}"));
    }
    fix
}

pub fn run(cfg: &Config) -> i32 {
    let mut r = Report { problems: 0 };
    println!("Indefessus doctor: checking your setup\n");

    // 1. Mailboxes make sense
    if cfg.target.mailbox.eq_ignore_ascii_case(&cfg.sender.address) || cfg.target.mailbox.eq_ignore_ascii_case(&cfg.trap.address) {
        r.bad(
            "The agent's mailbox is also used as the sender or trap.",
            "Use a second mailbox you control for [sender] and [trap], never the agent's own mailbox.",
        );
    } else {
        r.ok(&format!("Agent's mailbox: {} (sender and trap are separate mailboxes)", cfg.target.mailbox));
    }

    // 2. Sending
    let security = if cfg.sender.use_ssl { Ok(Security::Tls) } else { Security::parse(&cfg.sender.security) };
    match security {
        Err(e) => r.bad("Sender security setting is not valid.", &e.to_string()),
        Ok(security) => {
            let password = if security == Security::None { Ok(Config::secret_or_empty(&cfg.sender.password_env)) } else { Config::secret(&cfg.sender.password_env) };
            match password {
                Err(_) => r.bad(
                    &format!("The sender password is not set ({}).", cfg.sender.password_env),
                    &format!("Set it in this terminal: export {}='...'   (Windows: $env:{}='...')", cfg.sender.password_env, cfg.sender.password_env),
                ),
                Ok(password) => {
                    let s = SmtpSender {
                        host: cfg.sender.smtp_host.clone(),
                        port: cfg.sender.smtp_port,
                        username: cfg.sender.username.clone(),
                        password,
                        security,
                    };
                    match s.test() {
                        Ok(()) => r.ok(&format!("Can send mail through {}:{} as {}", cfg.sender.smtp_host, cfg.sender.smtp_port, cfg.sender.username)),
                        Err(e) => r.bad(
                            &format!("Cannot send mail through {}:{}.", cfg.sender.smtp_host, cfg.sender.smtp_port),
                            &login_fix(&cfg.sender.smtp_host, &cfg.sender.password_env, &format!("{e:#}")),
                        ),
                    }
                }
            }
        }
    }

    // 3. Reading the trap mailbox (and the sender's mailbox, where control replies arrive)
    let password = if cfg.trap.imap_tls { Config::secret(&cfg.trap.password_env) } else { Ok(Config::secret_or_empty(&cfg.trap.password_env)) };
    match password {
        Err(_) => r.bad(
            &format!("The trap password is not set ({}).", cfg.trap.password_env),
            &format!(
                "Set it in this terminal: export {}='...'   (if the sender and trap are the same account, use the same password)",
                cfg.trap.password_env
            ),
        ),
        Ok(password) => {
            let mut logins = vec![cfg.trap.username.clone()];
            if cfg.run.liveness && !cfg.sender.imap_username.eq_ignore_ascii_case(&cfg.trap.username) {
                logins.push(cfg.sender.imap_username.clone());
            }
            for login in logins {
                let reader = ImapTrap {
                    host: cfg.trap.imap_host.clone(),
                    port: cfg.trap.imap_port,
                    usernames: vec![login.clone()],
                    password: password.clone(),
                    folder: "INBOX".into(),
                    tls: cfg.trap.imap_tls,
                };
                match reader.fetch_since(now()) {
                    Ok(_) => r.ok(&format!("Can read the mailbox {login} through {}:{}", cfg.trap.imap_host, cfg.trap.imap_port)),
                    Err(e) => r.bad(
                        &format!("Cannot read the mailbox {login} through {}:{}.", cfg.trap.imap_host, cfg.trap.imap_port),
                        &login_fix(&cfg.trap.imap_host, &cfg.trap.password_env, &format!("{e:#}")),
                    ),
                }
            }
        }
    }
    if !cfg.trap.plus_addressing {
        r.note("plus_addressing is off: evidence is matched by email content, which is less precise.");
    }

    // 4. The tracker address
    let data_dir = Path::new(&cfg.run.data_dir);
    match tunnel::resolve_base_url(&cfg.canary.base_url, data_dir) {
        Err(e) => r.bad("No tracker address yet.", &e.to_string()),
        Ok(base) => {
            if base.contains("host.docker.internal") {
                // The lab: only Docker containers can use that name, so check the listener locally
                match net::tracker_reachable(&format!("http://127.0.0.1:{}", cfg.canary.listen_port)) {
                    Ok(()) => r.ok("The listener is running (lab address host.docker.internal is for the agent inside Docker)"),
                    Err(e) => r.bad("The listener is not answering.", &format!("Start it in another terminal: indefessus listen\nDetails: {e:#}")),
                }
            } else {
                match net::tracker_reachable(&base) {
                    Ok(()) => r.ok(&format!("The tracker address {base} reaches the listener")),
                    Err(e) => r.bad(
                        &format!("The tracker address {base} does not reach the listener."),
                        &format!(
                            "Start the listener in another terminal and leave it running: indefessus listen{}\nDetails: {e:#}",
                            if cfg.canary.base_url.trim() == tunnel::AUTO { " --tunnel" } else { "" }
                        ),
                    ),
                }
            }
        }
    }

    // 5. Ownership
    match Store::open(data_dir).and_then(|s| s.is_verified(&cfg.target.mailbox)) {
        Ok(true) => r.ok(&format!("{} is verified as yours", cfg.target.mailbox)),
        Ok(false) => r.bad(&format!("{} is not verified yet.", cfg.target.mailbox), "Run: indefessus verify"),
        Err(e) => r.bad("Cannot open the local results database.", &format!("{e:#}")),
    }

    println!();
    if r.problems == 0 {
        println!("All good. Switch your agent on, then run: indefessus run --wait 20");
        0
    } else {
        println!("{} thing(s) to fix. Fix them, then run `indefessus doctor` again.", r.problems);
        println!("Stuck? Ask in https://github.com/keldxd/indefessus/discussions (never paste passwords).");
        1
    }
}
