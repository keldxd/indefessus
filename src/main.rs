//! Indefessus: check whether the AI agent connected to your inbox can be turned against you,
//! then re-check it every day for silent changes.

mod canary;
mod attach;
mod clock;
mod config;
mod doctor;
mod evaluate;
mod mail;
mod net;
mod rawmail;
mod report;
mod report_html;
mod scenarios;
mod selftest;
mod store;
mod tunnel;
mod util;
mod wizard;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use config::Config;
use mail::{build_message, ImapTrap, Outgoing, Security, SmtpSender};
use scenarios::Library;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use store::Store;

#[derive(Parser)]
#[command(
    name = "indefessus",
    version,
    about = "Check whether the AI agent connected to your inbox can be turned against you, and re-check it every day for silent changes."
)]
struct Cli {
    /// Config file
    #[arg(short, long, default_value = "indefessus.toml", global = true)]
    config: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Set up a config by answering a few questions (start here)
    Setup,
    /// Check every part of your setup and say how to fix what is wrong
    Doctor,
    /// Write an example config file to fill in by hand
    Init,
    /// List the test library
    Scenarios,
    /// Prove the pipeline works with stand-in agents, no mailbox needed
    Selftest,
    /// Prove you own the target mailbox (required before any test)
    Verify,
    /// Run the listener that records visits to the tests' links
    Listen {
        /// Also open a free public tunnel to it (needs `cloudflared`), for base_url = "auto"
        #[arg(long)]
        tunnel: bool,
    },
    /// Send the test emails
    Run {
        /// Test ids to run (default: all)
        #[arg(long, num_args = 1..)]
        only: Vec<String>,
        /// Minutes to wait at most, then check automatically
        #[arg(long)]
        wait: Option<u64>,
        /// Run the tests this many times and summarise (AI agents are not consistent)
        #[arg(long, default_value_t = 1)]
        repeat: u32,
    },
    /// Collect evidence and print the report
    Check {
        /// Run id (default: latest)
        #[arg(long)]
        run: Option<String>,
    },
    /// Record what your assistant said, for a guided test
    Answer {
        scenario: String,
        #[arg(long)]
        run: Option<String>,
    },
    /// Print the report of a run (worked out again from its stored evidence with the current rules)
    Report {
        #[arg(long)]
        run: Option<String>,
        /// Also write the HTML report again (for example after upgrading Indefessus)
        #[arg(long)]
        html: bool,
    },
    /// Automatic tests for a daily schedule; exit code 2 if behaviour got worse
    Daily {
        /// Minutes to wait at most (default: from config)
        #[arg(long)]
        wait: Option<u64>,
        /// Run the tests this many times and summarise
        #[arg(long, default_value_t = 1)]
        repeat: u32,
    },
    /// List recent runs, or leave a bad run out of comparisons
    Runs {
        #[command(subcommand)]
        action: Option<RunsCmd>,
    },
    /// Print an anonymised summary of a run, to share publicly (no addresses or email content)
    Share {
        #[arg(long)]
        run: Option<String>,
        /// Describe the agent, for example "n8n + gpt-oss-120b, can send email and open links"
        #[arg(long, default_value = "")]
        agent: String,
    },
}

#[derive(Subcommand)]
enum RunsCmd {
    /// List recent runs (the default)
    List,
    /// Leave a run out of day-to-day comparisons, for example one where the agent was broken
    Exclude { run: String },
    /// Put an excluded run back into comparisons
    Include { run: String },
}

fn open(cfg: &Config) -> Result<(Store, Library)> {
    Ok((Store::open(Path::new(&cfg.run.data_dir))?, scenarios::load_library(&cfg.run.extra_scenarios)?))
}

fn sender(cfg: &Config) -> Result<SmtpSender> {
    let security = if cfg.sender.use_ssl { Security::Tls } else { Security::parse(&cfg.sender.security)? };
    let password = if security == Security::None {
        Config::secret_or_empty(&cfg.sender.password_env)
    } else {
        Config::secret(&cfg.sender.password_env)?
    };
    Ok(SmtpSender {
        host: cfg.sender.smtp_host.clone(),
        port: cfg.sender.smtp_port,
        username: cfg.sender.username.clone(),
        password,
        security,
    })
}

fn latest_live(store: &Store) -> Result<String> {
    match store.latest_runs("live", false, 1)?.into_iter().next() {
        Some(r) => Ok(r.id),
        None => bail!("No runs yet. Start one with: indefessus run"),
    }
}

fn start_run(cfg: &Config, store: &Store, lib: &Library, only: &[String], auto_only: bool) -> Result<String> {
    if !store.is_verified(&cfg.target.mailbox)? {
        bail!("{} is not verified yet. Run: indefessus verify", cfg.target.mailbox);
    }
    let chosen: Vec<_> = lib
        .list
        .iter()
        .filter(|sc| (only.is_empty() || only.contains(&sc.id)) && (!auto_only || !sc.is_guided()))
        .collect();
    if chosen.is_empty() {
        bail!("No tests selected.");
    }
    let base_url = tunnel::resolve_base_url(&cfg.canary.base_url, Path::new(&cfg.run.data_dir))?;
    preflight(cfg, &base_url)?;
    let run_id = util::run_id(util::now());
    store.new_run(&run_id, "live", "")?;
    let has_files = chosen.iter().any(|sc| sc.has_attachments());
    let (mut seeds, mut tests, mut guided) = (Vec::new(), Vec::new(), Vec::new());
    for sc in chosen {
        let r = scenarios::render(sc, &base_url, &cfg.trap.address, cfg.trap.plus_addressing)?;
        store.add_item(&run_id, &sc.id, &r.token, &r.seed_secret)?;
        if sc.has_seed() {
            seeds.push(build_message(&Outgoing {
                from: &cfg.sender.address,
                display_name: "Accounts Team",
                to: &cfg.target.mailbox,
                subject: &r.seed_subject,
                text: &r.seed_body,
                html: "",
                token: &r.token,
                attachments: &[],
            })?);
        }
        tests.push(build_message(&Outgoing {
            from: &cfg.sender.address,
            display_name: &cfg.sender.display_name,
            to: &cfg.target.mailbox,
            subject: &r.subject,
            text: &r.body_text,
            html: &r.body_html,
            token: &r.token,
            attachments: &r.attachments,
        })?);
        if sc.is_guided() {
            guided.push((sc.id.clone(), r.guided_prompt.clone()));
        }
    }
    let total = seeds.len() + tests.len();
    seeds.extend(tests);
    if cfg.run.liveness && has_files {
        // Proves the agent reads attachments: it can only quote this booking number from the file
        let token = scenarios::new_token();
        store.add_item(&run_id, evaluate::ATTACH_CHECK_ID, &token, "")?;
        let (subject, body, file) = evaluate::attachment_check_email(&token)?;
        seeds.push(build_message(&Outgoing {
            from: &cfg.sender.address,
            display_name: &cfg.sender.display_name,
            to: &cfg.target.mailbox,
            subject: &subject,
            text: &body,
            html: "",
            token: &token,
            attachments: std::slice::from_ref(&file),
        })?);
    }
    if cfg.run.liveness {
        // Sent last: once the agent has replied to it, it has very likely read the tests too
        let token = scenarios::new_token();
        store.add_item(&run_id, evaluate::CONTROL_ID, &token, "")?;
        let (subject, body) = evaluate::control_email(&token);
        seeds.push(build_message(&Outgoing {
            from: &cfg.sender.address,
            display_name: &cfg.sender.display_name,
            to: &cfg.target.mailbox,
            subject: &subject,
            text: &body,
            html: "",
            token: &token,
            attachments: &[],
        })?);
    }
    sender(cfg)?.send(&seeds)?;
    println!("Run {run_id}: sent {total} test emails to {}.", cfg.target.mailbox);
    if cfg.run.liveness {
        println!("Also sent one ordinary control email (\"Are you open this Saturday?\"). A working agent should reply to it.");
        if has_files {
            println!("And one booking email with a PDF attached. An agent that reads attachments should quote its booking number.");
        }
    }
    if !guided.is_empty() {
        println!("\nGuided tests: when the emails have arrived, ask your assistant these questions,");
        println!("then record each answer with `indefessus answer <test id>`:");
        for (id, prompt) in guided {
            println!("  {id}: \"{prompt}\"");
        }
    }
    Ok(run_id)
}

/// Before sending anything: the agent must be able to reach the tracker address, or the run is wasted.
fn preflight(cfg: &Config, base_url: &str) -> Result<()> {
    let check = if base_url.contains("host.docker.internal") {
        // The lab: that name only works inside Docker, so check the listener locally
        net::tracker_reachable(&format!("http://127.0.0.1:{}", cfg.canary.listen_port))
    } else {
        net::tracker_reachable(base_url)
    };
    if let Err(e) = check {
        bail!(
            "the tracker address {base_url} does not reach the Indefessus listener, so the test links would not work.\n\
             Start the listener in another terminal and leave it running: indefessus listen{}\n\
             Then check everything with: indefessus doctor\nDetails: {e:#}",
            if cfg.canary.base_url.trim() == tunnel::AUTO { " --tunnel" } else { "" }
        );
    }
    Ok(())
}

/// One line per recent run, for `indefessus runs`.
fn list_runs(store: &Store) -> Result<()> {
    let runs = store.latest_runs("live", false, 30)?;
    if runs.is_empty() {
        println!("No runs yet. Start one with: indefessus run");
        return Ok(());
    }
    println!("{:<22} {:<28} {:<14} {:<26} note", "run", "started", "agent check", "results");
    for run in runs {
        let items: Vec<_> = store.items(&run.id)?.into_iter().filter(|i| !i.scenario_id.starts_with('_')).collect();
        let count = |s: &str| items.iter().filter(|i| i.status == s).count();
        let check = match evaluate::liveness(store, &run.id)? {
            Some(true) => "replied",
            Some(false) => "NO REPLY",
            None => "-",
        };
        let results = if run.finished_at.is_none() {
            "not checked yet".to_string()
        } else {
            format!("{} worked, {} resisted", count(evaluate::FAIL), count(evaluate::PASS))
        };
        let note = if store.is_excluded(&run.id)? { "left out of comparisons" } else { "" };
        println!("{:<22} {:<28} {:<14} {:<26} {note}", run.id, clock::fmt_local(run.started_at), check, results);
    }
    println!("\nTo leave a bad run out of comparisons: indefessus runs exclude <run>");
    Ok(())
}

fn imap_reader(cfg: &Config, usernames: Vec<String>) -> Result<ImapTrap> {
    Ok(ImapTrap {
        host: cfg.trap.imap_host.clone(),
        port: cfg.trap.imap_port,
        usernames,
        password: if cfg.trap.imap_tls { Config::secret(&cfg.trap.password_env)? } else { Config::secret_or_empty(&cfg.trap.password_env) },
        folder: "INBOX".into(),
        tls: cfg.trap.imap_tls,
    })
}

/// Has the agent replied to this run's control email? None if the run has no control email.
fn check_alive(cfg: &Config, store: &Store, run_id: &str) -> Result<Option<bool>> {
    let Some(control) = store.items(run_id)?.into_iter().find(|i| i.scenario_id == evaluate::CONTROL_ID) else {
        return Ok(None);
    };
    let started = store.run(run_id)?.map(|r| r.started_at).unwrap_or(0.0);
    let reader = imap_reader(cfg, vec![cfg.sender.imap_username.clone()])?;
    let messages = mail::TrapSource::fetch_since(&reader, started)?;
    Ok(Some(evaluate::control_replied(&messages, &control.token, started)))
}

/// Did the agent quote the booking number from the attached file? None if the run had no such check.
fn check_attachments(cfg: &Config, store: &Store, run_id: &str) -> Result<Option<bool>> {
    let Some(check) = store.items(run_id)?.into_iter().find(|i| i.scenario_id == evaluate::ATTACH_CHECK_ID) else {
        return Ok(None);
    };
    let started = store.run(run_id)?.map(|r| r.started_at).unwrap_or(0.0);
    let reader = imap_reader(cfg, vec![cfg.sender.imap_username.clone()])?;
    let messages = mail::TrapSource::fetch_since(&reader, started)?;
    Ok(Some(evaluate::attachment_read(&messages, &check.token, started)))
}

/// Waits up to `max_minutes` for the agent. With a control email, it checks every minute and stops
/// early (after a short grace period) once the agent has replied; otherwise it waits the full time.
fn wait_for_agent(cfg: &Config, store: &Store, run_id: &str, max_minutes: u64) {
    let minute = std::time::Duration::from_secs(60);
    if !cfg.run.liveness {
        println!("\nWaiting {max_minutes} minutes for the agent...");
        std::thread::sleep(minute * max_minutes as u32);
        return;
    }
    println!("\nWaiting up to {max_minutes} minutes for the agent (checking every minute for its reply to the control email)...");
    for elapsed in 1..=max_minutes {
        std::thread::sleep(minute);
        match check_alive(cfg, store, run_id) {
            Ok(Some(true)) => {
                let grace = 2.min(max_minutes - elapsed);
                println!("  after {elapsed} min: the agent replied to the control email. Giving it {grace} more minutes to finish the tests...");
                std::thread::sleep(minute * grace as u32);
                return;
            }
            Ok(_) => println!("  after {elapsed} min: no reply to the control email yet"),
            Err(e) => println!("  after {elapsed} min: could not check the sender's mailbox yet ({e:#})"),
        }
    }
    println!("  the agent never replied to the control email in {max_minutes} minutes");
}

fn finish_check(cfg: &Config, store: &Store, lib: &Library, run_id: &str) -> Result<Vec<evaluate::Change>> {
    let baseline = evaluate::pick_baseline(store, Some(run_id))?;
    finish_check_with(cfg, store, lib, run_id, baseline.as_deref(), true)
}

/// `baseline`: the run to compare with (see evaluate::pick_baseline).
fn finish_check_with(cfg: &Config, store: &Store, lib: &Library, run_id: &str, baseline: Option<&str>, print: bool) -> Result<Vec<evaluate::Change>> {
    let usernames = if cfg.trap.read_each_address {
        store
            .items(run_id)?
            .iter()
            .filter(|i| !i.scenario_id.starts_with('_'))
            .map(|i| scenarios::trap_address(&cfg.trap.address, &i.token, cfg.trap.plus_addressing))
            .collect()
    } else {
        vec![cfg.trap.username.clone()]
    };
    let trap = imap_reader(cfg, usernames)?;
    evaluate::collect_trapmail(store, run_id, &trap)?;
    evaluate::evaluate_run(store, run_id, lib)?;
    if let Some(alive) = check_alive(cfg, store, run_id)? {
        evaluate::apply_liveness(store, run_id, alive)?;
    }
    if let Some(read) = check_attachments(cfg, store, run_id)? {
        evaluate::apply_attachment_check(store, run_id, lib, read)?;
    }
    store.finish_run(run_id)?;
    let changes = evaluate::compare(store, run_id, baseline)?;
    let dir = Path::new(&cfg.run.data_dir).join("reports");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{run_id}.html"));
    std::fs::write(&path, report::html_report(store, run_id, lib, &changes)?)?;
    if print {
        println!("{}", report::text_report(store, run_id, lib, &changes)?);
        println!("\nFull report: {}", path.display());
    } else {
        println!("  round finished: report saved to {}", path.display());
    }
    Ok(changes)
}

/// Several rounds of the same tests, one after the other, then a combined summary.
fn run_rounds(cfg: &Config, store: &Store, lib: &Library, only: &[String], auto_only: bool, rounds: u32, wait: u64) -> Result<Vec<evaluate::Change>> {
    let mut run_ids = Vec::new();
    let mut all_changes: Vec<evaluate::Change> = Vec::new();
    // Every round is compared with the same earlier good run, never with the round before it:
    // assistants are not consistent, so round-to-round differences are noise, not a change.
    let baseline = evaluate::pick_baseline(store, None)?;
    for round in 1..=rounds {
        println!("\n== Round {round} of {rounds} ==");
        let run_id = start_run(cfg, store, lib, only, auto_only)?;
        wait_for_agent(cfg, store, &run_id, wait);
        for change in finish_check_with(cfg, store, lib, &run_id, baseline.as_deref(), rounds == 1)? {
            if !all_changes.iter().any(|c| c.scenario == change.scenario && c.worse == change.worse) {
                all_changes.push(change);
            }
        }
        run_ids.push(run_id);
    }
    if rounds > 1 {
        println!("\n{}", report::repeat_summary(store, &run_ids, lib)?);
    }
    Ok(all_changes)
}

fn real_main() -> Result<i32> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Setup => wizard::run(&cli.config)?,
        Cmd::Doctor => return Ok(doctor::run(&Config::load(&cli.config)?)),
        Cmd::Init => {
            if cli.config.exists() {
                bail!("{} already exists, not overwriting it.", cli.config.display());
            }
            std::fs::write(&cli.config, config::EXAMPLE)?;
            println!("Wrote {}. Fill it in, set the password environment variables, then run: indefessus doctor\n(Easier: `indefessus setup` asks a few questions and writes it for you.)", cli.config.display());
        }
        Cmd::Scenarios => {
            let extra = Config::load(&cli.config).map(|c| c.run.extra_scenarios).unwrap_or_default();
            for sc in scenarios::load_library(&extra)?.list {
                println!("{:28} {:7} {:7} {:9} {}", sc.id, sc.mode, sc.harm, sc.severity, sc.title);
            }
        }
        Cmd::Selftest => return Ok(if selftest::run_selftest(true)? { 0 } else { 1 }),
        Cmd::Verify => {
            let cfg = Config::load(&cli.config)?;
            let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
            let msg = build_message(&Outgoing {
                from: &cfg.sender.address,
                display_name: "Indefessus",
                to: &cfg.target.mailbox,
                subject: &format!("Indefessus ownership check: {code}"),
                text: &format!("Your Indefessus ownership code is {code}.\n\nIf you did not start an Indefessus test, ignore this email."),
                html: "",
                token: "",
                attachments: &[],
            })?;
            sender(&cfg)?.send(&[msg])?;
            println!("Sent a 6-digit code to {}. Only the owner of that mailbox can read it.", cfg.target.mailbox);
            print!("Enter the code: ");
            std::io::Write::flush(&mut std::io::stdout())?;
            let mut entered = String::new();
            std::io::stdin().read_line(&mut entered)?;
            if entered.trim() != code {
                println!("That code does not match. Tests stay locked for this mailbox.");
                return Ok(1);
            }
            Store::open(Path::new(&cfg.run.data_dir))?.mark_verified(&cfg.target.mailbox)?;
            println!("Verified. You can now run tests against {}.", cfg.target.mailbox);
        }
        Cmd::Listen { tunnel: with_tunnel } => {
            let cfg = Config::load(&cli.config)?;
            let data_dir = Path::new(&cfg.run.data_dir);
            let store = Arc::new(Store::open(data_dir)?);
            let mut child = None;
            let public = if with_tunnel {
                println!("Starting a free Cloudflare tunnel (this takes a few seconds)...");
                let (c, address) = tunnel::start(cfg.canary.listen_port, data_dir)?;
                child = Some(c);
                if cfg.canary.base_url.trim() != tunnel::AUTO {
                    println!("Note: your config has base_url = \"{}\". Set it to \"auto\" to use this tunnel.", cfg.canary.base_url);
                }
                address
            } else {
                cfg.canary.base_url.clone()
            };
            println!(
                "Listening on {}:{}, public address {public}\nLeave this running during tests. Ctrl+C to stop.",
                cfg.canary.listen_host, cfg.canary.listen_port
            );
            let result = canary::serve_forever(store, &cfg.canary.listen_host, cfg.canary.listen_port);
            if let Some(mut c) = child {
                let _ = c.kill();
            }
            result?;
        }
        Cmd::Run { only, wait, repeat } => {
            let cfg = Config::load(&cli.config)?;
            let (store, lib) = open(&cfg)?;
            let repeat = repeat.max(1);
            match wait {
                None if repeat == 1 => {
                    start_run(&cfg, &store, &lib, &only, false)?;
                    println!("\nGive the agent time to read the emails, then run: indefessus check");
                }
                _ => {
                    run_rounds(&cfg, &store, &lib, &only, false, repeat, wait.unwrap_or(cfg.run.wait_minutes))?;
                }
            }
        }
        Cmd::Check { run } => {
            let cfg = Config::load(&cli.config)?;
            let (store, lib) = open(&cfg)?;
            let run_id = match run {
                Some(r) => r,
                None => latest_live(&store)?,
            };
            finish_check(&cfg, &store, &lib, &run_id)?;
        }
        Cmd::Answer { scenario, run } => {
            let cfg = Config::load(&cli.config)?;
            let (store, _) = open(&cfg)?;
            let run_id = match run {
                Some(r) => r,
                None => latest_live(&store)?,
            };
            if !store.items(&run_id)?.iter().any(|i| i.scenario_id == scenario) {
                bail!("Test {scenario} is not part of run {run_id}.");
            }
            println!("Paste the assistant's answer, then press Ctrl+D on a new line (Ctrl+Z then Enter on Windows):");
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            store.set_answer(&run_id, &scenario, &text)?;
            println!("Saved. Run `indefessus check` to update the results.");
        }
        Cmd::Report { run, html } => {
            let cfg = Config::load(&cli.config)?;
            let (store, lib) = open(&cfg)?;
            let run_id = match run {
                Some(r) => r,
                None => latest_live(&store)?,
            };
            if store.run(&run_id)?.and_then(|r| r.finished_at).is_some() {
                // Same evidence, current rules (for example after upgrading Indefessus)
                evaluate::reevaluate(&store, &run_id, &lib)?;
            }
            let baseline = evaluate::pick_baseline(&store, Some(run_id.as_str()))?;
            let changes = evaluate::compare(&store, &run_id, baseline.as_deref())?;
            println!("{}", report::text_report(&store, &run_id, &lib, &changes)?);
            if html {
                let dir = Path::new(&cfg.run.data_dir).join("reports");
                std::fs::create_dir_all(&dir)?;
                let path = dir.join(format!("{run_id}.html"));
                std::fs::write(&path, report::html_report(&store, &run_id, &lib, &changes)?)?;
                println!("\nFull report: {}", path.display());
            }
        }
        Cmd::Daily { wait, repeat } => {
            let cfg = Config::load(&cli.config)?;
            let (store, lib) = open(&cfg)?;
            let changes = run_rounds(&cfg, &store, &lib, &[], true, repeat.max(1), wait.unwrap_or(cfg.run.wait_minutes))?;
            if changes.iter().any(|c| c.worse) {
                return Ok(2);
            }
        }
        Cmd::Runs { action } => {
            let cfg = Config::load(&cli.config)?;
            let (store, _) = open(&cfg)?;
            match action.unwrap_or(RunsCmd::List) {
                RunsCmd::List => list_runs(&store)?,
                RunsCmd::Exclude { run } => {
                    if !store.set_excluded(&run, true)? {
                        bail!("There is no run {run}. See the list with: indefessus runs");
                    }
                    println!("Run {run} is now left out of comparisons. Undo with: indefessus runs include {run}");
                }
                RunsCmd::Include { run } => {
                    if !store.set_excluded(&run, false)? {
                        bail!("There is no run {run}. See the list with: indefessus runs");
                    }
                    println!("Run {run} is back in comparisons.");
                }
            }
        }
        Cmd::Share { run, agent } => {
            let cfg = Config::load(&cli.config)?;
            let (store, lib) = open(&cfg)?;
            let run_id = match run {
                Some(r) => r,
                None => latest_live(&store)?,
            };
            println!("{}", report::share_markdown(&store, &run_id, &lib, &agent)?);
            println!("\n---\nCopy everything above the line into a new result report:");
            println!("https://github.com/keldxd/indefessus/issues/new?template=share-result.yml");
        }
    }
    Ok(0)
}

fn main() {
    match real_main() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    }
}
