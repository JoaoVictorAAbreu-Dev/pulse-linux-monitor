//! JSON mínimo, só com a biblioteca padrão: um escritor incremental e um
//! analisador que preserva números como texto (para reescrever arquivos
//! sem perder precisão).

use std::fmt::Write as _;

/// Escapa `s` como string JSON (com aspas).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    push_quoted(&mut out, s);
    out
}

pub fn push_quoted(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"), // evita "</script>" em contextos HTML
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Formata um f64 de forma estável; NaN/∞ viram `null`.
pub fn num(v: f64) -> String {
    if !v.is_finite() {
        return "null".into();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// Construtor de objetos JSON: `Obj::new().s("a","x").n("b",1.0).finish()`.
pub struct Obj {
    buf: String,
    first: bool,
}

impl Default for Obj {
    fn default() -> Self {
        Self::new()
    }
}

impl Obj {
    pub fn new() -> Self {
        Obj {
            buf: String::from("{"),
            first: true,
        }
    }
    fn key(&mut self, k: &str) {
        if !self.first {
            self.buf.push(',');
        }
        self.first = false;
        push_quoted(&mut self.buf, k);
        self.buf.push(':');
    }
    pub fn s(mut self, k: &str, v: &str) -> Self {
        self.key(k);
        push_quoted(&mut self.buf, v);
        self
    }
    pub fn n(mut self, k: &str, v: f64) -> Self {
        self.key(k);
        self.buf.push_str(&num(v));
        self
    }
    pub fn u(mut self, k: &str, v: u64) -> Self {
        self.key(k);
        let _ = write!(self.buf, "{}", v);
        self
    }
    pub fn i(mut self, k: &str, v: i64) -> Self {
        self.key(k);
        let _ = write!(self.buf, "{}", v);
        self
    }
    pub fn b(mut self, k: &str, v: bool) -> Self {
        self.key(k);
        self.buf.push_str(if v { "true" } else { "false" });
        self
    }
    pub fn opt_n(self, k: &str, v: Option<f64>) -> Self {
        match v {
            Some(x) => self.n(k, x),
            None => self.raw(k, "null"),
        }
    }
    pub fn opt_s(self, k: &str, v: Option<&str>) -> Self {
        match v {
            Some(x) => self.s(k, x),
            None => self.raw(k, "null"),
        }
    }
    /// Insere JSON já serializado.
    pub fn raw(mut self, k: &str, json: &str) -> Self {
        self.key(k);
        self.buf.push_str(json);
        self
    }
    pub fn finish(mut self) -> String {
        self.buf.push('}');
        self.buf
    }
}

/// Junta valores já serializados num array JSON.
pub fn arr<I: IntoIterator<Item = String>>(items: I) -> String {
    let mut out = String::from("[");
    for (i, it) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&it);
    }
    out.push(']');
    out
}

pub fn arr_n(items: &[f64]) -> String {
    arr(items.iter().map(|v| num(*v)))
}

// ---------------------------------------------------------------- analisador

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// Número preservado como texto original.
    Num(String),
    Str(String),
    Arr(Vec<Value>),
    /// Objeto com ordem de chaves preservada.
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Obj(kv) => kv.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// Acessa um caminho de chaves: `v.path(&["message","usage"])`.
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        let mut cur = self;
        for k in keys {
            cur = cur.get(k)?;
        }
        Some(cur)
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(s) => s.parse().ok(),
            Value::Str(s) => s.parse().ok(),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    /// Insere ou substitui uma chave num objeto.
    pub fn set(&mut self, key: &str, v: Value) {
        if let Value::Obj(kv) = self {
            if let Some(slot) = kv.iter_mut().find(|(k, _)| k == key) {
                slot.1 = v;
            } else {
                kv.push((key.to_string(), v));
            }
        }
    }

    /// Serialização compacta.
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, None, 0);
        s
    }
    /// Serialização indentada com 2 espaços.
    pub fn to_pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, Some(2), 0);
        s
    }
    fn write(&self, out: &mut String, indent: Option<usize>, level: usize) {
        let nl = |out: &mut String, lvl: usize| {
            if let Some(n) = indent {
                out.push('\n');
                for _ in 0..n * lvl {
                    out.push(' ');
                }
            }
        };
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => out.push_str(n),
            Value::Str(s) => push_quoted_plain(out, s),
            Value::Arr(a) => {
                if a.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    nl(out, level + 1);
                    v.write(out, indent, level + 1);
                }
                nl(out, level);
                out.push(']');
            }
            Value::Obj(kv) => {
                if kv.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (k, v)) in kv.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    nl(out, level + 1);
                    push_quoted_plain(out, k);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    v.write(out, indent, level + 1);
                }
                nl(out, level);
                out.push('}');
            }
        }
    }
}

/// Como `push_quoted`, mas sem escapar `<` (para arquivos de configuração).
fn push_quoted_plain(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn parse(src: &str) -> Result<Value, String> {
    let mut p = Parser {
        b: src.as_bytes(),
        i: 0,
        depth: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("conteúdo extra na posição {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    depth: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn expect(&mut self, lit: &str) -> Result<(), String> {
        if self.b[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(format!("esperado '{}' na posição {}", lit, self.i))
        }
    }
    fn value(&mut self) -> Result<Value, String> {
        self.depth += 1;
        if self.depth > 256 {
            return Err("aninhamento profundo demais".into());
        }
        let r = match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::Str),
            Some(b't') => self.expect("true").map(|_| Value::Bool(true)),
            Some(b'f') => self.expect("false").map(|_| Value::Bool(false)),
            Some(b'n') => self.expect("null").map(|_| Value::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(format!("valor inesperado na posição {}", self.i)),
        };
        self.depth -= 1;
        r
    }
    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.i += 1;
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
        if s.parse::<f64>().is_err() {
            return Err(format!("número inválido '{}'", s));
        }
        Ok(Value::Num(s.to_string()))
    }
    fn hex4(&mut self) -> Result<u32, String> {
        if self.i + 4 > self.b.len() {
            return Err("escape \\u truncado".into());
        }
        let s = std::str::from_utf8(&self.b[self.i..self.i + 4]).map_err(|e| e.to_string())?;
        self.i += 4;
        u32::from_str_radix(s, 16).map_err(|e| e.to_string())
    }
    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // aspas
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.peek().ok_or("string não terminada")?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.peek().ok_or("escape truncado")?;
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp)
                                && self.b[self.i..].starts_with(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000
                                    + ((cp - 0xD800) << 10)
                                    + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            let mut tmp = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                        }
                        _ => return Err(format!("escape inválido na posição {}", self.i)),
                    }
                }
                c => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
    fn array(&mut self) -> Result<Value, String> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Arr(items));
                }
                _ => return Err(format!("esperado ',' ou ']' na posição {}", self.i)),
            }
        }
    }
    fn object(&mut self) -> Result<Value, String> {
        self.i += 1;
        let mut kv = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::Obj(kv));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(format!("esperada chave na posição {}", self.i));
            }
            let k = self.string()?;
            self.ws();
            self.expect(":")?;
            self.ws();
            let v = self.value()?;
            kv.push((k, v));
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Obj(kv));
                }
                _ => return Err(format!("esperado ',' ou '}}' na posição {}", self.i)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_escapes() {
        let j = Obj::new()
            .s("a", "x\"y\n</script>")
            .n("b", 1.5)
            .u("c", 3)
            .b("d", true)
            .finish();
        assert_eq!(j, r#"{"a":"x\"y\n\u003c/script>","b":1.5,"c":3,"d":true}"#);
        assert_eq!(num(f64::NAN), "null");
        assert_eq!(num(2.0), "2");
        assert_eq!(num(0.12345), "0.123");
    }

    #[test]
    fn roundtrip() {
        let src = r#"{"a":[1,2.50,{"b":null}],"s":"éé😀","t":true,"big":12345678901234567890}"#;
        let v = parse(src).unwrap();
        assert_eq!(v.path(&["s"]).unwrap().as_str().unwrap(), "éé😀");
        // número preservado literalmente
        assert!(v.to_json().contains("12345678901234567890"));
        assert!(v.to_json().contains("2.50"));
        let again = parse(&v.to_pretty()).unwrap();
        assert_eq!(v, again);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("{").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("{} x").is_err());
        let deep = "[".repeat(1000) + &"]".repeat(1000);
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn set_and_get() {
        let mut v = parse(r#"{"a":1}"#).unwrap();
        v.set("b", Value::Str("x".into()));
        v.set("a", Value::Num("2".into()));
        assert_eq!(v.to_json(), r#"{"a":2,"b":"x"}"#);
    }
}
