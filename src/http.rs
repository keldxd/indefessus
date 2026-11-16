//! A small HTTP server for the tracker listener. Standard library only.
//!
//! The listener faces the internet while a test runs, so every wait has a deadline and every
//! read has a size limit. Until 0.3.0 the listener used tiny_http, which has no socket timeouts:
//! a few clients sending a request very slowly could hold every worker, and the agent's own
//! visit would then go unrecorded, which would make a trick that worked look resisted.
//!
//! Each connection carries one request and is then closed.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How much the server accepts, and for how long it waits.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Request line and headers together
    pub header_bytes: usize,
    /// Body bytes kept; the rest of a larger body is never read
    pub body_bytes: usize,
    /// The whole request must arrive within this time
    pub read_time: Duration,
    pub write_time: Duration,
    pub workers: usize,
    /// Connections waiting for a free worker; any more are closed at once
    pub queue: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// Path and query, as sent
    pub target: String,
    pub user_agent: String,
    /// At most `Limits::body_bytes`
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: Option<&'static str>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn empty(status: u16) -> Response {
        Response { status, content_type: None, body: Vec::new() }
    }
}

pub type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync>;

/// Why a request could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fail {
    BadRequest,
    HeadersTooLarge,
    TimedOut,
    /// The client went away; nothing to answer
    Closed,
}

impl Fail {
    fn status(self) -> u16 {
        match self {
            Fail::BadRequest | Fail::Closed => 400,
            Fail::HeadersTooLarge => 431,
            Fail::TimedOut => 408,
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        408 => "Request Timeout",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// Reads one chunk. A timeout and a closed connection are told apart; nothing else matters here.
fn read_some<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize, Fail> {
    loop {
        match r.read(buf) {
            Ok(0) => return Err(Fail::Closed),
            Ok(n) => return Ok(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Err(Fail::TimedOut),
            Err(_) => return Err(Fail::Closed),
        }
    }
}

/// Where the headers end: (length of the header block, length of the blank-line separator).
fn head_end(buf: &[u8]) -> Option<(usize, usize)> {
    buf.iter().enumerate().find_map(|(i, &b)| {
        if b != b'\n' {
            return None;
        }
        match (buf.get(i + 1), buf.get(i + 2)) {
            (Some(b'\n'), _) => Some((i + 1, 1)),
            (Some(b'\r'), Some(b'\n')) => Some((i + 1, 2)),
            _ => None,
        }
    })
}

/// Decodes as much of a chunked body as `raw` holds, keeping at most `limit` bytes.
/// Returns the data and whether nothing more needs reading (the body ended, was malformed,
/// or reached the limit).
fn decode_chunked(raw: &[u8], limit: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let rest = raw.get(i..).unwrap_or_default();
        let Some(nl) = rest.iter().position(|&b| b == b'\n') else {
            return (out, false);
        };
        let line = String::from_utf8_lossy(&rest[..nl]);
        let size_text = line.split(';').next().unwrap_or_default().trim();
        let Ok(size) = usize::from_str_radix(size_text, 16) else {
            return (out, true);
        };
        i += nl + 1;
        if size == 0 {
            return (out, true);
        }
        let available = raw.len().saturating_sub(i);
        let take = size.min(available);
        out.extend_from_slice(raw.get(i..i + take).unwrap_or_default());
        if out.len() >= limit {
            out.truncate(limit);
            return (out, true);
        }
        if take < size {
            return (out, false);
        }
        i += size;
        if raw.get(i) == Some(&b'\r') {
            i += 1;
        }
        match raw.get(i) {
            Some(b'\n') => i += 1,
            Some(_) => return (out, true),
            None => return (out, false),
        }
    }
}

/// Reads one request. Only the first `limits.body_bytes` of a body are read; a body that
/// stops early is kept as far as it got, since what arrived may be the evidence.
fn read_request<R: Read>(r: &mut R, limits: &Limits) -> Result<Request, Fail> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    let (head_len, sep_len) = loop {
        if let Some(end) = head_end(&buf) {
            break end;
        }
        if buf.len() > limits.header_bytes {
            return Err(Fail::HeadersTooLarge);
        }
        let n = read_some(r, &mut chunk)?;
        buf.extend_from_slice(&chunk[..n]);
    };
    if head_len > limits.header_bytes {
        return Err(Fail::HeadersTooLarge);
    }
    let head = String::from_utf8_lossy(&buf[..head_len]).into_owned();
    let mut body: Vec<u8> = buf[head_len + sep_len..].to_vec();

    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let (Some(method), Some(target), Some(version)) = (first.next(), first.next(), first.next()) else {
        return Err(Fail::BadRequest);
    };
    if !version.starts_with("HTTP/") {
        return Err(Fail::BadRequest);
    }
    let mut request = Request { method: method.to_string(), target: target.to_string(), ..Request::default() };
    let mut length: Option<u64> = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.parse().map_err(|_| Fail::BadRequest)?);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.to_ascii_lowercase().contains("chunked");
        } else if name.eq_ignore_ascii_case("user-agent") {
            request.user_agent = value.to_string();
        }
    }

    let limit = limits.body_bytes;
    if chunked {
        // Chunk headers add a little on top of the data; stop reading well before it grows large
        let raw_limit = limit.saturating_mul(2).saturating_add(4096);
        loop {
            let (data, done) = decode_chunked(&body, limit);
            if done || body.len() >= raw_limit {
                request.body = data;
                break;
            }
            match read_some(r, &mut chunk) {
                Ok(n) => body.extend_from_slice(&chunk[..n]),
                Err(_) => {
                    request.body = data;
                    break;
                }
            }
        }
    } else if let Some(length) = length {
        let want = usize::try_from(length).unwrap_or(usize::MAX).min(limit);
        while body.len() < want {
            match read_some(r, &mut chunk) {
                Ok(n) => body.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        body.truncate(want);
        request.body = body;
    }
    Ok(request)
}

fn write_response<W: Write>(w: &mut W, response: &Response, with_body: bool) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\nConnection: close\r\n", response.status, reason(response.status));
    if response.status != 204 {
        head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    }
    if let Some(kind) = response.content_type {
        head.push_str(&format!("Content-Type: {kind}\r\n"));
    }
    head.push_str("\r\n");
    w.write_all(head.as_bytes())?;
    if with_body && response.status != 204 {
        w.write_all(&response.body)?;
    }
    w.flush()
}

/// A stream whose reads all share one deadline.
struct Deadline<'a> {
    stream: &'a mut TcpStream,
    until: Instant,
}

impl Read for Deadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

fn handle_connection(mut stream: TcpStream, limits: &Limits, handler: &Handler) {
    let _ = stream.set_write_timeout(Some(limits.write_time));
    let until = Instant::now() + limits.read_time;
    let result = read_request(&mut Deadline { stream: &mut stream, until }, limits);
    let (response, with_body) = match result {
        Ok(request) => (handler(&request), request.method != "HEAD"),
        Err(Fail::Closed) => return,
        Err(fail) => (Response::empty(fail.status()), true),
    };
    let _ = write_response(&mut stream, &response, with_body);
    let _ = stream.shutdown(Shutdown::Write);
}

fn worker(next: &Mutex<Receiver<TcpStream>>, limits: &Limits, handler: &Handler) {
    loop {
        // The lock is held only while waiting for the next connection; it guards nothing else.
        let connection = next.lock().unwrap_or_else(|e| e.into_inner()).recv();
        match connection {
            Ok(stream) => handle_connection(stream, limits, handler),
            Err(_) => return, // the accepting thread has stopped
        }
    }
}

fn accept(listener: TcpListener, queue: SyncSender<TcpStream>, stop: &AtomicBool) {
    for connection in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match connection {
            Ok(stream) => match queue.try_send(stream) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {} // every worker busy and the queue full: close it
                Err(TrySendError::Disconnected(_)) => return,
            },
            // For example too many open files: wait a moment instead of spinning
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// A running server. Dropping it stops it and waits for its threads.
pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Server {
    pub fn bind(address: &str, limits: Limits, handler: Handler) -> io::Result<Server> {
        let listener = TcpListener::bind(address)?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let (queue, next) = sync_channel::<TcpStream>(limits.queue);
        let next = Arc::new(Mutex::new(next));
        let mut threads = Vec::new();
        for _ in 0..limits.workers.max(1) {
            let (next, handler) = (next.clone(), handler.clone());
            threads.push(std::thread::spawn(move || worker(&next, &limits, &handler)));
        }
        let accept_stop = stop.clone();
        threads.push(std::thread::spawn(move || accept(listener, queue, &accept_stop)));
        Ok(Server { addr, stop, threads })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Serves until the process stops.
    pub fn wait(mut self) {
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        self.stop.store(true, Ordering::SeqCst);
        // The accepting thread waits for a connection; give it one so it sees the stop flag
        let mut wake = self.addr;
        match wake.ip() {
            IpAddr::V4(ip) if ip.is_unspecified() => wake.set_ip(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            IpAddr::V6(ip) if ip.is_unspecified() => wake.set_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            _ => {}
        }
        let _ = TcpStream::connect_timeout(&wake, Duration::from_secs(1));
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_LIMITS: Limits = Limits {
        header_bytes: 1024,
        body_bytes: 64,
        read_time: Duration::from_millis(300),
        write_time: Duration::from_millis(300),
        workers: 2,
        queue: 16,
    };

    fn parse(raw: &[u8]) -> Result<Request, Fail> {
        read_request(&mut &raw[..], &TEST_LIMITS)
    }

    #[test]
    fn reads_a_get_with_its_user_agent() {
        let r = parse(b"GET /c/ind0000beef?d=x HTTP/1.1\r\nHost: a\r\nuser-agent: naive-agent\r\n\r\n").unwrap();
        assert_eq!((r.method.as_str(), r.target.as_str(), r.user_agent.as_str()), ("GET", "/c/ind0000beef?d=x", "naive-agent"));
        assert!(r.body.is_empty());
        let bare = parse(b"GET /x HTTP/1.0\n\n").unwrap();
        assert_eq!(bare.target, "/x");
    }

    #[test]
    fn bodies_are_cut_at_the_limit() {
        let mut raw = b"POST /c/x HTTP/1.1\r\nContent-Length: 100000\r\n\r\n".to_vec();
        raw.extend_from_slice(&[b'a'; 100_000]);
        assert_eq!(parse(&raw).unwrap().body.len(), 64);
        // A body that stops early is kept as far as it got
        let short = parse(b"POST /c/x HTTP/1.1\r\nContent-Length: 50\r\n\r\nhello").unwrap();
        assert_eq!(short.body, b"hello");
        let exact = parse(b"POST /c/x HTTP/1.1\r\nContent-Length: 3\r\n\r\nabcdef").unwrap();
        assert_eq!(exact.body, b"abc");
    }

    #[test]
    fn chunked_bodies_are_decoded_and_cut() {
        let r = parse(b"POST /c/x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6;x=y\r\n world\r\n0\r\n\r\n").unwrap();
        assert_eq!(r.body, b"hello world");
        let mut raw = b"POST /c/x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for _ in 0..100 {
            raw.extend_from_slice(b"10\r\n0123456789abcdef\r\n");
        }
        assert_eq!(parse(&raw).unwrap().body.len(), 64);
        let huge = parse(b"POST /c/x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffffffffffff\r\nabc").unwrap();
        assert!(huge.body.is_empty());
        let unfinished = parse(b"POST /c/x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n10\r\nabc").unwrap();
        assert_eq!(unfinished.body, b"abc");
    }

    #[test]
    fn bad_and_oversized_requests_are_refused() {
        assert_eq!(parse(b""), Err(Fail::Closed));
        assert_eq!(parse(b"\r\n\r\n"), Err(Fail::BadRequest));
        assert_eq!(parse(b"GET\r\n\r\n"), Err(Fail::BadRequest));
        assert_eq!(parse(b"GET / FTP/1\r\n\r\n"), Err(Fail::BadRequest));
        assert_eq!(parse(b"POST / HTTP/1.1\r\nContent-Length: lots\r\n\r\n"), Err(Fail::BadRequest));
        let mut big = b"GET / HTTP/1.1\r\nX: ".to_vec();
        big.extend_from_slice(&[b'a'; 5000]);
        assert_eq!(parse(&big), Err(Fail::HeadersTooLarge));
        big.extend_from_slice(b"\r\n\r\n");
        assert_eq!(parse(&big), Err(Fail::HeadersTooLarge));
    }

    #[test]
    fn crafted_bytes_never_crash() {
        // Deterministic pseudo-random bytes, biased towards the characters the parser looks at
        let alphabet = b"\r\n: ;0123456789abcdefPOSTGET/HTTP\xff\xc3\xa9chunkedContent-LengthTransfer-Encoding";
        let mut seed: u64 = 0x1234_5678;
        for _ in 0..20_000 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let len = (seed >> 33) as usize % 300;
            let mut raw = Vec::with_capacity(len);
            for _ in 0..len {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                raw.push(alphabet[(seed >> 33) as usize % alphabet.len()]);
            }
            let _ = parse(&raw);
            let _ = decode_chunked(&raw, 64);
        }
    }

    #[test]
    fn answers_are_well_formed() {
        let mut out = Vec::new();
        let page = Response { status: 200, content_type: Some("text/html; charset=utf-8"), body: b"<p>hi</p>".to_vec() };
        write_response(&mut out, &page, true).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 9\r\n") && text.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert!(text.ends_with("\r\n\r\n<p>hi</p>"));
        let mut out = Vec::new();
        write_response(&mut out, &Response::empty(204), true).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
    }

    fn get(addr: SocketAddr, path: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn slow_clients_cannot_hold_up_the_agent() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = seen.clone();
        let handler: Handler = Arc::new(move |r: &Request| {
            log.lock().unwrap().push(r.target.clone());
            Response::empty(204)
        });
        let server = Server::bind("127.0.0.1:0", TEST_LIMITS, handler).unwrap();
        // More stalled clients than workers: each sends part of a request, then nothing
        let mut stalled = Vec::new();
        for _ in 0..(TEST_LIMITS.workers * 3) {
            let mut s = TcpStream::connect(server.addr()).unwrap();
            s.write_all(b"POST /c/x HTTP/1.1\r\nContent-Length: 1000\r\n\r\nab").unwrap();
            stalled.push(s);
        }
        let started = Instant::now();
        let answer = get(server.addr(), "/c/agent");
        assert!(answer.starts_with("HTTP/1.1 204"), "{answer}");
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        assert!(seen.lock().unwrap().contains(&"/c/agent".to_string()));
        // A client that never finishes its headers is told it took too long
        let mut slow = TcpStream::connect(server.addr()).unwrap();
        slow.write_all(b"GET /c/never HTTP/1.1\r\n").unwrap();
        slow.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut out = String::new();
        let _ = slow.read_to_string(&mut out);
        assert!(out.starts_with("HTTP/1.1 408"), "{out}");
        drop(stalled);
    }

    #[test]
    fn dropping_the_server_stops_it() {
        let handler: Handler = Arc::new(|_: &Request| Response::empty(204));
        let server = Server::bind("127.0.0.1:0", TEST_LIMITS, handler).unwrap();
        let addr = server.addr();
        assert!(get(addr, "/").starts_with("HTTP/1.1 204"));
        drop(server);
        assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err());
    }
}
