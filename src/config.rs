//! Configuration. Passwords are never stored in the file: they are read from environment variables.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;

pub const EXAMPLE: &str = r#"# Indefessus configuration. Passwords go in environment variables, never in this file.

[target]
# The mailbox your AI agent reads. It must be yours (indefessus verify checks this).
mailbox = "office@your-org.example"

[sender]
# An address YOU control that plays the "stranger" sending the test emails.
# Use a separate mailbox from the target, for example a fresh free email account.
address = "tester@your-other-domain.example"
display_name = "Partner Office"
smtp_host = "smtp.example.com"
smtp_port = 587          # 587 with STARTTLS, or 465 with use_ssl = true
use_ssl = false
username = "tester@your-other-domain.example"
password_env = "INDEFESSUS_SMTP_PASSWORD"

[trap]
# A mailbox YOU control. If the agent sends mail here, that is evidence it obeyed a stranger.
address = "trap@your-other-domain.example"
plus_addressing = true   # trap+token@... gives each test its own address (Gmail and most providers support it)
imap_host = "imap.example.com"
imap_port = 993
username = "trap@your-other-domain.example"
password_env = "INDEFESSUS_TRAP_PASSWORD"

[canary]
# A public address that reaches `indefessus listen` (for example through a tunnel).
base_url = "https://your-tunnel.example"
listen_host = "0.0.0.0"
listen_port = 8787

[run]
data_dir = ".indefessus"
wait_minutes = 30        # how long to give the agent before checking
extra_scenarios = []     # folders with your own test files
"#;

fn d_587() -> u16 {
    587
}
fn d_993() -> u16 {
    993
}
fn d_8787() -> u16 {
    8787
}
fn d_true() -> bool {
    true
}
fn d_30() -> u64 {
    30
}
fn d_data() -> String {
    ".indefessus".into()
}
fn d_host() -> String {
    "0.0.0.0".into()
}
fn d_display() -> String {
    "Partner Office".into()
}
fn d_smtp_env() -> String {
    "INDEFESSUS_SMTP_PASSWORD".into()
}
fn d_trap_env() -> String {
    "INDEFESSUS_TRAP_PASSWORD".into()
}

#[derive(Debug, Deserialize)]
pub struct Target {
    pub mailbox: String,
}

#[derive(Debug, Deserialize)]
pub struct Sender {
    pub address: String,
    #[serde(default = "d_display")]
    pub display_name: String,
    pub smtp_host: String,
    #[serde(default = "d_587")]
    pub smtp_port: u16,
    #[serde(default)]
    pub use_ssl: bool,
    #[serde(default)]
    pub username: String,
    #[serde(default = "d_smtp_env")]
    pub password_env: String,
}

#[derive(Debug, Deserialize)]
pub struct Trap {
    pub address: String,
    #[serde(default = "d_true")]
    pub plus_addressing: bool,
    pub imap_host: String,
    #[serde(default = "d_993")]
    pub imap_port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default = "d_trap_env")]
    pub password_env: String,
}

#[derive(Debug, Deserialize)]
pub struct Canary {
    pub base_url: String,
    #[serde(default = "d_host")]
    pub listen_host: String,
    #[serde(default = "d_8787")]
    pub listen_port: u16,
}

#[derive(Debug, Deserialize)]
pub struct Run {
    #[serde(default = "d_data")]
    pub data_dir: String,
    #[serde(default = "d_30")]
    pub wait_minutes: u64,
    #[serde(default)]
    pub extra_scenarios: Vec<String>,
}

impl Default for Run {
    fn default() -> Self {
        Run { data_dir: d_data(), wait_minutes: d_30(), extra_scenarios: vec![] }
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub target: Target,
    pub sender: Sender,
    pub trap: Trap,
    pub canary: Canary,
    #[serde(default)]
    pub run: Run,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        if !path.exists() {
            bail!("No config at {}. Create one with: indefessus init", path.display());
        }
        let text = std::fs::read_to_string(path)?;
        let mut cfg: Config = toml::from_str(&text).with_context(|| format!("{} is not a valid config", path.display()))?;
        if cfg.sender.username.is_empty() {
            cfg.sender.username = cfg.sender.address.clone();
        }
        if cfg.trap.username.is_empty() {
            cfg.trap.username = cfg.trap.address.clone();
        }
        Ok(cfg)
    }

    pub fn secret(env_name: &str) -> Result<String> {
        match std::env::var(env_name) {
            Ok(v) if !v.is_empty() => Ok(v),
            _ => bail!("Set the environment variable {env_name} (passwords are never stored in the config file)."),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn example_config_parses() {
        let cfg: super::Config = toml::from_str(super::EXAMPLE).unwrap();
        assert_eq!(cfg.sender.smtp_port, 587);
        assert_eq!(cfg.run.wait_minutes, 30);
    }
}
