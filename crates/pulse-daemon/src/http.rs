//! HTTP/1.1 mínimo sobre `TcpStream` (uma requisição por conexão).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const MAX_HEADER: usize = 16 * 1024;
pub const MAX_BODY: usize = 2 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    /// Query string original (sem o `?`), para repasse.
    pub raw_query: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, k: &str) -> Option<&str> {
        self.headers
            .get(&k.to_ascii_lowercase())
            .map(|s| s.as_str())
    }
    pub fn q(&self, k: &str) -> Option<&str> {
        self.query.get(k).map(|s| s.as_str())
    }
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = b
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok());
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

#[derive(Debug)]
pub enum ParseError {
    Closed,
    TooLarge,
    Bad(&'static str),
}

/// Analisa cabeçalhos já lidos (até `\r\n\r\n`).
pub fn parse_head(head: &str) -> Result<Request, ParseError> {
    let mut lines = head.split("\r\n");
    let first = lines
        .next()
        .ok_or(ParseError::Bad("linha inicial ausente"))?;
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().ok_or(ParseError::Bad("alvo ausente"))?;
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") || method.is_empty() {
        return Err(ParseError::Bad("versão HTTP não suportada"));
    }
    if !target.starts_with('/') {
        return Err(ParseError::Bad("alvo inválido"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut headers = HashMap::new();
    for l in lines {
        if l.is_empty() {
            continue;
        }
        let (k, v) = l
            .split_once(':')
            .ok_or(ParseError::Bad("cabeçalho inválido"))?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    Ok(Request {
        method,
        path: percent_decode(path),
        raw_query: query.to_string(),
        query: parse_query(query),
        headers,
        body: Vec::new(),
    })
}

pub fn read_request(s: &mut TcpStream) -> Result<Request, ParseError> {
    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 4096];
    let head_end;
    loop {
        let n = s.read(&mut tmp).map_err(|_| ParseError::Closed)?;
        if n == 0 {
            return Err(ParseError::Closed);
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            head_end = i;
            break;
        }
        if buf.len() > MAX_HEADER {
            return Err(ParseError::TooLarge);
        }
    }
    let head = std::str::from_utf8(&buf[..head_end])
        .map_err(|_| ParseError::Bad("cabeçalho não UTF-8"))?;
    let mut req = parse_head(head)?;
    let len: usize = match req.header("content-length") {
        Some(v) => v
            .parse()
            .map_err(|_| ParseError::Bad("content-length inválido"))?,
        None => 0,
    };
    if req.header("transfer-encoding").is_some() {
        return Err(ParseError::Bad("transfer-encoding não suportado"));
    }
    if len > MAX_BODY {
        return Err(ParseError::TooLarge);
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut tmp).map_err(|_| ParseError::Closed)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(len);
    req.body = body;
    Ok(req)
}

pub struct Response {
    pub status: u16,
    pub ctype: &'static str,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn new(status: u16, ctype: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Response {
            status,
            ctype,
            body: body.into(),
            headers: Vec::new(),
        }
    }
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self::new(status, "application/json; charset=utf-8", body)
    }
    pub fn ok_json(body: impl Into<Vec<u8>>) -> Self {
        Self::json(200, body)
    }
    pub fn error(status: u16, msg: &str) -> Self {
        Self::json(
            status,
            format!("{{\"error\":{}}}", pulse_core::json::quote(msg)),
        )
    }
    pub fn with_header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        421 => "Misdirected Request",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

pub const SECURITY_HEADERS: &str = "X-Content-Type-Options: nosniff\r\n\
X-Frame-Options: DENY\r\n\
Referrer-Policy: no-referrer\r\n\
Cross-Origin-Resource-Policy: same-origin\r\n\
Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'\r\n";

pub fn write_response(s: &mut TcpStream, r: &Response) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n{}",
        r.status,
        reason(r.status),
        r.ctype,
        r.body.len(),
        SECURITY_HEADERS
    );
    for (k, v) in &r.headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())?;
    s.write_all(&r.body)?;
    s.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_parsing() {
        let r = parse_head("GET /api/logs?unit=a%40b.service&q=erro+grave HTTP/1.1\r\nHost: 127.0.0.1:7878\r\nX-Pulse-Token: abc").unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/api/logs");
        assert_eq!(r.q("unit"), Some("a@b.service"));
        assert_eq!(r.q("q"), Some("erro grave"));
        assert_eq!(r.header("X-PULSE-TOKEN"), Some("abc"));
    }

    #[test]
    fn rejects_bad_heads() {
        assert!(parse_head("GET").is_err());
        assert!(parse_head("GET http://evil/ HTTP/1.1").is_err());
        assert!(parse_head("GET / HTTP/2").is_err());
        assert!(parse_head("GET / HTTP/1.1\r\nsemdoispontos").is_err());
    }

    #[test]
    fn decode() {
        assert_eq!(percent_decode("a%20b%2"), "a b%2");
        assert_eq!(percent_decode("%C3%A9"), "é");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
