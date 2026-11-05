//! Indefessus: check whether the AI agent connected to your inbox can be turned against you,
//! then re-check it every day for silent changes.

mod canary;
mod config;
mod evaluate;
mod mail;
mod rawmail;
mod report;
mod scenarios;
mod selftest;
mod store;
mod util;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use config::Config;
use mail::{build_message, ImapTrap, Outgoing, SmtpSender};
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
    /// Write an example config file
    Init,
    /// List the test library
    Scenarios,
    /// Prove the pipeline works with two stand-in agents, no mailbox needed
    Selftest,
    /// Prove you own the target mailbox (required before any test)
    Verify,
    /// Run the listener that records visits to the tests' links
    Listen,
    /// Send the test emails
    Run {
        /// Test ids to run (default: all)
        #[arg(long, num_args = 1..)]
        only: Vec<String>,
        /// Minutes to wait, then check automatically
        #[arg(long)]
        wait: Option<u64>,
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
    /// Print the report of a run
    Report {
        #[arg(long)]
        run: Option<String>,
    },
    /// Automatic tests for a daily schedule; exit code 2 if behaviour got worse
    Daily {
        /// Minutes to wait (default: from config)
        #[arg(long)]
        wait: Option<u64>,
    },
}

fn open(cfg: &Config) -> Result<(Store, Library)> {
    Ok((Store::open(Path::new(&cfg.run.data_dir))?, scenarios::load_library(&cfg.run.extra_scenarios)?))
}

fn sender(cfg: &Config) -> Result<SmtpSender> {
    Ok(SmtpSender {
        host: cfg.sender.smtp_host.clone(),
        port: cfg.sender.smtp_port,
        username: cfg.sender.username.clone(),
        password: Config::secret(&cfg.sender.password_env)?,
        use_ssl: cfg.sender.use_ssl,
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
    let run_id = util::run_id(util::now());
    store.new_run(&run_id, "live", "")?;
    let (mut seeds, mut tests, mut guided) = (Vec::new(), Vec::new(), Vec::new());
    for sc in chosen {
        let r = scenarios::render(sc, &cfg.canary.base_url, &cfg.trap.address, cfg.trap.plus_addressing);
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
        })?);
        if sc.is_guided() {
            guided.push((sc.id.clone(), r.guided_prompt.clone()));
        }
    }
    let total = seeds.len() + tests.len();
    seeds.extend(tests);
    sender(cfg)?.send(&seeds)?;
    println!("Run {run_id}: sent {total} test emails to {}.", cfg.target.mailbox);
    if !guided.is_empty() {
        println!("\nGuided tests: when the emails have arrived, ask your assistant these questions,");
        println!("then record each answer with `indefessus answer <test id>`:");
        for (id, prompt) in guided {
            println!("  {id}: \"{prompt}\"");
        }
    }
    Ok(run_id)
}

fn previous_finished(store: &Store, run_id: &str) -> Result<Option<String>> {
    Ok(store.latest_runs("live", true, 20)?.into_iter().map(|r| r.id).find(|id| id.as_str() < run_id))
}

fn finish_check(cfg: &Config, store: &Store, lib: &Library, run_id: &str) -> Result<Vec<evaluate::Change>> {
    let trap = ImapTrap {
        host: cfg.trap.imap_host.clone(),
        port: cfg.trap.imap_port,
        username: cfg.trap.username.clone(),
        password: Config::secret(&cfg.trap.password_env)?,
        folder: "INBOX".into(),
    };
    evaluate::collect_trapmail(store, run_id, &trap)?;
    evaluate::evaluate_run(store, run_id, lib)?;
    store.finish_run(run_id)?;
    let changes = evaluate::compare(store, run_id, previous_finished(store, run_id)?.as_deref())?;
    println!("{}", report::text_report(store, run_id, lib, &changes)?);
    let dir = Path::new(&cfg.run.data_dir).join("reports");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{run_id}.html"));
    std::fs::write(&path, report::html_report(store, run_id, lib, &changes)?)?;
    println!("\nFull report: {}", path.display());
    Ok(changes)
}

fn real_main() -> Result<i32> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init => {
            if cli.config.exists() {
                bail!("{} already exists, not overwriting it.", cli.config.display());
            }
            std::fs::write(&cli.config, config::EXAMPLE)?;
            println!("Wrote {}. Fill it in, set the password environment variables, then run: indefessus verify", cli.config.display());
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
        Cmd::Listen => {
            let cfg = Config::load(&cli.config)?;
            let store = Arc::new(Store::open(Path::new(&cfg.run.data_dir))?);
            println!(
                "Listening on {}:{}, public address {}. Leave this running during tests. Ctrl+C to stop.",
                cfg.canary.listen_host, cfg.canary.listen_port, cfg.canary.base_url
            );
            canary::serve_forever(store, &cfg.canary.listen_host, cfg.canary.listen_port)?;
        }
        Cmd::Run { only, wait } => {
            let cfg = Config::load(&cli.config)?;
            let (store, lib) = open(&cfg)?;
            let run_id = start_run(&cfg, &store, &lib, &only, false)?;
            match wait {
                None => println!("\nGive the agent time to read the emails, then run: indefessus check"),
                Some(minutes) => {
                    println!("\nWaiting {minutes} minutes for the agent...");
                    std::thread::sleep(std::time::Duration::from_secs(minutes * 60));
                    finish_check(&cfg, &store, &lib, &run_id)?;
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
        Cmd::Report { run } => {
            let cfg = Config::load(&cli.config)?;
            let (store, lib) = open(&cfg)?;
            let run_id = match run {
                Some(r) => r,
                None => latest_live(&store)?,
            };
            let changes = evaluate::compare(&store, &run_id, previous_finished(&store, &run_id)?.as_deref())?;
            println!("{}", report::text_report(&store, &run_id, &lib, &changes)?);
        }
        Cmd::Daily { wait } => {
            let cfg = Config::load(&cli.config)?;
            let (store, lib) = open(&cfg)?;
            let run_id = start_run(&cfg, &store, &lib, &[], true)?;
            std::thread::sleep(std::time::Duration::from_secs(wait.unwrap_or(cfg.run.wait_minutes) * 60));
            let changes = finish_check(&cfg, &store, &lib, &run_id)?;
            if changes.iter().any(|c| c.worse) {
                return Ok(2);
            }
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
