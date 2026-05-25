/// Minimal HTTP/1.1 server — no hyper, no axum, no tower.
///
/// Handles:
///   - HTTP/1.1 persistent connections (keep-alive)
///   - Content-Length framed bodies
///   - One tokio task per connection for full concurrency
///   - Correct Connection: close / keep-alive negotiation
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const MAX_HEADERS: usize = 64;
const READ_BUF: usize = 8192;
const MAX_BODY: usize = 16 * 1024 * 1024; // 16 MiB

// ── Public types ──────────────────────────────────────────────────────────────

/// Parsed view of an incoming HTTP request.
/// Strings that come from the per-loop `String` locals have `'static` lifetimes
/// after being dereferenced — we avoid borrowing `buf` across await points by
/// copying the small scalar values we need (method, path, auth).
pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    #[allow(dead_code)] // used by server loop, not by handlers directly
    pub keep_alive: bool,
    /// Raw value of the Authorization header, if present (e.g. "Bearer sk-...").
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
        Self::json(
            404,
            br#"{"error":{"message":"not found","type":"api_error","code":404}}"#.to_vec(),
        )
    }

    pub fn method_not_allowed() -> Self {
        Self::json(
            405,
            br#"{"error":{"message":"method not allowed","type":"api_error","code":405}}"#.to_vec(),
        )
    }
}

// ── Routing trait ─────────────────────────────────────────────────────────────

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
        // 1. Read until we have the full header section (\r\n\r\n)
        let header_end = loop {
            if let Some(pos) = find_header_end(&buf[buf_start..]) {
                break buf_start + pos;
            }
            if buf.len() - buf_start >= READ_BUF * 4 {
                anyhow::bail!("request headers too large");
            }
            let mut tmp = [0u8; READ_BUF];
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Ok(()); // clean EOF / client closed
            }
            buf.extend_from_slice(&tmp[..n]);
        };

        // 2. Parse with httparse — borrows from buf
        let header_section = &buf[buf_start..header_end + 4];
        let mut raw_headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut parsed = httparse::Request::new(&mut raw_headers);
        parsed.parse(header_section)?;

        // Copy the small values we need into owned Strings so they outlive `buf`
        let method = parsed.method.unwrap_or("").to_ascii_uppercase();
        let path   = parsed.path.unwrap_or("/").to_owned();

        let http11 = parsed.version.unwrap_or(0) == 1;

        let conn_val: String = parsed.headers.iter()
            .find(|h| h.name.eq_ignore_ascii_case("connection"))
            .and_then(|h| std::str::from_utf8(h.value).ok())
            .unwrap_or("")
            .to_ascii_lowercase();

        let keep_alive = if http11 { conn_val != "close" } else { conn_val == "keep-alive" };

        let content_length: usize = parsed.headers.iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-length"))
            .and_then(|h| std::str::from_utf8(h.value).ok())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);

        let auth: Option<String> = parsed.headers.iter()
            .find(|h| h.name.eq_ignore_ascii_case("authorization"))
            .and_then(|h| std::str::from_utf8(h.value).ok())
            .map(str::to_owned);

        drop(parsed); // release borrow of raw_headers / buf

        if content_length > MAX_BODY {
            anyhow::bail!("body too large ({content_length} bytes)");
        }

        // 3. Read body
        let body_offset = header_end - buf_start + 4; // offset within buf[buf_start..]
        let body_end = buf_start + body_offset + content_length;

        if buf.len() < body_end {
            let need = body_end - buf.len();
            let old = buf.len();
            buf.resize(old + need, 0);
            stream.read_exact(&mut buf[old..]).await?;
        }

        let body = &buf[buf_start + body_offset..body_end];

        // 4. Dispatch
        let request = Request {
            method: &method,
            path: &path,
            body,
            keep_alive,
            auth_header: auth.as_deref(),
        };
        let response = Arc::clone(&state).route(&request);

        // 5. Write response
        let reason   = status_reason(response.status);
        let conn_out = if keep_alive { "keep-alive" } else { "close" };
        let head = format!(
            "HTTP/1.1 {s} {reason}\r\nContent-Type: {ct}\r\nContent-Length: {cl}\r\nConnection: {conn}\r\n\r\n",
            s      = response.status,
            ct     = response.content_type,
            cl     = response.body.len(),
            conn   = conn_out,
        );
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(&response.body).await?;

        if !keep_alive {
            return Ok(());
        }

        // Advance past consumed request and compact if needed
        buf_start = body_end;
        if buf_start > READ_BUF * 2 {
            buf.drain(..buf_start);
            buf_start = 0;
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

#[inline]
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
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
