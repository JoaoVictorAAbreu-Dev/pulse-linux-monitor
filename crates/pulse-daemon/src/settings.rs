//! Arquivo de configuração `~/.config/pulse/config.toml` (subconjunto de TOML:
//! `chave = valor`, seções `[alerts]` e `[history]`, comentários com `#`).
//!
//! Precedência: padrões < arquivo < flags da linha de comando.
//! A UI só reescreve a seção `[alerts]`, preservando o resto do arquivo.

use pulse_core::alerts::Rules;
use pulse_core::json::Obj;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MAX_RETENTION_DAYS: u64 = 90;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileConfig {
    pub port: Option<u16>,
    pub bind: Option<String>,
    pub interval_ms: Option<u64>,
    pub notify: Option<bool>,
    pub agents: Option<bool>,
    pub retention_days: Option<u64>,
    /// Pares (chave, valor) da seção [alerts], já validados.
    pub alerts: BTreeMap<String, f64>,
    /// [agents] notify_attention: avisar no desktop quando um agente pede atenção.
    pub notify_attention: Option<bool>,
    /// [agents] show_tool_details: resumo das ferramentas (opt-in).
    pub show_tool_details: Option<bool>,
    /// [prices] modelo = "entrada, saída, leitura de cache, escrita de cache" (US$/milhão de tokens).
    pub prices: BTreeMap<String, [f64; 4]>,
    /// [notify] canais externos (ntfy, webhook, Telegram).
    pub channels: crate::notify::Channels,
    pub notify_alerts: Option<bool>,
    pub notify_agents: Option<bool>,
}

pub fn valid_telegram_token(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 100
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || ":_-".contains(c))
}

pub fn valid_chat_id(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 64
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_@".contains(c))
}

pub fn valid_ntfy_token(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 200
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
}

/// Lê "3, 15, 0.3, 3.75" ou [3, 15, 0.3, 3.75].
pub fn parse_price(v: &str) -> Option<[f64; 4]> {
    let v = v.trim().trim_start_matches('[').trim_end_matches(']');
    let n: Vec<f64> = v
        .split(',')
        .map(|x| x.trim().trim_matches('"').parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    if n.len() != 4
        || n.iter()
            .any(|x| !x.is_finite() || *x < 0.0 || *x > 10_000.0)
    {
        return None;
    }
    Some([n[0], n[1], n[2], n[3]])
}

pub fn valid_model_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 120
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:/@+".contains(c))
}

pub fn path() -> PathBuf {
    pulse_core::sys::config_dir().join("config.toml")
}

fn unquote(v: &str) -> &str {
    let v = v.trim();
    v.strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(v)
}

fn strip_comment(l: &str) -> &str {
    // '#' fora de aspas inicia comentário
    let mut in_q = false;
    for (i, c) in l.char_indices() {
        match c {
            '"' => in_q = !in_q,
            '#' if !in_q => return &l[..i],
            _ => {}
        }
    }
    l
}

pub const ALERT_KEYS: [&str; 10] = [
    "cpu_pct",
    "cpu_samples",
    "mem_warn",
    "mem_crit",
    "swap_warn",
    "disk_warn",
    "disk_crit",
    "temp_crit",
    "proc_pct",
    "proc_samples",
];

/// Analisa o arquivo. Erros por linha não impedem o resto de ser lido;
/// são devolvidos como avisos.
pub fn parse(src: &str) -> (FileConfig, Vec<String>) {
    let mut c = FileConfig::default();
    let mut warn = Vec::new();
    let mut section = String::new();
    for (n, raw) in src.lines().enumerate() {
        let l = strip_comment(raw).trim();
        if l.is_empty() {
            continue;
        }
        if let Some(s) = l.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            section = s.trim().to_string();
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            warn.push(format!("linha {}: esperado chave = valor", n + 1));
            continue;
        };
        let (k, v) = (unquote(k), unquote(v));
        let bad =
            |w: &mut Vec<String>| w.push(format!("linha {}: valor inválido para {}", n + 1, k));
        match (section.as_str(), k) {
            ("", "port") => match v.parse::<u16>() {
                Ok(p) if p > 0 => c.port = Some(p),
                _ => bad(&mut warn),
            },
            ("", "bind") => c.bind = Some(v.to_string()),
            ("", "interval_ms") => match v.parse::<u64>() {
                Ok(i) if i >= 250 => c.interval_ms = Some(i),
                _ => bad(&mut warn),
            },
            ("", "notify") | ("", "agents") => match v {
                "true" | "false" => {
                    let b = v == "true";
                    if k == "notify" {
                        c.notify = Some(b)
                    } else {
                        c.agents = Some(b)
                    }
                }
                _ => bad(&mut warn),
            },
            ("history", "retention_days") => match v.parse::<u64>() {
                Ok(d) if (1..=MAX_RETENTION_DAYS).contains(&d) => c.retention_days = Some(d),
                _ => bad(&mut warn),
            },
            ("agents", "notify_attention") | ("agents", "show_tool_details") => match v {
                "true" | "false" => {
                    let b = Some(v == "true");
                    if k == "notify_attention" {
                        c.notify_attention = b
                    } else {
                        c.show_tool_details = b
                    }
                }
                _ => bad(&mut warn),
            },
            ("prices", model) => match (valid_model_key(model), parse_price(v)) {
                (true, Some(p)) => {
                    c.prices.insert(model.to_string(), p);
                }
                _ => warn.push(format!(
                    "linha {}: preço inválido para {} (use \"entrada, saída, leitura de cache, escrita de cache\" em US$ por milhão de tokens)",
                    n + 1,
                    model
                )),
            },
            ("notify", "ntfy_url") | ("notify", "webhook_url") => {
                if crate::notify::valid_url(v) {
                    if k == "ntfy_url" {
                        c.channels.ntfy_url = Some(v.to_string())
                    } else {
                        c.channels.webhook_url = Some(v.to_string())
                    }
                } else {
                    warn.push(format!("linha {}: URL inválida em {} (use http:// ou https://)", n + 1, k))
                }
            }
            ("notify", "ntfy_token") => match valid_ntfy_token(v) {
                true => c.channels.ntfy_token = Some(v.to_string()),
                false => bad(&mut warn),
            },
            ("notify", "telegram_bot_token") => match valid_telegram_token(v) {
                true => c.channels.telegram_bot_token = Some(v.to_string()),
                false => bad(&mut warn),
            },
            ("notify", "telegram_chat_id") => match valid_chat_id(v) {
                true => c.channels.telegram_chat_id = Some(v.to_string()),
                false => bad(&mut warn),
            },
            ("notify", "alerts") | ("notify", "agents") => match v {
                "true" | "false" => {
                    let b = Some(v == "true");
                    if k == "alerts" {
                        c.notify_alerts = b
                    } else {
                        c.notify_agents = b
                    }
                }
                _ => bad(&mut warn),
            },
            ("alerts", key) if ALERT_KEYS.contains(&key) => match v.parse::<f64>() {
                Ok(x) if x.is_finite() => {
                    c.alerts.insert(key.to_string(), x);
                }
                _ => bad(&mut warn),
            },
            (s, key) => warn.push(format!(
                "linha {}: chave desconhecida {}{}",
                n + 1,
                if s.is_empty() {
                    String::new()
                } else {
                    format!("[{}].", s)
                },
                key
            )),
        }
    }
    (c, warn)
}

/// Aplica pares de [alerts] sobre as regras e valida o resultado.
pub fn apply_alerts(base: &Rules, kv: &BTreeMap<String, f64>) -> Result<Rules, String> {
    let mut r = base.clone();
    for (k, v) in kv {
        let v = *v;
        match k.as_str() {
            "cpu_pct" => r.cpu_pct = v,
            "cpu_samples" => r.cpu_samples = v as u32,
            "mem_warn" => r.mem_warn = v,
            "mem_crit" => r.mem_crit = v,
            "swap_warn" => r.swap_warn = v,
            "disk_warn" => r.disk_warn = v,
            "disk_crit" => r.disk_crit = v,
            "temp_crit" => r.temp_crit = v,
            "proc_pct" => r.proc_pct = v,
            "proc_samples" => r.proc_samples = v as u32,
            other => return Err(format!("chave desconhecida: {}", other)),
        }
    }
    validate(&r)?;
    Ok(r)
}

pub fn validate(r: &Rules) -> Result<(), String> {
    let pct = |n: &str, v: f64| {
        if (1.0..=100.0).contains(&v) {
            Ok(())
        } else {
            Err(format!("{} deve estar entre 1 e 100", n))
        }
    };
    pct("cpu_pct", r.cpu_pct)?;
    pct("mem_warn", r.mem_warn)?;
    pct("mem_crit", r.mem_crit)?;
    pct("swap_warn", r.swap_warn)?;
    pct("disk_warn", r.disk_warn)?;
    pct("disk_crit", r.disk_crit)?;
    pct("proc_pct", r.proc_pct)?;
    if r.mem_warn >= r.mem_crit {
        return Err("mem_warn deve ser menor que mem_crit".into());
    }
    if r.disk_warn >= r.disk_crit {
        return Err("disk_warn deve ser menor que disk_crit".into());
    }
    if !(30.0..=120.0).contains(&r.temp_crit) {
        return Err("temp_crit deve estar entre 30 e 120 °C".into());
    }
    for (n, v) in [
        ("cpu_samples", r.cpu_samples),
        ("proc_samples", r.proc_samples),
    ] {
        if !(1..=86_400).contains(&v) {
            return Err(format!("{} deve estar entre 1 e 86400", n));
        }
    }
    Ok(())
}

pub fn rules_pairs(r: &Rules) -> Vec<(&'static str, f64)> {
    vec![
        ("cpu_pct", r.cpu_pct),
        ("cpu_samples", r.cpu_samples as f64),
        ("mem_warn", r.mem_warn),
        ("mem_crit", r.mem_crit),
        ("swap_warn", r.swap_warn),
        ("disk_warn", r.disk_warn),
        ("disk_crit", r.disk_crit),
        ("temp_crit", r.temp_crit),
        ("proc_pct", r.proc_pct),
        ("proc_samples", r.proc_samples as f64),
    ]
}

pub fn rules_json(r: &Rules) -> String {
    let mut o = Obj::new();
    for (k, v) in rules_pairs(r) {
        o = o.n(k, v);
    }
    o.finish()
}

fn fmt_num(v: f64) -> String {
    if v == v.trunc() {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// Reescreve só a seção [alerts] de `src` com as regras dadas, preservando
/// comentários e o resto do arquivo. Cria a seção se não existir.
pub fn rewrite_alerts(src: &str, r: &Rules) -> String {
    let pairs = rules_pairs(r);
    let mut out: Vec<String> = Vec::new();
    let mut in_alerts = false;
    let mut seen_section = false;
    let mut written: Vec<&str> = Vec::new();
    let flush_missing = |out: &mut Vec<String>, written: &Vec<&str>| {
        for (k, v) in &pairs {
            if !written.contains(k) {
                out.push(format!("{} = {}", k, fmt_num(*v)));
            }
        }
    };
    for raw in src.lines() {
        let t = strip_comment(raw).trim();
        if let Some(s) = t.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            if in_alerts {
                flush_missing(&mut out, &written);
            }
            in_alerts = s.trim() == "alerts";
            seen_section |= in_alerts;
            out.push(raw.to_string());
            continue;
        }
        if in_alerts {
            if let Some((k, _)) = t.split_once('=') {
                let k = k.trim();
                if let Some((key, v)) = pairs.iter().find(|(pk, _)| *pk == k) {
                    let comment = raw
                        .find(" #")
                        .map(|i| raw[i..].to_string())
                        .unwrap_or_default();
                    out.push(format!("{} = {}{}", key, fmt_num(*v), comment));
                    written.push(key);
                    continue;
                }
            }
        }
        out.push(raw.to_string());
    }
    if in_alerts {
        flush_missing(&mut out, &written);
    }
    if !seen_section {
        if out.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
            out.push(String::new());
        }
        out.push("[alerts]".into());
        flush_missing(&mut out, &Vec::new());
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Reescreve uma seção `[nome]` com as linhas `chave = valor` dadas,
/// preservando comentários e o resto do arquivo. Com `replace_all`, chaves
/// da seção que não estão em `pairs` são removidas (ex.: preço apagado).
pub fn rewrite_section(
    src: &str,
    section: &str,
    pairs: &[(String, String)],
    replace_all: bool,
) -> String {
    let key_of = |t: &str| t.split_once('=').map(|(k, _)| unquote(k).to_string());
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    let mut seen = false;
    let mut written: Vec<String> = Vec::new();
    let flush = |out: &mut Vec<String>, written: &Vec<String>| {
        for (k, v) in pairs {
            if !written.contains(k) {
                out.push(format!("{} = {}", k, v));
            }
        }
    };
    for raw in src.lines() {
        let t = strip_comment(raw).trim();
        if let Some(s) = t.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            if inside {
                flush(&mut out, &written);
            }
            inside = s.trim() == section;
            seen |= inside;
            out.push(raw.to_string());
            continue;
        }
        if inside {
            if let Some(k) = key_of(t) {
                if let Some((pk, v)) = pairs.iter().find(|(pk, _)| unquote(pk) == k) {
                    out.push(format!("{} = {}", pk, v));
                    written.push(pk.clone());
                    continue;
                }
                if replace_all {
                    continue;
                }
            }
        }
        out.push(raw.to_string());
    }
    if inside {
        flush(&mut out, &written);
    }
    if !seen {
        if out.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
            out.push(String::new());
        }
        out.push(format!("[{}]", section));
        flush(&mut out, &Vec::new());
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

pub fn fmt_price(p: &[f64; 4]) -> String {
    format!(
        "\"{}, {}, {}, {}\"",
        fmt_num(p[0]),
        fmt_num(p[1]),
        fmt_num(p[2]),
        fmt_num(p[3])
    )
}

/// Grava o arquivo de forma atômica (temporário + rename), modo 0600.
pub fn write_atomic(p: &Path, body: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("toml.tmp");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    f.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

pub const EXAMPLE: &str = "# ~/.config/pulse/config.toml — configuração do Pulse
# Flags da linha de comando têm prioridade sobre este arquivo.

# port = 7878
# interval_ms = 1000
# notify = false
# agents = true

[history]
# retention_days = 7          # 1 a 90 dias de médias de 60 s

[agents]
# notify_attention = true     # aviso no desktop quando um agente pede atenção
# show_tool_details = false   # resumo das ferramentas (comando, caminho) — opt-in

[prices]
# US$ por milhão de tokens: \"entrada, saída, leitura de cache, escrita de cache\".
# Confira os preços atuais no site do provedor; o Pulse não traz preços prontos.
# Um modelo casa pelo nome exato ou pelo prefixo mais longo.
# \"claude-opus-5-5\" = \"0, 0, 0, 0\"

[notify]
# Canais externos para alertas (e, opcionalmente, agentes pedindo atenção).
# ntfy_url = \"https://ntfy.sh/um-topico-dificil-de-adivinhar\"
# ntfy_token = \"tk_...\"               # só para servidores ntfy com autenticação
# webhook_url = \"https://exemplo.com/webhook\"   # JSON genérico ou webhook do Discord
# telegram_bot_token = \"123456:ABC...\"
# telegram_chat_id = \"123456789\"
# alerts = true                 # encaminhar alertas do sistema
# agents = false                # encaminhar \"agente pedindo atenção\"

[alerts]
# cpu_pct = 90                # CPU acima disso…
# cpu_samples = 30            # …por N amostras seguidas
# mem_warn = 90
# mem_crit = 96
# swap_warn = 60
# disk_warn = 90
# disk_crit = 97
# temp_crit = 88              # °C
# proc_pct = 95               # processo preso: CPU acima disso…
# proc_samples = 300          # …por N amostras
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_comments_and_warnings() {
        let (c, w) = parse(
            "port = 9000 # comentário\nbind = \"127.0.0.1\"\nnotify = true\ninterval_ms = 100\n\n[history]\nretention_days = 30\n[alerts]\ncpu_pct = 80\nmem_crit = abc\nfoo = 1\nsem igual\n",
        );
        assert_eq!(c.port, Some(9000));
        assert_eq!(c.bind.as_deref(), Some("127.0.0.1"));
        assert_eq!(c.notify, Some(true));
        assert_eq!(c.interval_ms, None, "abaixo do mínimo");
        assert_eq!(c.retention_days, Some(30));
        assert_eq!(c.alerts.get("cpu_pct"), Some(&80.0));
        assert_eq!(w.len(), 4, "{:?}", w);
    }

    #[test]
    fn apply_and_validate() {
        let mut kv = BTreeMap::new();
        kv.insert("cpu_pct".to_string(), 75.0);
        let r = apply_alerts(&Rules::default(), &kv).unwrap();
        assert_eq!(r.cpu_pct, 75.0);
        kv.insert("mem_warn".to_string(), 99.0);
        assert!(apply_alerts(&Rules::default(), &kv).is_err(), "warn ≥ crit");
        kv.insert("mem_warn".to_string(), 150.0);
        assert!(apply_alerts(&Rules::default(), &kv).is_err());
    }

    #[test]
    fn rewrite_preserves_rest() {
        let src = "# topo\nport = 9000\n\n[alerts]\ncpu_pct = 90 # meu limite\n# nota\n\n[history]\nretention_days = 14\n";
        let r = Rules {
            cpu_pct: 70.0,
            ..Rules::default()
        };
        let out = rewrite_alerts(src, &r);
        assert!(out.starts_with("# topo\nport = 9000\n"));
        assert!(out.contains("cpu_pct = 70 # meu limite"));
        assert!(out.contains("# nota"));
        assert!(out.contains("proc_samples = 300"));
        assert!(out.contains("[history]\nretention_days = 14"));
        let (c, w) = parse(&out);
        assert!(w.is_empty(), "{:?}", w);
        assert_eq!(c.alerts["cpu_pct"], 70.0);
        assert_eq!(c.retention_days, Some(14));
        // sem seção: cria no fim
        let out2 = rewrite_alerts("port = 1\n", &Rules::default());
        assert!(out2.contains("\n\n[alerts]\ncpu_pct = 90\n"));
        // exemplo comentado é válido
        assert!(parse(EXAMPLE).1.is_empty());
    }

    #[test]
    fn agents_and_prices() {
        let (c, w) = parse(
            "[agents]\nnotify_attention = false\nshow_tool_details = true\n[prices]\n\"claude-opus-5-5\" = \"5, 25, 0.5, 6.25\"\nollama/qwen3:4b = [0, 0, 0, 0]\nruim = \"1, 2\"\n\"x y\" = \"1,1,1,1\"\n",
        );
        assert_eq!(
            (c.notify_attention, c.show_tool_details),
            (Some(false), Some(true))
        );
        assert_eq!(c.prices["claude-opus-5-5"], [5.0, 25.0, 0.5, 6.25]);
        assert_eq!(c.prices["ollama/qwen3:4b"], [0.0; 4]);
        assert_eq!(w.len(), 2, "{:?}", w);
        assert_eq!(parse_price("-1,0,0,0"), None);
    }

    #[test]
    fn notify_section() {
        let (c, w) = parse(
            "[notify]\nntfy_url = \"https://ntfy.sh/t\"\nwebhook_url = \"ftp://x\"\ntelegram_bot_token = \"123:AB-c_d\"\ntelegram_chat_id = \"-100123\"\nagents = true\nntfy_token = \"tem espaço\"\n",
        );
        assert_eq!(c.channels.ntfy_url.as_deref(), Some("https://ntfy.sh/t"));
        assert_eq!(c.channels.webhook_url, None);
        assert_eq!(c.channels.telegram_chat_id.as_deref(), Some("-100123"));
        assert_eq!(c.notify_agents, Some(true));
        assert_eq!(w.len(), 2, "{:?}", w);
    }

    #[test]
    fn rewrite_generic_section() {
        let src = "port = 1\n\n[prices]\n# meus preços\n\"a\" = \"1, 1, 1, 1\"\n\"b\" = \"2, 2, 2, 2\"\n\n[alerts]\ncpu_pct = 80\n";
        let pairs = vec![
            ("\"b\"".to_string(), fmt_price(&[3.0, 4.0, 0.5, 0.0])),
            ("\"c\"".to_string(), fmt_price(&[1.0; 4])),
        ];
        let out = rewrite_section(src, "prices", &pairs, true);
        let (c, w) = parse(&out);
        assert!(w.is_empty(), "{:?}", w);
        assert!(!c.prices.contains_key("a"), "removido com replace_all");
        assert_eq!(c.prices["b"], [3.0, 4.0, 0.5, 0.0]);
        assert_eq!(c.prices["c"], [1.0; 4]);
        assert!(out.contains("# meus preços"));
        assert_eq!(c.alerts["cpu_pct"], 80.0);
        let out2 = rewrite_section(
            "port = 1\n",
            "agents",
            &[("notify_attention".into(), "false".into())],
            false,
        );
        assert_eq!(parse(&out2).0.notify_attention, Some(false));
    }
}
