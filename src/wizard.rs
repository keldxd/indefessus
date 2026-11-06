//! `indefessus setup`: a few plain questions, then a ready config file and the exact next steps.

use anyhow::{bail, Result};
use std::io::{BufRead, Write};
use std::path::Path;

fn ask(question: &str, default: &str) -> Result<String> {
    if default.is_empty() {
        print!("{question}: ");
    } else {
        print!("{question} [{default}]: ");
    }
    std::io::stdout().flush()?;
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        bail!("setup cancelled");
    }
    let answer = line.trim();
    Ok(if answer.is_empty() { default.to_string() } else { answer.to_string() })
}

fn ask_email(question: &str, default: &str) -> Result<String> {
    loop {
        let a = ask(question, default)?;
        if a.contains('@') && a.contains('.') && !a.contains(' ') {
            return Ok(a);
        }
        println!("  That doesn't look like an email address, please try again.");
    }
}

fn ask_choice(question: &str, options: &[&str], default: usize) -> Result<usize> {
    println!("{question}");
    for (i, o) in options.iter().enumerate() {
        println!("  {}) {o}", i + 1);
    }
    loop {
        let a = ask("Choose a number", &default.to_string())?;
        if let Ok(n) = a.parse::<usize>() {
            if (1..=options.len()).contains(&n) {
                return Ok(n);
            }
        }
        println!("  Please type a number from 1 to {}.", options.len());
    }
}

struct Mail {
    sender: String,
    smtp_host: String,
    smtp_port: u16,
    security: String,
    trap: String,
    imap_host: String,
    imap_port: u16,
    plus: bool,
}

pub fn run(config_path: &Path) -> Result<()> {
    println!("Indefessus setup. A few questions, then you're ready to test your agent.");
    println!("You need: the mailbox your AI agent reads, and a SECOND mailbox you control that plays the stranger.");
    println!("Never use the agent's own mailbox as the second one.\n");
    if config_path.exists() {
        let a = ask(&format!("{} already exists. Replace it? (yes/no)", config_path.display()), "no")?;
        if !a.eq_ignore_ascii_case("yes") && !a.eq_ignore_ascii_case("y") {
            println!("Nothing changed.");
            return Ok(());
        }
    }

    let target = ask_email("1. Which mailbox does your AI agent read? (the one to test)", "")?;

    let provider = ask_choice(
        "\n2. Which provider is your SECOND mailbox (it sends the tests and catches what the agent leaks)?",
        &["Gmail (recommended: free, supports one address per test)", "Another provider (I know its SMTP and IMAP settings)"],
        1,
    )?;
    let mail = if provider == 1 {
        let address = ask_email("   The second Gmail address", "")?;
        if address.eq_ignore_ascii_case(&target) {
            bail!("the second mailbox must be different from the agent's mailbox");
        }
        Mail {
            sender: address.clone(),
            smtp_host: "smtp.gmail.com".into(),
            smtp_port: 587,
            security: "starttls".into(),
            trap: address,
            imap_host: "imap.gmail.com".into(),
            imap_port: 993,
            plus: true,
        }
    } else {
        let sender = ask_email("   Address that sends the tests", "")?;
        let smtp_host = ask("   Its SMTP server (for example smtp.example.com)", "")?;
        let smtp_port: u16 = ask("   SMTP port (587 for STARTTLS, 465 for TLS)", "587")?.parse().unwrap_or(587);
        let security = if smtp_port == 465 { "tls" } else { "starttls" }.to_string();
        let trap = ask_email("   Address that catches leaks (can be the same as the sender)", &sender)?;
        let imap_host = ask("   Its IMAP server (for example imap.example.com)", "")?;
        let imap_port: u16 = ask("   IMAP port", "993")?.parse().unwrap_or(993);
        let plus = ask("   Does it support plus addresses like name+test@domain? (yes/no)", "yes")?.to_lowercase().starts_with('y');
        Mail { sender, smtp_host, smtp_port, security, trap, imap_host, imap_port, plus }
    };

    let tunnel = ask_choice(
        "\n3. The test links need a public address that reaches this computer.",
        &["Use a free automatic tunnel (recommended, needs the free `cloudflared` program)", "I have my own public address"],
        1,
    )?;
    let base_url = if tunnel == 1 { "auto".to_string() } else { ask("   Your public address (https://...)", "")? };

    let same_account = mail.sender.eq_ignore_ascii_case(&mail.trap);
    let config = format!(
        r#"# Indefessus configuration, written by `indefessus setup`. Passwords go in environment variables, never here.

[target]
mailbox = "{target}"

[sender]
address = "{sender}"
display_name = "Partner Office"
smtp_host = "{smtp_host}"
smtp_port = {smtp_port}
security = "{security}"
username = "{sender}"
password_env = "INDEFESSUS_SMTP_PASSWORD"

[trap]
address = "{trap}"
plus_addressing = {plus}
imap_host = "{imap_host}"
imap_port = {imap_port}
username = "{trap}"
password_env = "INDEFESSUS_TRAP_PASSWORD"

[canary]
base_url = "{base_url}"   # "auto" uses the address from `indefessus listen --tunnel`
listen_host = "0.0.0.0"
listen_port = 8787

[run]
data_dir = ".indefessus"
wait_minutes = 20
liveness = true
"#,
        target = target,
        sender = mail.sender,
        smtp_host = mail.smtp_host,
        smtp_port = mail.smtp_port,
        security = mail.security,
        trap = mail.trap,
        plus = mail.plus,
        imap_host = mail.imap_host,
        imap_port = mail.imap_port,
        base_url = base_url,
    );
    std::fs::write(config_path, config)?;
    let cfg_flag = if config_path == Path::new("indefessus.toml") { String::new() } else { format!(" -c {}", config_path.display()) };

    println!("\nWrote {}.\n", config_path.display());
    println!("Next steps:");
    let mut step = 1;
    if provider == 1 {
        println!("  {step}. Create a Gmail App Password for {} (needs 2-Step Verification):", mail.sender);
        println!("     https://myaccount.google.com/apppasswords");
        step += 1;
    }
    println!("  {step}. Put the password(s) in environment variables for this terminal (never in the file):");
    if same_account {
        println!("       Linux/macOS:  export INDEFESSUS_SMTP_PASSWORD='...'; export INDEFESSUS_TRAP_PASSWORD=\"$INDEFESSUS_SMTP_PASSWORD\"");
        println!("       Windows:      $env:INDEFESSUS_SMTP_PASSWORD='...'; $env:INDEFESSUS_TRAP_PASSWORD=$env:INDEFESSUS_SMTP_PASSWORD");
    } else {
        println!("       Linux/macOS:  export INDEFESSUS_SMTP_PASSWORD='...'; export INDEFESSUS_TRAP_PASSWORD='...'");
        println!("       Windows:      $env:INDEFESSUS_SMTP_PASSWORD='...'; $env:INDEFESSUS_TRAP_PASSWORD='...'");
    }
    step += 1;
    if tunnel == 1 {
        println!("  {step}. In a second terminal, start the listener with a free tunnel and leave it running:");
        println!("       indefessus{cfg_flag} listen --tunnel");
    } else {
        println!("  {step}. In a second terminal, start the listener and leave it running:");
        println!("       indefessus{cfg_flag} listen");
    }
    step += 1;
    println!("  {step}. Check that everything works:    indefessus{cfg_flag} doctor");
    step += 1;
    println!("  {step}. Prove you own {target}:        indefessus{cfg_flag} verify");
    step += 1;
    println!("  {step}. Test your agent (it must be switched on):  indefessus{cfg_flag} run --wait 20");
    println!("\nOnly test mailboxes and agents you own or are allowed to test.");
    Ok(())
}
