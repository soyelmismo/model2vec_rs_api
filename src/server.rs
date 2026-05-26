use std::borrow::Cow;
use std::{io, pin::Pin, sync::Arc, task::Poll};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
};

const MAX_HEADERS: usize = 64;
const READ_BUF: usize = 8192;
const MAX_BODY: usize = 16 * 1024 * 1024;
const MAX_HEADER_SIZE: usize = READ_BUF * 4;

// ── Public types ──────────────────────────────────────────────────────────────

pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    pub auth_header: Option<&'a str>,
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
        Self::json(404, br#"{"error":{"message":"not found","type":"api_error","code":404}}"# as &'static [u8])
    }
    pub fn method_not_allowed() -> Self {
        Self::json(405, br#"{"error":{"message":"method not allowed","type":"api_error","code":405}}"# as &'static [u8])
    }
}

#[async_trait::async_trait]
pub trait Routable: Send + Sync {
    async fn route(&self, req: &Request<'_>) -> Response;
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
        drop(tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, state).await {
                log::debug!("connection error from {peer}: {e}");
            }
        }));
    }
}

// ── Per-connection loop ───────────────────────────────────────────────────────

async fn handle_connection<S>(mut stream: TcpStream, state: Arc<S>) -> anyhow::Result<()>
where
    S: Send + Sync + 'static,
    Arc<S>: Routable,
{
    let mut buf: Vec<u8> = Vec::with_capacity(READ_BUF);
    let mut buf_start = 0usize;
    let mut scan_from = 0usize;

    loop {
        let header_end = loop {
            if let Some(pos) = find_header_end(&buf[scan_from..]) {
                break scan_from + pos;
            }
            scan_from = buf.len().saturating_sub(3);
            if buf.len() - buf_start >= MAX_HEADER_SIZE {
                anyhow::bail!("request headers too large");
            }
            let mut tmp = [0u8; READ_BUF];
            let n = read(&mut stream, &mut tmp).await?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&tmp[..n]);
        };

        // Parse headers from buf — extract what we need, then drop all borrows
        // before potentially mutating buf to read the body.
        let ParsedHeaders {
            method,
            path,
            auth,
            keep_alive,
            content_length,
            body_offset,
        } = parse_headers(&buf, buf_start, header_end)?;

        let body_end = buf_start + body_offset + content_length;

        if buf.len() < body_end {
            let need = body_end - buf.len();
            let old = buf.len();
            buf.resize(old + need, 0);
            read_exact(&mut stream, &mut buf[old..]).await?;
        }

        // Now borrow buf again for body + request construction
        let body = &buf[buf_start + body_offset..body_end];
        let request = Request {
            method: &method,
            path: &path,
            body,
            auth_header: auth.as_deref(),
        };
        let response = state.route(&request).await;

        let conn_out = if keep_alive { "keep-alive" } else { "close" };
        let mut head = Vec::with_capacity(128);
        let mut ib = ::itoa::Buffer::new();
        head.extend_from_slice(b"HTTP/1.1 ");
        head.extend_from_slice(ib.format(response.status).as_bytes());
        head.push(b' ');
        head.extend_from_slice(status_reason(response.status).as_bytes());
        head.extend_from_slice(b"\r\nContent-Type: ");
        head.extend_from_slice(response.content_type.as_bytes());
        head.extend_from_slice(b"\r\nContent-Length: ");
        head.extend_from_slice(ib.format(response.body.len()).as_bytes());
        head.extend_from_slice(b"\r\nConnection: ");
        head.extend_from_slice(conn_out.as_bytes());
        head.extend_from_slice(b"\r\n\r\n");
        write_all(&mut stream, &head).await?;
        write_all(&mut stream, &response.body).await?;

        if !keep_alive {
            return Ok(());
        }

        buf_start = body_end;
        scan_from = buf_start;
        if buf_start > READ_BUF * 2 {
            buf.copy_within(buf_start.., 0);
            buf.truncate(buf.len() - buf_start);
            buf_start = 0;
            scan_from = 0;
        }
    }
}

/// Parsed header data — all owned so the borrow on `buf` is released.
struct ParsedHeaders {
    method: String,
    path: String,
    auth: Option<String>,
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

    let conn_val = header_val(parsed.headers, "connection");
    let keep_alive = if http11 {
        !conn_val.eq_ignore_ascii_case("close")
    } else {
        conn_val.eq_ignore_ascii_case("keep-alive")
    };

    let content_length: usize =
        header_val(parsed.headers, "content-length").trim().parse().unwrap_or(0);

    let auth: Option<String> = parsed
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("authorization"))
        .and_then(|h| std::str::from_utf8(h.value).ok())
        .map(str::to_owned);

    if content_length > MAX_BODY {
        anyhow::bail!("body too large ({content_length} bytes)");
    }

    let body_offset = header_end - buf_start + 4;

    Ok(ParsedHeaders {
        method,
        path,
        auth,
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

async fn read_exact(stream: &mut TcpStream, mut buf: &mut [u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let mut rb = ReadBuf::new(buf);
        std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_read(cx, &mut rb)).await?;
        let n = rb.filled().len();
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed",
            ));
        }
        buf = &mut buf[n..];
    }
    Ok(())
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

fn header_val<'a>(headers: &'a [httparse::Header<'a>], name: &str) -> &'a str {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .and_then(|h| std::str::from_utf8(h.value).ok())
        .unwrap_or("")
}

const fn status_reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
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
        assert_eq!(status_reason(500), "Internal Server Error");
    }

    #[test]
    fn status_reason_unknown_default() {
        assert_eq!(status_reason(999), "Unknown");
        assert_eq!(status_reason(0), "Unknown");
    }

    #[test]
    fn header_val_found() {
        let raw = b"GET / HTTP/1.1\r\ncontent-type: application/json\r\n\r\n";
        let mut headers = [httparse::EMPTY_HEADER; 2];
        let mut req = httparse::Request::new(&mut headers);
        let _ = req.parse(raw);
        assert_eq!(header_val(req.headers, "content-type"), "application/json");
    }

    #[test]
    fn header_val_not_found() {
        let raw = b"GET / HTTP/1.1\r\nx-custom: val\r\n\r\n";
        let mut headers = [httparse::EMPTY_HEADER; 2];
        let mut req = httparse::Request::new(&mut headers);
        let _ = req.parse(raw);
        assert_eq!(header_val(req.headers, "nonexistent"), "");
    }

    #[test]
    fn header_val_empty_headers() {
        assert_eq!(header_val(&[], "anything"), "");
    }

    #[test]
    fn parse_headers_basic() {
        let raw = b"POST /v1/embeddings HTTP/1.1\r\ncontent-length: 5\r\nauthorization: Bearer test\r\n\r\nhello";
        let ph = parse_headers(raw, 0, raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap());
        let ph = ph.unwrap();
        assert_eq!(ph.method, "POST");
        assert_eq!(ph.path, "/v1/embeddings");
        assert_eq!(ph.auth.as_deref(), Some("Bearer test"));
        assert!(ph.keep_alive);
        assert_eq!(ph.content_length, 5);
        assert_eq!(ph.body_offset, raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4);
    }
}
