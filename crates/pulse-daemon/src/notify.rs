//! Canais externos de notificação: ntfy, webhook (genérico ou Discord) e
//! Telegram. O envio usa o `curl` com toda a configuração (URL, cabeçalhos,
//! corpo) passada pela entrada padrão (`-K -`): segredos nunca aparecem na
//! linha de comando, em /proc nem nos logs.

use pulse_core::json::{self, Obj};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Channels {
    pub ntfy_url: Option<String>,
    pub ntfy_token: Option<String>,
    pub webhook_url: Option<String>,
    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<String>,
    /// Encaminhar alertas do sistema (padrão: sim).
    pub alerts: bool,
    /// Encaminhar "agente pedindo atenção" (padrão: não).
    pub agents: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub title: String,
    pub body: String,
    /// "info", "aviso" ou "critico".
    pub severity: String,
    pub host: String,
}

impl Channels {
    pub fn any(&self) -> bool {
        self.ntfy_url.is_some() || self.webhook_url.is_some() || self.telegram()
    }
    fn telegram(&self) -> bool {
        self.telegram_bot_token.is_some() && self.telegram_chat_id.is_some()
    }
    /// Resumo sem segredos: só o host de cada URL.
    pub fn to_public_json(&self) -> String {
        let host = |u: &Option<String>| u.as_deref().map(url_host);
        Obj::new()
            .opt_s("ntfy", host(&self.ntfy_url).as_deref())
            .b("ntfy_auth", self.ntfy_token.is_some())
            .opt_s("webhook", host(&self.webhook_url).as_deref())
            .opt_s(
                "webhook_kind",
                self.webhook_url
                    .as_deref()
                    .map(|u| if is_discord(u) { "discord" } else { "json" }),
            )
            .b("telegram", self.telegram())
            .b("alerts", self.alerts)
            .b("agents", self.agents)
            .finish()
    }
}

pub fn valid_url(u: &str) -> bool {
    (u.starts_with("https://") || u.starts_with("http://"))
        && u.len() <= 2048
        && !u.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub fn url_host(u: &str) -> String {
    let rest = u.split_once("://").map(|x| x.1).unwrap_or(u);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // nunca devolve credenciais embutidas (usuario:senha@host)
    host.rsplit('@').next().unwrap_or("").to_string()
}

fn is_discord(u: &str) -> bool {
    let h = url_host(u);
    (h == "discord.com" || h == "discordapp.com" || h.ends_with(".discord.com"))
        && u.contains("/api/webhooks/")
}

/// Base64 padrão (RFC 4648), sem dependências.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        o.push(T[(n >> 18) as usize & 63] as char);
        o.push(T[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        o.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    o
}

/// Cabeçalhos HTTP não carregam UTF-8: textos com acento vão em RFC 2047
/// (`=?UTF-8?B?...?=`), formato que o ntfy decodifica.
pub fn header_text(s: &str) -> String {
    if s.is_ascii() {
        s.to_string()
    } else {
        format!("=?UTF-8?B?{}?=", base64(s.as_bytes()))
    }
}

/// Escapa uma string para o arquivo de configuração do curl.
pub fn curl_quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_control() => {}
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Configurações de curl (uma por canal), sem executar nada. Testável.
pub fn build_requests(ch: &Channels, m: &Message) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let common = "silent\nshow-error\nfail\nmax-time = 10\nproto = \"=https,http\"\n";
    if let Some(url) = &ch.ntfy_url {
        let prio = match m.severity.as_str() {
            "critico" => "5",
            "aviso" => "4",
            _ => "3",
        };
        let mut c = format!("{}url = {}\n", common, curl_quote(url));
        c.push_str(&format!(
            "header = {}\n",
            curl_quote(&format!("Title: {}", header_text(&m.title)))
        ));
        c.push_str(&format!(
            "header = {}\n",
            curl_quote(&format!("Priority: {}", prio))
        ));
        c.push_str(&format!("header = {}\n", curl_quote("Tags: computer")));
        if let Some(t) = &ch.ntfy_token {
            c.push_str(&format!(
                "header = {}\n",
                curl_quote(&format!("Authorization: Bearer {}", t))
            ));
        }
        c.push_str(&format!(
            "data-binary = {}\n",
            curl_quote(&format!("{}\n{}", m.body, m.host))
        ));
        out.push(("ntfy", c));
    }
    if let Some(url) = &ch.webhook_url {
        let body = if is_discord(url) {
            Obj::new()
                .s(
                    "content",
                    &format!("**{}**\n{} · {}", m.title, m.body, m.host),
                )
                .finish()
        } else {
            Obj::new()
                .s("source", "pulse")
                .s("title", &m.title)
                .s("message", &m.body)
                .s("severity", &m.severity)
                .s("host", &m.host)
                .u("ts", pulse_core::sys::now_ms())
                .finish()
        };
        let mut c = format!("{}url = {}\n", common, curl_quote(url));
        c.push_str(&format!(
            "header = {}\n",
            curl_quote("Content-Type: application/json")
        ));
        c.push_str(&format!("data-binary = {}\n", curl_quote(&body)));
        out.push((
            if is_discord(url) {
                "discord"
            } else {
                "webhook"
            },
            c,
        ));
    }
    if let (Some(tok), Some(chat)) = (&ch.telegram_bot_token, &ch.telegram_chat_id) {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", tok);
        let mut c = format!("{}url = {}\n", common, curl_quote(&url));
        c.push_str(&format!(
            "data-urlencode = {}\n",
            curl_quote(&format!("chat_id={}", chat))
        ));
        c.push_str(&format!(
            "data-urlencode = {}\n",
            curl_quote(&format!("text={}\n{} · {}", m.title, m.body, m.host))
        ));
        out.push(("telegram", c));
    }
    out
}

/// Executa um curl com a configuração pela entrada padrão.
fn run_curl(config: &str) -> Result<(), String> {
    let mut child = Command::new("curl")
        .args(["-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl indisponível: {}", e))?;
    child
        .stdin
        .take()
        .ok_or("sem stdin")?
        .write_all(config.as_bytes())
        .map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                if st.success() {
                    return Ok(());
                }
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut e, &mut err);
                }
                // a mensagem do curl pode citar a URL: mostra só o essencial
                let first = err.lines().next().unwrap_or("").trim();
                let msg = first.split(':').next_back().unwrap_or(first).trim();
                return Err(if msg.is_empty() {
                    format!("curl saiu com {}", st)
                } else {
                    msg.to_string()
                });
            }
            Ok(None) if start.elapsed() > Duration::from_secs(15) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("tempo esgotado".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Envia para todos os canais e devolve o resultado de cada um.
pub fn send(ch: &Channels, m: &Message) -> Vec<(&'static str, Result<(), String>)> {
    build_requests(ch, m)
        .into_iter()
        .map(|(name, cfg)| (name, run_curl(&cfg)))
        .collect()
}

/// Envio em segundo plano (não bloqueia o coletor nem a API).
pub fn send_async(ch: Channels, m: Message) {
    if !ch.any() {
        return;
    }
    std::thread::spawn(move || {
        for (name, r) in send(&ch, &m) {
            let res = match &r {
                Ok(()) => "ok".to_string(),
                Err(e) => e.clone(),
            };
            crate::auth::audit(&format!("notify.{}", name), &m.title, &res, "local");
        }
    });
}

pub fn results_json(r: &[(&'static str, Result<(), String>)]) -> String {
    json::arr(r.iter().map(|(n, res)| {
        Obj::new()
            .s("channel", n)
            .b("ok", res.is_ok())
            .opt_s("error", res.as_ref().err().map(|s| s.as_str()))
            .finish()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg() -> Message {
        Message {
            title: "Disco \"cheio\"".into(),
            body: "/ 98%".into(),
            severity: "critico".into(),
            host: "pc".into(),
        }
    }

    #[test]
    fn urls_and_hosts() {
        assert!(valid_url("https://ntfy.sh/meu-topico"));
        assert!(valid_url("http://127.0.0.1:9000/x"));
        assert!(!valid_url("file:///etc/passwd"));
        assert!(!valid_url("https://a b"));
        assert_eq!(
            url_host("https://user:senha@ntfy.example.com:8080/t?x"),
            "ntfy.example.com:8080"
        );
        assert!(is_discord("https://discord.com/api/webhooks/1/abc"));
        assert!(!is_discord("https://evil.com/discord.com/api/webhooks/1"));
    }

    #[test]
    fn requests_per_channel() {
        let ch = Channels {
            ntfy_url: Some("https://ntfy.sh/topico".into()),
            ntfy_token: Some("tk_SEGREDO".into()),
            webhook_url: Some("https://discord.com/api/webhooks/1/xyz".into()),
            telegram_bot_token: Some("123:ABC".into()),
            telegram_chat_id: Some("42".into()),
            alerts: true,
            agents: false,
        };
        let r = build_requests(&ch, &msg());
        let names: Vec<&str> = r.iter().map(|x| x.0).collect();
        assert_eq!(names, vec!["ntfy", "discord", "telegram"]);
        let ntfy = &r[0].1;
        assert!(
            ntfy.contains("header = \"Title: Disco \\\"cheio\\\"\""),
            "{}",
            ntfy
        );
        assert!(ntfy.contains("Priority: 5") && ntfy.contains("Bearer tk_SEGREDO"));
        assert!(ntfy.contains("proto = \"=https,http\"") && ntfy.contains("fail\n"));
        assert!(r[1].1.contains("\\\"content\\\""));
        assert!(r[2]
            .1
            .contains("url = \"https://api.telegram.org/bot123:ABC/sendMessage\""));
        assert!(r[2].1.contains("data-urlencode = \"chat_id=42\""));
        // o resumo público não vaza segredos
        let p = ch.to_public_json();
        assert!(!p.contains("SEGREDO") && !p.contains("123:ABC") && !p.contains("xyz"));
        assert!(p.contains("\"webhook_kind\":\"discord\""));
        // webhook genérico em JSON
        let g = Channels {
            webhook_url: Some("https://exemplo.com/hook".into()),
            ..Default::default()
        };
        assert!(build_requests(&g, &msg())[0]
            .1
            .contains("\\\"source\\\":\\\"pulse\\\""));
        assert!(build_requests(&Channels::default(), &msg()).is_empty());
    }

    #[test]
    fn base64_and_rfc2047() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64("ção".as_bytes()), "w6fDo28=");
        assert_eq!(header_text("Disk full"), "Disk full");
        assert_eq!(
            header_text("notificação"),
            "=?UTF-8?B?bm90aWZpY2HDp8Ojbw==?="
        );
    }

    #[test]
    fn quoting_drops_control_chars() {
        assert_eq!(curl_quote("a\"b\\c\nd\u{7}"), "\"a\\\"b\\\\c\\nd\"");
    }
}
