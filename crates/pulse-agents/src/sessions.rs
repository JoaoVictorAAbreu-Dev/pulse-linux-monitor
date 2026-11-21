//! Sessões a partir das transcrições locais (JSONL) do Claude Code e do Codex,
//! e do banco SQLite do OpenClaw (via `sqlite3`, opcional).
//!
//! Por padrão só metadados são expostos: projeto, modelo, nomes de
//! ferramentas, tokens, ritmo e linha do tempo. O texto de pedidos e
//! respostas nunca sai daqui. Com o modo detalhado (opt-in), a linha do
//! tempo ganha um resumo curto e mascarado da ferramenta (comando, caminho,
//! padrão de busca), nunca o conteúdo de mensagens.

use crate::detail::tool_summary;
use pulse_core::json::{self, Obj, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 24 * 1024 * 1024;
const TIMELINE: usize = 40;
/// Intervalos maiores que isso entre eventos não contam como tempo ativo.
const IDLE_GAP_MS: u64 = 5 * 60 * 1000;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolStat {
    pub calls: u64,
    pub timed: u64,
    pub total_ms: u64,
    pub max_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Session {
    pub agent: String,
    pub id: String,
    pub file: String,
    pub project: String,
    pub title: String,
    pub model: String,
    pub started: u64,
    pub updated: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tokens_cache_read: u64,
    pub tokens_cache_write: u64,
    pub prompts: u64,
    /// Mensagens (OpenClaw, onde não separamos pedidos de respostas).
    pub messages: u64,
    /// Mensagens geradas por subagentes (sidechains do Claude Code).
    pub subagent_msgs: u64,
    pub tools: BTreeMap<String, ToolStat>,
    /// Tempo com atividade (soma dos intervalos curtos entre eventos).
    pub active_ms: u64,
    /// Tempo em que o agente esperou o usuário (resposta → próximo pedido).
    pub wait_ms: u64,
    /// (ts ms, tipo, rótulo)
    pub timeline: Vec<(u64, String, String)>,
    pub partial: bool,
    pub(crate) last_ts: u64,
    pub(crate) last_kind: String,
}

impl Session {
    pub fn tokens_cache(&self) -> u64 {
        self.tokens_cache_read + self.tokens_cache_write
    }
    pub fn tokens_total(&self) -> u64 {
        self.tokens_in + self.tokens_out + self.tokens_cache()
    }
    pub fn tool_calls(&self) -> u64 {
        self.tools.values().map(|t| t.calls).sum()
    }
    pub fn to_json(&self, with_timeline: bool) -> String {
        let mut tools: Vec<(&String, &ToolStat)> = self.tools.iter().collect();
        tools.sort_by_key(|t| std::cmp::Reverse(t.1.calls));
        let mut o = Obj::new()
            .s("agent", &self.agent)
            .s("id", &self.id)
            .s("project", &self.project)
            .s("title", &self.title)
            .s("model", &self.model)
            .u("started", self.started)
            .u("updated", self.updated)
            .u("tokens_in", self.tokens_in)
            .u("tokens_out", self.tokens_out)
            .u("tokens_cache", self.tokens_cache())
            .u("tokens_cache_read", self.tokens_cache_read)
            .u("tokens_cache_write", self.tokens_cache_write)
            .u("tokens_total", self.tokens_total())
            .u("prompts", self.prompts)
            .u("messages", self.messages)
            .u("subagent_msgs", self.subagent_msgs)
            .u("active_ms", self.active_ms)
            .u("wait_ms", self.wait_ms)
            .b("partial", self.partial)
            .raw(
                "tools",
                &json::arr(tools.into_iter().map(|(k, t)| {
                    Obj::new()
                        .s("name", k)
                        .u("n", t.calls)
                        .opt_n(
                            "avg_ms",
                            if t.timed > 0 {
                                Some(t.total_ms as f64 / t.timed as f64)
                            } else {
                                None
                            },
                        )
                        .opt_n(
                            "max_ms",
                            if t.timed > 0 {
                                Some(t.max_ms as f64)
                            } else {
                                None
                            },
                        )
                        .finish()
                })),
            );
        if with_timeline {
            o =
                o.raw(
                    "timeline",
                    &json::arr(self.timeline.iter().map(|(ts, k, l)| {
                        Obj::new().u("ts", *ts).s("kind", k).s("label", l).finish()
                    })),
                );
        }
        o.finish()
    }

    /// Registra um evento e acumula o ritmo (tempo ativo × esperando o usuário).
    fn event(&mut self, ts: u64, kind: &str, label: &str) {
        if ts == 0 {
            return;
        }
        if self.started == 0 || ts < self.started {
            self.started = ts;
        }
        if self.last_ts > 0 && ts >= self.last_ts {
            let gap = ts - self.last_ts;
            if kind == "prompt" && matches!(self.last_kind.as_str(), "reply" | "done") {
                self.wait_ms += gap;
            } else if gap < IDLE_GAP_MS {
                self.active_ms += gap;
            }
        }
        if ts >= self.last_ts {
            self.last_ts = ts;
            self.last_kind = kind.to_string();
        }
        self.updated = self.updated.max(ts);
        // eventos de resposta seguidos viram um só
        if kind == "reply"
            && self
                .timeline
                .last()
                .map(|e| e.1 == "reply")
                .unwrap_or(false)
        {
            return;
        }
        self.timeline
            .push((ts, kind.to_string(), label.chars().take(100).collect()));
        if self.timeline.len() > TIMELINE * 4 {
            self.timeline.drain(..self.timeline.len() - TIMELINE);
        }
    }

    fn tool_call(&mut self, name: &str) {
        self.tools.entry(name.to_string()).or_default().calls += 1;
    }

    fn tool_done(&mut self, name: &str, ms: u64) {
        let t = self.tools.entry(name.to_string()).or_default();
        t.timed += 1;
        t.total_ms += ms;
        t.max_ms = t.max_ms.max(ms);
    }

    fn finish(&mut self) {
        if self.timeline.len() > TIMELINE {
            self.timeline.drain(..self.timeline.len() - TIMELINE);
        }
    }
}

/// ISO 8601 UTC (`2026-11-20T22:33:00.123Z`) → ms desde a época.
pub fn parse_iso_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (
        n(0..4)?,
        n(5..7)?,
        n(8..10)?,
        n(11..13)?,
        n(14..16)?,
        n(17..19)?,
    );
    let mut ms = 0i64;
    if b.get(19) == Some(&b'.') {
        let frac: String = s[20..].chars().take_while(|c| c.is_ascii_digit()).collect();
        let f3: String = format!("{:0<3}", frac).chars().take(3).collect();
        ms = f3.parse().unwrap_or(0);
    }
    // dias desde a época (Howard Hinnant, days_from_civil)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let t = ((days * 86_400 + h * 3600 + mi * 60 + se) * 1000) + ms;
    if t < 0 {
        None
    } else {
        Some(t as u64)
    }
}

pub(crate) fn ts_of(v: &Value) -> u64 {
    v.get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(parse_iso_ms)
        .unwrap_or(0)
}

pub(crate) fn u(v: Option<&Value>) -> u64 {
    v.and_then(|x| x.as_f64())
        .map(|x| x.max(0.0) as u64)
        .unwrap_or(0)
}

pub fn project_name(cwd: &str) -> String {
    cwd.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(cwd)
        .to_string()
}

fn label(name: &str, input: Option<&Value>, details: bool) -> String {
    match (details, input) {
        (true, Some(i)) => {
            let s = tool_summary(name, i);
            if s.is_empty() {
                name.to_string()
            } else {
                format!("{} · {}", name, s)
            }
        }
        _ => name.to_string(),
    }
}

/// Analisa uma transcrição do Claude Code.
pub fn parse_claude(src: &str, details: bool) -> Session {
    let mut s = Session {
        agent: "Claude Code".into(),
        ..Default::default()
    };
    let mut seen_msg: std::collections::HashSet<String> = std::collections::HashSet::new();
    // id do tool_use → (ferramenta, quando foi chamado)
    let mut pending: HashMap<String, (String, u64)> = HashMap::new();
    for line in src.lines() {
        let Ok(v) = json::parse(line) else { continue };
        let ts = ts_of(&v);
        if s.id.is_empty() {
            if let Some(id) = v.get("sessionId").and_then(|x| x.as_str()) {
                s.id = id.to_string();
            }
        }
        if let Some(cwd) = v.get("cwd").and_then(|x| x.as_str()) {
            s.project = cwd.to_string();
        }
        if let Some(t) = v.get("aiTitle").and_then(|x| x.as_str()) {
            s.title = t.chars().take(120).collect();
        }
        let sidechain = v.get("isSidechain").and_then(|x| x.as_bool()) == Some(true);
        match v.get("type").and_then(|x| x.as_str()) {
            Some("user") => {
                let content = v.path(&["message", "content"]);
                if let Some(Value::Arr(items)) = content {
                    for it in items {
                        if it.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                            if let Some(id) = it.get("tool_use_id").and_then(|x| x.as_str()) {
                                if let Some((name, t0)) = pending.remove(id) {
                                    if ts >= t0 && t0 > 0 {
                                        s.tool_done(&name, ts - t0);
                                    }
                                }
                            }
                        }
                    }
                }
                if sidechain {
                    continue;
                }
                let is_prompt = match content {
                    Some(Value::Str(_)) => true,
                    Some(Value::Arr(items)) => {
                        items
                            .iter()
                            .any(|i| i.get("type").and_then(|t| t.as_str()) == Some("text"))
                            && !items.iter().any(|i| {
                                i.get("type").and_then(|t| t.as_str()) == Some("tool_result")
                            })
                    }
                    _ => false,
                };
                if is_prompt {
                    s.prompts += 1;
                    s.event(ts, "prompt", "pedido do usuário");
                }
            }
            Some("assistant") => {
                let Some(msg) = v.get("message") else {
                    continue;
                };
                if !sidechain {
                    if let Some(m) = msg.get("model").and_then(|x| x.as_str()) {
                        if !m.starts_with('<') {
                            s.model = m.to_string();
                        }
                    }
                }
                // A mesma mensagem da API pode vir dividida em várias linhas:
                // conta o uso só uma vez por id. Subagentes também gastam tokens.
                let mid = msg
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if mid.is_empty() || seen_msg.insert(mid) {
                    if sidechain {
                        s.subagent_msgs += 1;
                    }
                    if let Some(us) = msg.get("usage") {
                        s.tokens_in += u(us.get("input_tokens"));
                        s.tokens_out += u(us.get("output_tokens"));
                        s.tokens_cache_read += u(us.get("cache_read_input_tokens"));
                        s.tokens_cache_write += u(us.get("cache_creation_input_tokens"));
                    }
                }
                if let Some(Value::Arr(items)) = msg.get("content") {
                    for it in items {
                        match it.get("type").and_then(|t| t.as_str()) {
                            Some("tool_use") => {
                                let name = it
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("?")
                                    .to_string();
                                if let Some(id) = it.get("id").and_then(|x| x.as_str()) {
                                    pending.insert(id.to_string(), (name.clone(), ts));
                                }
                                if !sidechain {
                                    s.tool_call(&name);
                                    let l = label(&name, it.get("input"), details);
                                    s.event(ts, "tool", &l);
                                }
                            }
                            Some("text") if !sidechain => s.event(ts, "reply", "resposta"),
                            _ => {}
                        }
                    }
                }
            }
            _ => {
                if ts > 0 {
                    s.updated = s.updated.max(ts);
                }
            }
        }
    }
    s.finish();
    s
}

/// Argumentos de uma chamada de ferramenta do Codex (string JSON ou objeto).
fn codex_args(p: &Value) -> Option<Value> {
    match p.get("arguments") {
        Some(Value::Str(s)) => json::parse(s).ok(),
        Some(v @ Value::Obj(_)) => Some(v.clone()),
        _ => match p.get("input") {
            Some(Value::Str(s)) => Some(Value::Obj(vec![("patch".into(), Value::Str(s.clone()))])),
            Some(v) => Some(v.clone()),
            None => p.get("action").cloned(),
        },
    }
}

/// Analisa uma transcrição ("rollout") do Codex.
pub fn parse_codex(src: &str, details: bool) -> Session {
    let mut s = Session {
        agent: "Codex".into(),
        ..Default::default()
    };
    let mut pending: HashMap<String, (String, u64)> = HashMap::new();
    for line in src.lines() {
        let Ok(v) = json::parse(line) else { continue };
        let ts = ts_of(&v);
        let p = v.get("payload");
        let pt = p.and_then(|x| x.get("type")).and_then(|x| x.as_str());
        match (v.get("type").and_then(|x| x.as_str()), pt) {
            (Some("session_meta"), _) => {
                if let Some(p) = p {
                    if let Some(id) = p.get("id").and_then(|x| x.as_str()) {
                        s.id = id.into();
                    }
                    if let Some(c) = p.get("cwd").and_then(|x| x.as_str()) {
                        s.project = c.into();
                    }
                }
                if ts > 0 && s.started == 0 {
                    s.started = ts;
                }
            }
            (Some("turn_context"), _) => {
                if let Some(m) = p.and_then(|x| x.get("model")).and_then(|x| x.as_str()) {
                    s.model = m.into();
                }
                if let Some(c) = p.and_then(|x| x.get("cwd")).and_then(|x| x.as_str()) {
                    s.project = c.into();
                }
            }
            (
                Some("response_item"),
                Some("function_call" | "custom_tool_call" | "local_shell_call"),
            ) => {
                let p = p.unwrap();
                let name = p
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("shell")
                    .to_string();
                if let Some(id) = p.get("call_id").and_then(|x| x.as_str()) {
                    pending.insert(id.to_string(), (name.clone(), ts));
                }
                s.tool_call(&name);
                let args = if details { codex_args(p) } else { None };
                let l = label(&name, args.as_ref(), details);
                s.event(ts, "tool", &l);
            }
            (
                Some("response_item"),
                Some(
                    "function_call_output" | "custom_tool_call_output" | "local_shell_call_output",
                ),
            ) => {
                if let Some(id) = p.and_then(|x| x.get("call_id")).and_then(|x| x.as_str()) {
                    if let Some((name, t0)) = pending.remove(id) {
                        if ts >= t0 && t0 > 0 {
                            s.tool_done(&name, ts - t0);
                        }
                    }
                }
            }
            (Some("response_item"), Some("message")) => {
                match p.and_then(|x| x.get("role")).and_then(|x| x.as_str()) {
                    Some("user") => {
                        s.prompts += 1;
                        s.event(ts, "prompt", "pedido do usuário");
                    }
                    Some("assistant") => s.event(ts, "reply", "resposta"),
                    _ => {}
                }
            }
            (Some("event_msg"), Some("token_count")) => {
                if let Some(t) = p.and_then(|x| x.path(&["info", "total_token_usage"])) {
                    let cached = u(t.get("cached_input_tokens"));
                    s.tokens_in = u(t.get("input_tokens")).saturating_sub(cached);
                    s.tokens_cache_read = cached;
                    s.tokens_out = u(t.get("output_tokens"));
                }
            }
            (Some("event_msg"), Some("task_complete")) => s.event(ts, "done", "tarefa concluída"),
            _ => {
                if ts > 0 {
                    s.updated = s.updated.max(ts);
                }
            }
        }
    }
    s.finish();
    s
}

pub(crate) fn read_capped(path: &Path, max: u64) -> Option<(String, bool)> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let partial = len > max;
    if partial {
        f.seek(SeekFrom::Start(len - max)).ok()?;
    }
    let mut buf = Vec::new();
    f.take(max).read_to_end(&mut buf).ok()?;
    let mut s = String::from_utf8_lossy(&buf).into_owned();
    if partial {
        if let Some(i) = s.find('\n') {
            s.drain(..=i);
        }
    }
    Some((s, partial))
}

pub(crate) fn walk_jsonl(dir: &Path, depth: u32, out: &mut Vec<(PathBuf, SystemTime, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() && depth > 0 {
            walk_jsonl(&p, depth - 1, out);
        } else if md.is_file() && p.extension().map(|x| x == "jsonl").unwrap_or(false) {
            out.push((p, md.modified().unwrap_or(UNIX_EPOCH), md.len()));
        }
    }
}

/// Varre as transcrições recentes, com cache por (caminho, mtime, tamanho, modo).
pub struct Scanner {
    cache: HashMap<PathBuf, (SystemTime, u64, bool, Session)>,
    pub claude_dir: PathBuf,
    pub codex_dir: PathBuf,
    pub home: PathBuf,
    pub max_age: Duration,
    pub max_sessions: usize,
    /// Modo detalhado (opt-in): resumo das ferramentas na linha do tempo.
    pub details: bool,
    pub openclaw: crate::openclaw::Reader,
}

impl Scanner {
    pub fn new(home: &Path) -> Self {
        Scanner {
            cache: HashMap::new(),
            claude_dir: home.join(".claude/projects"),
            codex_dir: home.join(".codex/sessions"),
            home: home.to_path_buf(),
            max_age: Duration::from_secs(3 * 24 * 3600),
            max_sessions: 16,
            details: false,
            openclaw: crate::openclaw::Reader::new(home),
        }
    }

    pub fn scan(&mut self) -> Vec<Session> {
        let mut files: Vec<(PathBuf, SystemTime, u64, bool)> = Vec::new();
        let mut c = Vec::new();
        walk_jsonl(&self.claude_dir, 1, &mut c);
        files.extend(c.into_iter().map(|(p, m, l)| (p, m, l, true)));
        let mut x = Vec::new();
        walk_jsonl(&self.codex_dir, 4, &mut x);
        files.extend(x.into_iter().map(|(p, m, l)| (p, m, l, false)));
        let now = SystemTime::now();
        files.retain(|f| {
            now.duration_since(f.1)
                .map(|d| d < self.max_age)
                .unwrap_or(true)
        });
        files.sort_by_key(|f| std::cmp::Reverse(f.1));
        files.truncate(self.max_sessions);
        let mut out = Vec::new();
        let mut keep = std::collections::HashSet::new();
        for (path, mtime, len, is_claude) in files {
            keep.insert(path.clone());
            if let Some((m, l, d, s)) = self.cache.get(&path) {
                if *m == mtime && *l == len && *d == self.details {
                    out.push(s.clone());
                    continue;
                }
            }
            let Some((src, partial)) = read_capped(&path, MAX_BYTES) else {
                continue;
            };
            let mut s = if is_claude {
                parse_claude(&src, self.details)
            } else {
                parse_codex(&src, self.details)
            };
            s.partial = partial;
            s.file = path.to_string_lossy().into_owned();
            if s.id.is_empty() {
                s.id = path
                    .file_stem()
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_default();
            }
            let mt = mtime
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            s.updated = s.updated.max(mt);
            self.cache
                .insert(path, (mtime, len, self.details, s.clone()));
            out.push(s);
        }
        self.cache.retain(|k, _| keep.contains(k));
        // Sessões sem atividade real (ex.: só metadados) ficam de fora.
        out.retain(|s| s.prompts > 0 || !s.tools.is_empty() || s.tokens_total() > 0);
        let cutoff = now
            .checked_sub(self.max_age)
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        out.extend(
            self.openclaw
                .sessions()
                .into_iter()
                .filter(|s| s.updated >= cutoff),
        );
        out.sort_by_key(|s| std::cmp::Reverse(s.updated));
        out
    }
}

pub fn list_json(list: &[Session]) -> String {
    json::arr(list.iter().map(|s| {
        let o = s.to_json(true);
        // acrescenta o nome curto do projeto
        let short = project_name(&s.project);
        format!(
            "{},\"project_name\":{}}}",
            &o[..o.len() - 1],
            json::quote(&short)
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parse() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_iso_ms("2023-11-14T22:13:20.5Z"),
            Some(1_700_000_000_500)
        );
        assert_eq!(
            parse_iso_ms("2024-02-29T00:00:00.000Z"),
            Some(1_709_164_800_000)
        );
        assert_eq!(parse_iso_ms("lixo"), None);
    }

    fn claude_src() -> String {
        [
            r#"{"type":"user","sessionId":"s1","cwd":"/home/u/proj","timestamp":"2026-01-01T10:00:00Z","message":{"role":"user","content":"faça algo SEGREDO"}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:06Z","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"npm test --token=abc123"}}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100}}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T10:00:09Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:10Z","message":{"id":"m2","model":"claude-opus-5-5","content":[{"type":"tool_use","id":"t2","name":"Bash"},{"type":"tool_use","id":"t3","name":"Read","input":{"file_path":"/home/u/proj/a.rs"}}],"usage":{"input_tokens":1,"output_tokens":2}}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T10:00:11Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t3","content":"x"}]}}"#,
            r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-01-01T10:00:12Z","message":{"id":"sub1","model":"claude-haiku","content":[{"type":"text","text":"sub"}],"usage":{"input_tokens":7,"output_tokens":3}}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:20Z","message":{"id":"m3","content":[{"type":"text","text":"pronto"}]}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T10:02:20Z","message":{"role":"user","content":"outra coisa"}}"#,
            "linha quebrada",
        ]
        .join("\n")
    }

    #[test]
    fn claude_transcript() {
        let s = parse_claude(&claude_src(), false);
        assert_eq!(s.id, "s1");
        assert_eq!(s.project, "/home/u/proj");
        assert_eq!(
            s.model, "claude-opus-5-5",
            "subagente não troca o modelo da sessão"
        );
        assert_eq!(s.prompts, 2);
        assert_eq!(s.tokens_in, 18, "m1 contado uma vez + subagente");
        assert_eq!(s.tokens_out, 10);
        assert_eq!((s.tokens_cache_read, s.tokens_cache_write), (100, 20));
        assert_eq!(s.subagent_msgs, 1);
        assert_eq!(s.tools["Bash"].calls, 2);
        assert_eq!(s.tools["Read"].calls, 1);
        // latência: t1 6s→9s = 3 s; t3 10s→11s = 1 s; t2 sem resultado
        assert_eq!((s.tools["Bash"].timed, s.tools["Bash"].max_ms), (1, 3000));
        assert_eq!(s.tools["Read"].total_ms, 1000);
        // ritmo: 20 s ativos até a resposta final; 2 min esperando o usuário
        assert_eq!(s.active_ms, 20_000);
        assert_eq!(s.wait_ms, 120_000);
        let j = s.to_json(true);
        assert!(
            !j.contains("SEGREDO") && !j.contains("npm test"),
            "sem detalhes por padrão"
        );
    }

    #[test]
    fn claude_details_are_opt_in_and_redacted() {
        let s = parse_claude(&claude_src(), true);
        let labels: Vec<&str> = s.timeline.iter().map(|t| t.2.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.starts_with("Bash · npm test")),
            "{:?}",
            labels
        );
        assert!(labels.contains(&"Read · /home/u/proj/a.rs"));
        let j = s.to_json(true);
        assert!(!j.contains("abc123"), "token mascarado");
        assert!(!j.contains("SEGREDO"), "texto do pedido nunca aparece");
    }

    #[test]
    fn codex_transcript() {
        let src = [
            r#"{"timestamp":"2026-01-01T10:00:00Z","type":"session_meta","payload":{"id":"c1","cwd":"/w/app"}}"#,
            r#"{"timestamp":"2026-01-01T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5","cwd":"/w/app"}}"#,
            r#"{"timestamp":"2026-01-01T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[]}}"#,
            r#"{"timestamp":"2026-01-01T10:00:03Z","type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"k1","arguments":"{\"command\":[\"bash\",\"-lc\",\"cargo test\"]}"}}"#,
            r#"{"timestamp":"2026-01-01T10:00:07Z","type":"response_item","payload":{"type":"function_call_output","call_id":"k1","output":"ok"}}"#,
            r#"{"timestamp":"2026-01-01T10:00:08Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":7}}}}"#,
            r#"{"timestamp":"2026-01-01T10:00:09Z","type":"event_msg","payload":{"type":"task_complete"}}"#,
        ]
        .join("\n");
        let s = parse_codex(&src, false);
        assert_eq!(s.id, "c1");
        assert_eq!(s.model, "gpt-5");
        assert_eq!(s.prompts, 1);
        assert_eq!(s.tools["shell"].calls, 1);
        assert_eq!(s.tools["shell"].total_ms, 4000);
        assert_eq!(
            (s.tokens_in, s.tokens_cache_read, s.tokens_out),
            (60, 40, 7)
        );
        assert_eq!(s.timeline.last().unwrap().1, "done");
        let d = parse_codex(&src, true);
        assert!(
            d.timeline
                .iter()
                .any(|t| t.2 == "shell · bash -lc cargo test"),
            "{:?}",
            d.timeline
        );
    }

    #[test]
    fn scanner_reads_temp_home() {
        let home = std::env::temp_dir().join(format!("pulse-sess-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let d = home.join(".claude/projects/-tmp-x");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("abc.jsonl"),
            r#"{"type":"user","sessionId":"abc","cwd":"/tmp/x","timestamp":"2026-01-01T10:00:00Z","message":{"content":"oi"}}"#,
        )
        .unwrap();
        let mut sc = Scanner::new(&home);
        let v = sc.scan();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "abc");
        let j = list_json(&v);
        assert!(json::parse(&j).is_ok());
        assert!(j.contains("\"project_name\":\"x\""));
        assert_eq!(sc.scan().len(), 1, "cache");
        let _ = std::fs::remove_dir_all(&home);
    }
}
