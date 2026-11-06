//! A tiny HTTP(S) client, only used to check that the tracker address can be reached.

use anyhow::{anyhow, bail, Context, Result};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub struct Url {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}

pub fn parse_url(url: &str) -> Result<Url> {
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        bail!("the address must start with http:// or https://: {url}");
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        bail!("no host in {url}");
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') || h.ends_with(']') => (h.to_string(), p.parse().map_err(|_| anyhow!("bad port in {url}"))?),
        _ => (authority.to_string(), if https { 443 } else { 80 }),
    };
    Ok(Url { https, host, port, path: path.to_string() })
}

/// GET a URL and return the status code and body (at most 64 KB).
pub fn get(url: &str, timeout_secs: u64) -> Result<(u16, String)> {
    let u = parse_url(url)?;
    let timeout = Duration::from_secs(timeout_secs);
    let addr = (u.host.as_str(), u.port)
        .to_socket_addrs()
        .with_context(|| format!("cannot find {}", u.host))?
        .next()
        .ok_or_else(|| anyhow!("cannot find {}", u.host))?;
    let tcp = TcpStream::connect_timeout(&addr, timeout).with_context(|| format!("cannot connect to {}:{}", u.host, u.port))?;
    tcp.set_read_timeout(Some(timeout))?;
    tcp.set_write_timeout(Some(timeout))?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: indefessus-check\r\nAccept: text/html\r\nConnection: close\r\n\r\n",
        u.path, u.host
    );
    let mut raw = Vec::new();
    if u.https {
        let connector = native_tls::TlsConnector::new()?;
        let mut tls = connector.connect(&u.host, tcp).map_err(|e| anyhow!("secure connection to {} failed: {e}", u.host))?;
        tls.write_all(request.as_bytes())?;
        let _ = tls.take(65_536).read_to_end(&mut raw);
    } else {
        let mut tcp = tcp;
        tcp.write_all(request.as_bytes())?;
        let _ = tcp.take(65_536).read_to_end(&mut raw);
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("no valid answer from {url}"))?;
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
    Ok((status, body))
}

/// Is the Indefessus listener answering at this tracker address?
/// A brand-new tunnel address can take a few seconds to work, so failures are retried briefly.
pub fn tracker_reachable(base_url: &str) -> Result<()> {
    let mut last = None;
    for attempt in 0..4 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(5));
        }
        match tracker_once(base_url) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("no answer from {base_url}")))
}

fn tracker_once(base_url: &str) -> Result<()> {
    let url = format!("{}/c/{}/check", base_url.trim_end_matches('/'), crate::canary::HEALTH_TOKEN);
    let (status, body) = get(&url, 10)?;
    if status != 200 || !body.contains("security test") {
        bail!("{base_url} answered (status {status}), but not with the Indefessus listener page");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let u = parse_url("https://abc-def.trycloudflare.com").unwrap();
        assert!(u.https && u.host == "abc-def.trycloudflare.com" && u.port == 443 && u.path == "/");
        let u = parse_url("http://host.docker.internal:8787/c/x?y=1").unwrap();
        assert!(!u.https && u.host == "host.docker.internal" && u.port == 8787 && u.path == "/c/x?y=1");
        assert!(parse_url("ftp://x").is_err());
    }
}
