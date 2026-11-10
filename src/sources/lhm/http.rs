//! Minimal blocking HTTP/1.1 GET for LHM's localhost endpoint.
//!
//! Deliberately hand-rolled: the target is localhost JSON over cleartext, so a
//! TLS-capable client crate would add binary size for nothing. Sends
//! `Connection: close` and reads to EOF, which makes the framing unambiguous
//! whether the server replies with `Content-Length` or `chunked`.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Why a GET failed.
#[derive(Debug)]
pub enum HttpError {
    /// Connecting, sending or receiving failed (refused, timed out, reset).
    Io(io::Error),
    /// The server answered with something that is not valid HTTP/1.1.
    Malformed(String),
    /// A non-2xx status code.
    Status(u16),
    /// The body is not UTF-8.
    NotText,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpError::Io(e) => write!(f, "{e}"),
            HttpError::Malformed(what) => write!(f, "malformed HTTP response: {what}"),
            HttpError::Status(code) => write!(f, "HTTP {code}"),
            HttpError::NotText => write!(f, "response body is not UTF-8"),
        }
    }
}

impl std::error::Error for HttpError {}

impl From<io::Error> for HttpError {
    fn from(e: io::Error) -> Self {
        HttpError::Io(e)
    }
}

/// Fetch `path` from `authority` (`host:port`), with `timeout` on every
/// socket operation, and return the body as text.
///
/// # Errors
/// The connection fails or times out, or the response is not a 2xx with a
/// UTF-8 body ([`parse_response`]).
pub fn get(authority: &str, path: &str, timeout: Duration) -> Result<String, HttpError> {
    let mut stream = TcpStream::connect(authority)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&raw)
}

/// The body of a complete HTTP/1.1 response, if its status is 2xx.
///
/// # Errors
/// Malformed head or chunking, a non-2xx status, or a non-UTF-8 body.
pub fn parse_response(raw: &[u8]) -> Result<String, HttpError> {
    let split = find(raw, b"\r\n\r\n").ok_or_else(|| HttpError::Malformed("no header terminator".into()))?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = &raw[split + 4..];

    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or("");
    let code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| HttpError::Malformed(format!("status line {status_line:?}")))?;
    if !(200..300).contains(&code) {
        return Err(HttpError::Status(code));
    }

    let chunked = lines.any(|l| {
        l.split_once(':').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("transfer-encoding") && value.trim().eq_ignore_ascii_case("chunked")
        })
    });
    let body = if chunked { dechunk(body)? } else { body.to_vec() };
    String::from_utf8(body).map_err(|_| HttpError::NotText)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Decode an HTTP/1.1 `chunked` body.
fn dechunk(input: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut out = Vec::new();
    let mut rest = input;
    loop {
        let line_end = find(rest, b"\r\n").ok_or_else(|| HttpError::Malformed("chunked body truncated".into()))?;
        let size_line = String::from_utf8_lossy(&rest[..line_end]);
        // A chunk size may carry extensions after a ';'.
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| HttpError::Malformed(format!("bad chunk size {size_hex:?}")))?;

        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if rest.len() < size + 2 {
            return Err(HttpError::Malformed("chunked body truncated mid-chunk".into()));
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..]; // skip the chunk's trailing CRLF
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn malformed(r: Result<impl fmt::Debug, HttpError>) -> String {
        match r {
            Err(HttpError::Malformed(what)) => what,
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn plain_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(parse_response(raw).unwrap(), "{}");
    }

    #[test]
    fn chunked_body_with_extension_and_terminator() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\n\r\n4;ext=1\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).unwrap(), "{\"a\":1}");
    }

    #[test]
    fn non_success_status_is_an_error() {
        assert!(matches!(parse_response(b"HTTP/1.1 404 Not Found\r\n\r\n"), Err(HttpError::Status(404))));
        assert_eq!(HttpError::Status(404).to_string(), "HTTP 404");
    }

    #[test]
    fn malformed_responses_are_errors() {
        assert_eq!(malformed(parse_response(b"HTTP/1.1 200 OK\r\n")), "no header terminator");
        assert!(malformed(parse_response(b"garbage\r\n\r\n")).starts_with("status line"));
        assert!(matches!(parse_response(b"HTTP/1.1 200 OK\r\n\r\n\xff"), Err(HttpError::NotText)));
    }

    #[test]
    fn broken_chunking_is_an_error() {
        assert_eq!(malformed(dechunk(b"zz\r\n")), "bad chunk size \"zz\"");
        assert_eq!(malformed(dechunk(b"a\r\nshort\r\n")), "chunked body truncated mid-chunk");
        assert_eq!(malformed(dechunk(b"2\r\nab\r\n")), "chunked body truncated");
    }
}
