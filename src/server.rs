use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{io, pin::Pin, task::Poll};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
};

const MAX_HEADERS: usize = 64;
const READ_BUF: usize = 8192;
const MAX_BODY: usize = 2 * 1024 * 1024;
const MAX_HEADER_SIZE: usize = READ_BUF * 4;
const MAX_CONNECTIONS: usize = 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const RATE_LIMIT_WINDOW: Duration = Duration::from_mins(1);
const RATE_LIMIT_MAX_REQUESTS: usize = 10000;

// ── Public types ──────────────────────────────────────────────────────────────

pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    pub auth_header: Option<&'a str>,
    #[allow(dead_code)]
    pub forwarded_for: Option<&'a str>,
}

pub struct Response {
    pub status: u16,
    pub body: Cow<'static, [u8]>,
    pub content_type: &'static str,
}

impl Response {
    pub fn json(status: u16, body: impl Into<Cow<'static, [u8]>>) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: "application/json",
        }
    }
    pub fn not_found() -> Self {
        Self::json(
            404,
            br#"{"error":{"message":"not found","type":"api_error","code":404}}"#
                as &'static [u8],
        )
    }
    pub fn method_not_allowed() -> Self {
        Self::json(
            405,
            br#"{"error":{"message":"method not allowed","type":"api_error","code":405}}"#
                as &'static [u8],
        )
    }
    pub fn too_many_requests() -> Self {
        Self::json(
            429,
            br#"{"error":{"message":"rate limit exceeded","type":"api_error","code":429}}"#
                as &'static [u8],
        )
    }
}

#[async_trait::async_trait]
pub trait Routable: Send + Sync {
    async fn route(&self, req: &Request<'_>) -> Response;
}

// ── Rate limiter ─────────────────────────────────────────────────────────────

struct RateLimiter {
    entries: HashMap<String, (usize, Instant)>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            max_requests,
            window,
        }
    }

    fn check(&mut self, key: &str) -> bool {
        let now = Instant::now();
        let entry = self.entries.entry(key.to_owned()).or_insert((0, now));

        if now.duration_since(entry.1) > self.window {
            *entry = (1, now);
            return true;
        }

        entry.0 += 1;
        entry.0 <= self.max_requests
    }

    fn cleanup(&mut self) {
        let now = Instant::now();
        self.entries.retain(|_, (count, start)| {
            now.duration_since(*start) <= self.window || *count <= self.max_requests
        });
    }
}

// ── Server entry point ────────────────────────────────────────────────────────

pub async fn serve<S>(addr: &str, state: Arc<S>) -> anyhow::Result<()>
where
    S: Send + Sync + 'static,
    Arc<S>: Routable,
{
    let listener = TcpListener::bind(addr).await?;
    log::info!("listening on {addr}");

    let sem = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let rate_limiter = Arc::new(tokio::sync::Mutex::new(RateLimiter::new(
        RATE_LIMIT_MAX_REQUESTS,
        RATE_LIMIT_WINDOW,
    )));

    let cleanup_limiter = Arc::clone(&rate_limiter);
    let _cleanup = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_mins(1)).await;
            cleanup_limiter.lock().await.cleanup();
        }
    });

    loop {
        let (stream, peer) = listener.accept().await?;

        let Ok(permit) = Arc::clone(&sem).try_acquire_owned() else {
            log::warn!("connection limit reached, rejecting {peer}");
            continue;
        };

        log::debug!("accepted connection from {peer}");

        if let Err(e) = stream.set_nodelay(true) {
            log::debug!("failed to set TCP_NODELAY for {peer}: {e}");
        }

        let state = Arc::clone(&state);
        let limiter = Arc::clone(&rate_limiter);
        let peer_ip = peer.ip().to_string();
        drop(tokio::spawn(async move {
            let _permit = permit;
            if let Err(e) = handle_connection(stream, state, &limiter, &peer_ip).await {
                log::debug!("connection error from {peer}: {e}");
            }
        }));
    }
}

// ── Per-connection loop ───────────────────────────────────────────────────────

async fn handle_connection<S>(
    mut stream: TcpStream,
    state: Arc<S>,
    limiter: &tokio::sync::Mutex<RateLimiter>,
    peer_ip: &str,
) -> anyhow::Result<()>
where
    S: Send + Sync + 'static,
    Arc<S>: Routable,
{
    let mut buf: Vec<u8> = Vec::with_capacity(READ_BUF);
    let mut buf_start = 0usize;
    let mut scan_from = 0usize;
    let mut head_buf: Vec<u8> = Vec::with_capacity(256);
    let mut itoa_buf = itoa::Buffer::new();
    let mut read_buf = vec![0u8; READ_BUF];

    loop {
        let header_end = tokio::time::timeout(IDLE_TIMEOUT, async {
            loop {
                if let Some(pos) = find_header_end(&buf[scan_from..]) {
                    return Ok::<usize, io::Error>(scan_from + pos);
                }
                scan_from = buf.len().saturating_sub(3);
                if buf.len() - buf_start >= MAX_HEADER_SIZE {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "request headers too large",
                    ));
                }
                let n = read(&mut stream, &mut read_buf).await?;
                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "connection closed",
                    ));
                }
                buf.extend_from_slice(&read_buf[..n]);
            }
        })
        .await
        .map_err(|_| {
            log::debug!("idle timeout reading headers from {peer_ip}");
            io::Error::new(io::ErrorKind::TimedOut, "idle timeout")
        })??;

        let ParsedHeaders {
            method,
            path,
            auth,
            forwarded_for,
            keep_alive,
            content_length,
            body_offset,
        } = parse_headers(&buf, buf_start, header_end)?;

        let rate_limit_key = forwarded_for.as_deref().unwrap_or(peer_ip);

        if !limiter.lock().await.check(rate_limit_key) {
            let resp = Response::too_many_requests();
            write_response(&mut stream, &mut head_buf, &mut itoa_buf, &resp).await?;
            return Ok(());
        }

        let body_end = buf_start + body_offset + content_length;

        if buf.len() < body_end {
            tokio::time::timeout(IDLE_TIMEOUT, async {
                while buf.len() < body_end {
                    let to_read = std::cmp::min(read_buf.len(), body_end - buf.len());
                    let n = read(&mut stream, &mut read_buf[..to_read]).await?;
                    if n == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "connection closed",
                        ));
                    }
                    buf.extend_from_slice(&read_buf[..n]);
                }
                Ok::<(), io::Error>(())
            })
            .await
            .map_err(|_| {
                log::debug!("idle timeout reading body from {peer_ip}");
                io::Error::new(io::ErrorKind::TimedOut, "idle timeout")
            })??;
        }

        let body = &buf[buf_start + body_offset..body_end];
        let request = Request {
            method: &method,
            path: &path,
            body,
            auth_header: auth.as_deref(),
            forwarded_for: forwarded_for.as_deref(),
        };
        let response = state.route(&request).await;

        write_response(&mut stream, &mut head_buf, &mut itoa_buf, &response).await?;

        if !keep_alive {
            return Ok(());
        }

        buf_start = body_end;
        scan_from = body_end;
        if buf_start > READ_BUF {
            buf.copy_within(buf_start.., 0);
            buf.truncate(buf.len() - buf_start);
            buf_start = 0;
            scan_from = 0;
        }
    }
}

async fn write_response(
    stream: &mut TcpStream,
    head_buf: &mut Vec<u8>,
    itoa_buf: &mut itoa::Buffer,
    response: &Response,
) -> io::Result<()> {
    let conn_out = "close";

    head_buf.clear();
    head_buf.extend_from_slice(b"HTTP/1.1 ");
    head_buf.extend_from_slice(itoa_buf.format(response.status).as_bytes());
    head_buf.push(b' ');
    head_buf.extend_from_slice(status_reason(response.status).as_bytes());
    head_buf.extend_from_slice(b"\r\nContent-Type: ");
    head_buf.extend_from_slice(response.content_type.as_bytes());
    head_buf.extend_from_slice(b"\r\nContent-Length: ");
    head_buf.extend_from_slice(itoa_buf.format(response.body.len()).as_bytes());
    head_buf.extend_from_slice(b"\r\nConnection: ");
    head_buf.extend_from_slice(conn_out.as_bytes());
    head_buf.extend_from_slice(b"\r\nX-Content-Type-Options: nosniff");
    head_buf.extend_from_slice(b"\r\nX-Frame-Options: DENY");
    head_buf.extend_from_slice(b"\r\nX-Content-Security-Policy: default-src 'none'");
    head_buf.extend_from_slice(b"\r\nCache-Control: no-store");
    head_buf.extend_from_slice(b"\r\n\r\n");
    write_all(stream, head_buf).await?;
    write_all(stream, &response.body).await?;
    Ok(())
}

/// Parsed header data — all owned so the borrow on `buf` is released.
struct ParsedHeaders {
    method: String,
    path: String,
    auth: Option<String>,
    forwarded_for: Option<String>,
    keep_alive: bool,
    content_length: usize,
    body_offset: usize,
}

fn parse_headers(buf: &[u8], buf_start: usize, header_end: usize) -> anyhow::Result<ParsedHeaders> {
    let header_section = &buf[buf_start..header_end + 4];
    let mut raw_headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut parsed = httparse::Request::new(&mut raw_headers);
    let status = parsed.parse(header_section)?;
    debug_assert!(matches!(status, httparse::Status::Complete(_)));

    let method = parsed.method.unwrap_or("").to_owned();
    let path = parsed.path.unwrap_or("/").to_owned();
    let http11 = parsed.version.unwrap_or(0) == 1;

    let mut conn_val = None;
    let mut content_length_val = None;
    let mut auth = None;
    let mut forwarded_for = None;

    let mut matches = 0;
    for h in parsed.headers {
        let name = h.name.as_bytes();
        let name_len = name.len();
        if name_len == 10
            && (name[0] == b'c' || name[0] == b'C')
            && h.name.eq_ignore_ascii_case("connection")
        {
            if conn_val.is_none() {
                conn_val = Some(h.value);
                matches += 1;
            }
        } else if name_len == 14
            && (name[0] == b'c' || name[0] == b'C')
            && h.name.eq_ignore_ascii_case("content-length")
        {
            if content_length_val.is_none() {
                content_length_val = Some(h.value);
                matches += 1;
            }
        } else if name_len == 13
            && (name[0] == b'a' || name[0] == b'A')
            && h.name.eq_ignore_ascii_case("authorization")
        {
            if auth.is_none() {
                auth = Some(h.value);
                matches += 1;
            }
        } else if name_len == 15
            && (name[0] == b'x' || name[0] == b'X')
            && h.name.eq_ignore_ascii_case("x-forwarded-for")
            && forwarded_for.is_none()
        {
            forwarded_for = Some(h.value);
            matches += 1;
        }

        if matches == 4 {
            break;
        }
    }

    let conn_val = conn_val.and_then(|v| std::str::from_utf8(v).ok()).unwrap_or("");
    let keep_alive = if http11 {
        !conn_val.eq_ignore_ascii_case("close")
    } else {
        conn_val.eq_ignore_ascii_case("keep-alive")
    };

    let content_length: usize = content_length_val
        .and_then(|v| std::str::from_utf8(v).ok())
        .unwrap_or("")
        .trim()
        .parse()
        .unwrap_or(0);

    let auth = auth.and_then(|v| std::str::from_utf8(v).ok()).map(str::to_owned);

    let forwarded_for = forwarded_for
        .and_then(|v| std::str::from_utf8(v).ok())
        .map(|v| v.split(',').next().unwrap_or(v).trim().to_owned());

    let base_path = path.split('?').next().unwrap_or(path.as_str());
    let max_body_limit = match (method.as_str(), base_path) {
        ("POST", "/v1/embeddings" | "/embeddings") => MAX_BODY,
        _ => 0,
    };

    if content_length > max_body_limit {
        anyhow::bail!(
            "body too large ({content_length} bytes, max allowed: {max_body_limit} bytes)"
        );
    }

    let body_offset = header_end - buf_start + 4;

    Ok(ParsedHeaders {
        method,
        path,
        auth,
        forwarded_for,
        keep_alive,
        content_length,
        body_offset,
    })
}

// ── Manual I/O helpers ────────────────────────────────────────────────────────

async fn read(stream: &mut TcpStream, buf: &mut [u8]) -> io::Result<usize> {
    std::future::poll_fn(|cx| {
        let mut rb = ReadBuf::new(buf);
        match Pin::new(&mut *stream).poll_read(cx, &mut rb) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(rb.filled().len())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}

async fn write_all(stream: &mut TcpStream, mut buf: &[u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let n = std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_write(cx, buf)).await?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "connection closed",
            ));
        }
        buf = &buf[n..];
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

#[inline]
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

const fn status_reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_header_end_returns_position() {
        let buf = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\nbody";
        assert_eq!(find_header_end(buf), Some(31));
    }

    #[test]
    fn find_header_end_no_match() {
        assert_eq!(find_header_end(b"no double crlf here"), None);
    }

    #[test]
    fn find_header_end_empty() {
        assert_eq!(find_header_end(b""), None);
    }

    #[test]
    fn status_reason_known_codes() {
        assert_eq!(status_reason(200), "OK");
        assert_eq!(status_reason(400), "Bad Request");
        assert_eq!(status_reason(401), "Unauthorized");
        assert_eq!(status_reason(404), "Not Found");
        assert_eq!(status_reason(405), "Method Not Allowed");
        assert_eq!(status_reason(413), "Payload Too Large");
        assert_eq!(status_reason(429), "Too Many Requests");
        assert_eq!(status_reason(500), "Internal Server Error");
    }

    #[test]
    fn status_reason_unknown_default() {
        assert_eq!(status_reason(999), "Unknown");
        assert_eq!(status_reason(0), "Unknown");
    }

    #[test]
    fn parse_headers_basic() {
        let raw = b"POST /v1/embeddings HTTP/1.1\r\ncontent-length: 5\r\nauthorization: Bearer test\r\n\r\nhello";
        let ph = parse_headers(
            raw,
            0,
            raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap(),
        );
        let ph = ph.unwrap();
        assert_eq!(ph.method, "POST");
        assert_eq!(ph.path, "/v1/embeddings");
        assert_eq!(ph.auth.as_deref(), Some("Bearer test"));
        assert!(ph.keep_alive);
        assert_eq!(ph.content_length, 5);
        assert_eq!(
            ph.body_offset,
            raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4
        );
    }

    #[test]
    fn find_header_end_skips_lone_cr() {
        let buf = b"GET / HTTP/1.1\r\nX-Foo: bar\rX-Baz: qux\r\n\r\n";
        assert!(find_header_end(buf).is_some());
    }

    #[test]
    fn rate_limiter_allows_within_limit() {
        let mut limiter = RateLimiter::new(3, Duration::from_mins(1));
        assert!(limiter.check("1.2.3.4"));
        assert!(limiter.check("1.2.3.4"));
        assert!(limiter.check("1.2.3.4"));
    }

    #[test]
    fn rate_limiter_blocks_over_limit() {
        let mut limiter = RateLimiter::new(2, Duration::from_mins(1));
        assert!(limiter.check("1.2.3.4"));
        assert!(limiter.check("1.2.3.4"));
        assert!(!limiter.check("1.2.3.4"));
    }

    #[test]
    fn rate_limiter_is_per_ip() {
        let mut limiter = RateLimiter::new(1, Duration::from_mins(1));
        assert!(limiter.check("1.2.3.4"));
        assert!(limiter.check("5.6.7.8"));
        assert!(!limiter.check("1.2.3.4"));
        assert!(!limiter.check("5.6.7.8"));
    }

    #[test]
    fn too_many_requests_response() {
        let resp = Response::too_many_requests();
        assert_eq!(resp.status, 429);
    }

    #[test]
    fn method_not_allowed_response() {
        let resp = Response::method_not_allowed();
        assert_eq!(resp.status, 405);
        assert_eq!(resp.content_type, "application/json");
        assert_eq!(
            resp.body.as_ref(),
            br#"{"error":{"message":"method not allowed","type":"api_error","code":405}}"#
        );
    }

    #[test]
    fn parse_headers_body_limits() {
        let make_req = |method: &str, path: &str, cl: usize| -> Vec<u8> {
            format!("{method} {path} HTTP/1.1\r\ncontent-length: {cl}\r\n\r\n").into_bytes()
        };

        let raw = make_req("POST", "/v1/embeddings", 5);
        let ph = parse_headers(
            &raw,
            0,
            raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap(),
        );
        assert!(ph.is_ok());

        let raw = make_req("POST", "/v1/embeddings", 2 * 1024 * 1024 + 1);
        let ph = parse_headers(
            &raw,
            0,
            raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap(),
        );
        assert!(
            ph.is_err(),
            "should reject POST /v1/embeddings with body > 2MB"
        );

        let raw = make_req("GET", "/health", 1);
        let ph = parse_headers(
            &raw,
            0,
            raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap(),
        );
        assert!(ph.is_err(), "should reject GET /health with body > 0");

        let raw = make_req("GET", "/health", 0);
        let ph = parse_headers(
            &raw,
            0,
            raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap(),
        );
        assert!(ph.is_ok(), "should allow GET /health with 0 body");

        let raw = make_req("GET", "/v1/models", 5);
        let ph = parse_headers(
            &raw,
            0,
            raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap(),
        );
        assert!(ph.is_err(), "should reject GET /v1/models with body > 0");

        let raw = make_req("POST", "/unknown_path", 5);
        let ph = parse_headers(
            &raw,
            0,
            raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap(),
        );
        assert!(ph.is_err(), "should reject unknown paths with body > 0");
    }

    #[tokio::test]
    async fn handle_connection_read_eof() {
        struct DummyRoutable;
        #[async_trait::async_trait]
        impl Routable for Arc<DummyRoutable> {
            async fn route(&self, _req: &Request<'_>) -> Response {
                Response::not_found()
            }
        }

        let state = Arc::new(DummyRoutable);
        let limiter = tokio::sync::Mutex::new(RateLimiter::new(100, Duration::from_mins(1)));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client_stream = TcpStream::connect(addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();

        // Drop client to cause EOF on read
        drop(client_stream);

        let result = handle_connection(server_stream, state, &limiter, "127.0.0.1").await;
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("connection closed"),
            "unexpected error: {err_str}",
        );
    }

    #[test]
    fn response_json_initialization() {
        let resp = Response::json(200, b"{\"ok\":true}" as &'static [u8]);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body.as_ref(), b"{\"ok\":true}");
        assert_eq!(resp.content_type, "application/json");
        assert!(matches!(resp.body, Cow::Borrowed(_)));
    }

    #[test]
    fn response_json_owned_body() {
        let body_vec = vec![1, 2, 3, 4];
        let resp = Response::json(201, body_vec);
        assert_eq!(resp.status, 201);
        assert_eq!(resp.body.as_ref(), &[1, 2, 3, 4]);
        assert_eq!(resp.content_type, "application/json");
        assert!(matches!(resp.body, Cow::Owned(_)));
    }

    #[test]
    fn response_json_empty_body() {
        let resp = Response::json(204, b"" as &'static [u8]);
        assert_eq!(resp.status, 204);
        assert!(resp.body.is_empty());
        assert_eq!(resp.content_type, "application/json");
    }
}
