//! A free public address for the listener, using a Cloudflare quick tunnel (no account needed).
//!
//! `indefessus listen --tunnel` starts `cloudflared`, reads the address it is given and saves it
//! in the data folder. A config with `base_url = "auto"` then uses that address for the test links.

use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub const AUTO: &str = "auto";

pub fn address_file(data_dir: &Path) -> PathBuf {
    data_dir.join("tunnel-address")
}

pub fn install_help() -> &'static str {
    "The free tunnel needs the `cloudflared` program (no account needed). Install it with:\n\
     \x20 Windows:  winget install --id Cloudflare.cloudflared\n\
     \x20 macOS:    brew install cloudflared\n\
     \x20 Linux:    see https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/\n\
     Then run `indefessus listen --tunnel` again."
}

/// Finds a quick-tunnel address such as https://words-words.trycloudflare.com in a line of output.
pub fn find_address(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '|' || c == '"').unwrap_or(rest.len());
    let url = &rest[..end];
    if url.ends_with(".trycloudflare.com") {
        Some(url.to_string())
    } else {
        None
    }
}

/// Starts a quick tunnel to the local listener and returns the running process and its public address.
pub fn start(local_port: u16, data_dir: &Path) -> Result<(Child, String)> {
    let _ = std::fs::remove_file(address_file(data_dir));
    let mut child = match Command::new("cloudflared")
        .args(["tunnel", "--no-autoupdate", "--url", &format!("http://127.0.0.1:{local_port}")])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!("{}", install_help()),
        Err(e) => return Err(e).context("could not start cloudflared"),
    };
    let stderr = child.stderr.take().context("no output from cloudflared")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut sent = false;
        for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            if !sent {
                if let Some(url) = find_address(&line) {
                    let _ = tx.send(url);
                    sent = true;
                }
            }
            // keep reading so cloudflared never blocks on a full pipe
        }
    });
    let address = match rx.recv_timeout(Duration::from_secs(45)) {
        Ok(a) => a,
        Err(_) => {
            let _ = child.kill();
            bail!("cloudflared did not give a tunnel address within 45 seconds. Check your internet connection and try again.");
        }
    };
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(address_file(data_dir), &address)?;
    Ok((child, address))
}

/// The tracker address to use: the configured one, or the saved tunnel address for "auto".
pub fn resolve_base_url(configured: &str, data_dir: &Path) -> Result<String> {
    if configured.trim() != AUTO {
        return Ok(configured.trim_end_matches('/').to_string());
    }
    match std::fs::read_to_string(address_file(data_dir)) {
        Ok(a) if !a.trim().is_empty() => Ok(a.trim().to_string()),
        _ => bail!("base_url is \"auto\", but no tunnel is running. In another terminal, start: indefessus listen --tunnel"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tunnel_address() {
        let line = "2026-11-06T10:00:00Z INF |  https://brave-lion-river-tree.trycloudflare.com                    |";
        assert_eq!(find_address(line).as_deref(), Some("https://brave-lion-river-tree.trycloudflare.com"));
        assert_eq!(find_address("INF Requesting new quick Tunnel on trycloudflare.com..."), None);
        assert_eq!(find_address("see https://www.cloudflare.com/website-terms/"), None);
    }

    #[test]
    fn resolves_auto() {
        let dir = std::env::temp_dir().join(format!("idf-tunnel-{}", rand::random::<u32>()));
        assert_eq!(resolve_base_url("https://x.example/", &dir).unwrap(), "https://x.example");
        assert!(resolve_base_url("auto", &dir).is_err());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(address_file(&dir), "https://a-b.trycloudflare.com\n").unwrap();
        assert_eq!(resolve_base_url("auto", &dir).unwrap(), "https://a-b.trycloudflare.com");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
