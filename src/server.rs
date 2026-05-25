/// Minimal HTTP/1.1 server — no hyper, no axum, no tower, no io-util.
///
/// Handles:
///   - HTTP/1.1 persistent connections (keep-alive)
///   - Content-Length framed bodies
///   - One tokio task per connection for full concurrency
///   - Correct Connection: close / keep-alive negotiation
///
/// I/O helpers (read, read_exact, write_all) are implemented manually
/// using the raw tokio::io::AsyncRead / AsyncWrite traits so we don't
/// need the `io-util` feature (and therefore don't pull in `bytes`).
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::Poll,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
};

const MAX_HEADERS: usize = 64;
const READ_BUF: usize = 8192;
const MAX_BODY: usize = 16 * 1024 * 1024; // 16 MiB

// ── Public types ──────────────────────────────────────────────────────────────

pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    pub auth_header: Option<&'a str>,
}

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

impl Response {
    pub fn json(status: u16, body: Vec<u8>) -> Self {
        Self { status, body, content_type: "application/json" }
    }
    pub fn not_found() -> Self {
        Self::json(404, br#"{"error":{"message":"not found","type":"api_error","code":404}}"#.to_vec())
    }
    pub fn method_not_allowed() -> Self {
        Self::json(405, br#"{"error":{"message":"method not allowed","type":"api_error","code":405}}"#.to_vec())
    }
}

pub trait Routable {
    fn route(self, req: &Request<'_>) -> Response;
}

// ── Server entry point ────────────────────────────────────────────────────────

pub async fn serve<S>(addr: &str, state: Arc<S>) -> anyhow::Result<()>
where
    S: Send + Sync + 'static,
    Arc<S>: Routable,
{
    let listener = TcpListener::bind(addr).await?;
    log::info!("listening on {addr}");
    loop {
        let (stream, peer) = listener.accept().await?;
        log::debug!("accepted connection from {peer}");
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, state).await {
                log::debug!("connection error from {peer}: {e}");
            }
        });
    }
}

// ── Per-connection loop ───────────────────────────────────────────────────────

async fn handle_connection<S>(mut stream: TcpStream, state: Arc<S>) -> anyhow::Result<()>
where
    Arc<S>: Routable,
{
    let mut buf: Vec<u8> = Vec::with_capacity(READ_BUF);
    let mut buf_start = 0usize;

    loop {
        // 1. Read until \r\n\r\n
        let header_end = loop {
            if let Some(pos) = find_header_end(&buf[buf_start..]) {
                break buf_start + pos;
            }
            if buf.len() - buf_start >= READ_BUF * 4 {
                anyhow::bail!("request headers too large");
            }
            let mut tmp = [0u8; READ_BUF];
            let n = read(&mut stream, &mut tmp).await?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&tmp[..n]);
        };

        // 2. Parse headers
        let header_section = &buf[buf_start..header_end + 4];
        let mut raw_headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut parsed = httparse::Request::new(&mut raw_headers);
        parsed.parse(header_section)?;

        let method = parsed.method.unwrap_or("").to_ascii_uppercase();
        let path   = parsed.path.unwrap_or("/").to_owned();
        let http11 = parsed.version.unwrap_or(0) == 1;

        let conn_val: String = header_val(&parsed.headers, "connection")
            .to_ascii_lowercase();
        let keep_alive = if http11 { conn_val != "close" } else { conn_val == "keep-alive" };

        let content_length: usize = header_val(&parsed.headers, "content-length")
            .trim().parse().unwrap_or(0);

        let auth: Option<String> = parsed.headers.iter()
            .find(|h| h.name.eq_ignore_ascii_case("authorization"))
            .and_then(|h| std::str::from_utf8(h.value).ok())
            .map(str::to_owned);

        drop(parsed);

        if content_length > MAX_BODY {
            anyhow::bail!("body too large ({content_length} bytes)");
        }

        // 3. Read body
        let body_offset = header_end - buf_start + 4;
        let body_end    = buf_start + body_offset + content_length;

        if buf.len() < body_end {
            let need = body_end - buf.len();
            let old  = buf.len();
            buf.resize(old + need, 0);
            read_exact(&mut stream, &mut buf[old..]).await?;
        }

        let body = &buf[buf_start + body_offset..body_end];

        // 4. Dispatch
        let request = Request {
            method: &method,
            path: &path,
            body,
            auth_header: auth.as_deref(),
        };
        let response = Arc::clone(&state).route(&request);

        // 5. Write response
        let conn_out = if keep_alive { "keep-alive" } else { "close" };
        let head = format!(
            "HTTP/1.1 {s} {reason}\r\nContent-Type: {ct}\r\nContent-Length: {cl}\r\nConnection: {conn}\r\n\r\n",
            s      = response.status,
            reason = status_reason(response.status),
            ct     = response.content_type,
            cl     = response.body.len(),
            conn   = conn_out,
        );
        write_all(&mut stream, head.as_bytes()).await?;
        write_all(&mut stream, &response.body).await?;

        if !keep_alive { return Ok(()); }

        buf_start = body_end;
        if buf_start > READ_BUF * 2 {
            buf.drain(..buf_start);
            buf_start = 0;
        }
    }
}

// ── Manual I/O helpers (replaces io-util / bytes) ────────────────────────────

/// Single non-blocking read. Returns number of bytes read (0 = EOF).
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

/// Read until `buf` is completely filled (mirrors `read_exact`).
async fn read_exact(stream: &mut TcpStream, mut buf: &mut [u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let mut rb = ReadBuf::new(buf);
        std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_read(cx, &mut rb)).await?;
        let n = rb.filled().len();
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed"));
        }
        buf = &mut buf[n..];
    }
    Ok(())
}

/// Write all bytes in `buf` (mirrors `write_all`).
async fn write_all(stream: &mut TcpStream, mut buf: &[u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let n = std::future::poll_fn(|cx| {
            Pin::new(&mut *stream).poll_write(cx, buf)
        })
        .await?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "connection closed"));
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

fn header_val<'a>(headers: &'a [httparse::Header<'a>], name: &str) -> &'a str {
    headers.iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .and_then(|h| std::str::from_utf8(h.value).ok())
        .unwrap_or("")
}

fn status_reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _   => "Unknown",
    }
}
