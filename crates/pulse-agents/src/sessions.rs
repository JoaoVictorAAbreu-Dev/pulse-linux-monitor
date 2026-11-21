//! Sessões a partir das transcrições locais (JSONL) do Claude Code e do Codex.
//! Só metadados são expostos: projeto, modelo, nomes de ferramentas, tokens
//! e linha do tempo. O texto das mensagens nunca sai daqui.

use pulse_core::json::{self, Obj, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 24 * 1024 * 1024;
const TIMELINE: usize = 40;

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
    pub tokens_cache: u64,
    pub prompts: u64,
    pub tools: BTreeMap<String, u64>,
    /// (ts ms, tipo, rótulo)
    pub timeline: Vec<(u64, String, String)>,
    pub partial: bool,
}

impl Session {
    pub fn tokens_total(&self) -> u64 {
        self.tokens_in + self.tokens_out + self.tokens_cache
    }
    pub fn to_json(&self, with_timeline: bool) -> String {
        let mut tools: Vec<(&String, &u64)> = self.tools.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1));
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
            .u("tokens_cache", self.tokens_cache)
            .u("tokens_total", self.tokens_total())
            .u("prompts", self.prompts)
            .b("partial", self.partial)
            .raw(
                "tools",
                &json::arr(tools.into_iter().map(|(k, v)| Obj::new().s("name", k).u("n", *v).finish())),
            );
        if with_timeline {
            o = o.raw(
                "timeline",
                &json::arr(self.timeline.iter().map(|(ts, k, l)| Obj::new().u("ts", *ts).s("kind", k).s("label", l).finish())),
            );
        }
        o.finish()
    }
    fn event(&mut self, ts: u64, kind: &str, label: &str) {
        if ts == 0 {
            return;
        }
        if self.started == 0 || ts < self.started {
            self.started = ts;
        }
        self.updated = self.updated.max(ts);
        self.timeline.push((ts, kind.to_string(), label.chars().take(80).collect()));
        if self.timeline.len() > TIMELINE * 4 {
            self.timeline.drain(..self.timeline.len() - TIMELINE);
        }
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
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
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
    if t < 0 { None } else { Some(t as u64) }
}

fn ts_of(v: &Value) -> u64 {
    v.get("timestamp").and_then(|t| t.as_str()).and_then(parse_iso_ms).unwrap_or(0)
}

fn u(v: Option<&Value>) -> u64 {
    v.and_then(|x| x.as_f64()).map(|x| x.max(0.0) as u64).unwrap_or(0)
}

fn project_name(cwd: &str) -> String {
    cwd.trim_end_matches('/').rsplit('/').next().unwrap_or(cwd).to_string()
}

/// Analisa uma transcrição do Claude Code.
pub fn parse_claude(src: &str) -> Session {
    let mut s = Session { agent: "Claude Code".into(), ..Default::default() };
    let mut seen_msg: std::collections::HashSet<String> = std::collections::HashSet::new();
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
        if v.get("isSidechain").and_then(|x| x.as_bool()) == Some(true) {
            continue;
        }
        match v.get("type").and_then(|x| x.as_str()) {
            Some("user") => {
                let content = v.path(&["message", "content"]);
                let is_prompt = match content {
                    Some(Value::Str(_)) => true,
                    Some(Value::Arr(items)) => {
                        items.iter().any(|i| i.get("type").and_then(|t| t.as_str()) == Some("text"))
                            && !items.iter().any(|i| i.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
                    }
                    _ => false,
                };
                if is_prompt {
                    s.prompts += 1;
                    s.event(ts, "prompt", "pedido do usuário");
                }
            }
            Some("assistant") => {
                let Some(msg) = v.get("message") else { continue };
                if let Some(m) = msg.get("model").and_then(|x| x.as_str()) {
                    if !m.starts_with('<') {
                        s.model = m.to_string();
                    }
                }
                // A mesma mensagem da API pode vir dividida em várias linhas:
                // conta o uso só uma vez por id.
                let mid = msg.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if mid.is_empty() || seen_msg.insert(mid) {
                    if let Some(us) = msg.get("usage") {
                        s.tokens_in += u(us.get("input_tokens"));
                        s.tokens_out += u(us.get("output_tokens"));
                        s.tokens_cache += u(us.get("cache_read_input_tokens")) + u(us.get("cache_creation_input_tokens"));
                    }
                }
                if let Some(Value::Arr(items)) = msg.get("content") {
                    for it in items {
                        match it.get("type").and_then(|t| t.as_str()) {
                            Some("tool_use") => {
                                let name = it.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string();
                                *s.tools.entry(name.clone()).or_default() += 1;
                                s.event(ts, "tool", &name);
                            }
                            Some("text") => s.event(ts, "reply", "resposta"),
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
    dedupe_timeline(&mut s);
    s.finish();
    s
}

/// Colapsa eventos "resposta" consecutivos.
fn dedupe_timeline(s: &mut Session) {
    s.timeline.dedup_by(|a, b| a.1 == "reply" && b.1 == "reply");
}

/// Analisa uma transcrição ("rollout") do Codex.
pub fn parse_codex(src: &str) -> Session {
    let mut s = Session { agent: "Codex".into(), ..Default::default() };
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
            (Some("response_item"), Some("function_call" | "custom_tool_call" | "local_shell_call")) => {
                let name = p.and_then(|x| x.get("name")).and_then(|x| x.as_str()).unwrap_or("shell").to_string();
                *s.tools.entry(name.clone()).or_default() += 1;
                s.event(ts, "tool", &name);
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
                    s.tokens_cache = cached;
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
    dedupe_timeline(&mut s);
    s.finish();
    s
}

fn read_capped(path: &Path) -> Option<(String, bool)> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let partial = len > MAX_BYTES;
    if partial {
        f.seek(SeekFrom::Start(len - MAX_BYTES)).ok()?;
    }
    let mut buf = Vec::new();
    f.take(MAX_BYTES).read_to_end(&mut buf).ok()?;
    let mut s = String::from_utf8_lossy(&buf).into_owned();
    if partial {
        if let Some(i) = s.find('\n') {
            s.drain(..=i);
        }
    }
    Some((s, partial))
}

fn walk_jsonl(dir: &Path, depth: u32, out: &mut Vec<(PathBuf, SystemTime, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
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

/// Varre as transcrições recentes, com cache por (caminho, mtime, tamanho).
pub struct Scanner {
    cache: HashMap<PathBuf, (SystemTime, u64, Session)>,
    pub claude_dir: PathBuf,
    pub codex_dir: PathBuf,
    pub max_age: Duration,
    pub max_sessions: usize,
}

impl Scanner {
    pub fn new(home: &Path) -> Self {
        Scanner {
            cache: HashMap::new(),
            claude_dir: home.join(".claude/projects"),
            codex_dir: home.join(".codex/sessions"),
            max_age: Duration::from_secs(3 * 24 * 3600),
            max_sessions: 16,
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
        files.retain(|f| now.duration_since(f.1).map(|d| d < self.max_age).unwrap_or(true));
        files.sort_by(|a, b| b.1.cmp(&a.1));
        files.truncate(self.max_sessions);
        let mut out = Vec::new();
        let mut keep = std::collections::HashSet::new();
        for (path, mtime, len, is_claude) in files {
            keep.insert(path.clone());
            if let Some((m, l, s)) = self.cache.get(&path) {
                if *m == mtime && *l == len {
                    out.push(s.clone());
                    continue;
                }
            }
            let Some((src, partial)) = read_capped(&path) else { continue };
            let mut s = if is_claude { parse_claude(&src) } else { parse_codex(&src) };
            s.partial = partial;
            s.file = path.to_string_lossy().into_owned();
            if s.id.is_empty() {
                s.id = path.file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            }
            let mt = mtime.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
            s.updated = s.updated.max(mt);
            self.cache.insert(path, (mtime, len, s.clone()));
            out.push(s);
        }
        self.cache.retain(|k, _| keep.contains(k));
        // Sessões sem atividade real (ex.: só metadados) ficam de fora.
        out.retain(|s| s.prompts > 0 || !s.tools.is_empty() || s.tokens_total() > 0);
        out.sort_by(|a, b| b.updated.cmp(&a.updated));
        out
    }
}

pub fn list_json(list: &[Session]) -> String {
    json::arr(list.iter().map(|s| {
        let o = s.to_json(true);
        // acrescenta o nome curto do projeto
        let short = project_name(&s.project);
        format!("{},\"project_name\":{}}}", &o[..o.len() - 1], json::quote(&short))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parse() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso_ms("2023-11-14T22:13:20.5Z"), Some(1_700_000_000_500));
        assert_eq!(parse_iso_ms("2024-02-29T00:00:00.000Z"), Some(1_709_164_800_000));
        assert_eq!(parse_iso_ms("lixo"), None);
    }

    #[test]
    fn claude_transcript() {
        let src = [
            r#"{"type":"user","sessionId":"s1","cwd":"/home/u/proj","timestamp":"2026-01-01T10:00:00Z","message":{"role":"user","content":"faça algo"}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100}}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:06Z","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"tool_use","name":"Bash","input":{}}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100}}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T10:00:07Z","message":{"role":"user","content":[{"type":"tool_result","content":"x"}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:08Z","message":{"id":"m2","model":"claude-opus-5-5","content":[{"type":"tool_use","name":"Bash"},{"type":"tool_use","name":"Read"}],"usage":{"input_tokens":1,"output_tokens":2}}}"#,
            "linha quebrada",
        ]
        .join("\n");
        let s = parse_claude(&src);
        assert_eq!(s.id, "s1");
        assert_eq!(s.project, "/home/u/proj");
        assert_eq!(s.model, "claude-opus-5-5");
        assert_eq!(s.prompts, 1);
        assert_eq!(s.tokens_in, 11, "m1 contado uma vez");
        assert_eq!(s.tokens_out, 7);
        assert_eq!(s.tokens_cache, 100);
        assert_eq!(s.tools["Bash"], 2);
        assert_eq!(s.tools["Read"], 1);
        assert!(s.updated > s.started);
        // sem texto de mensagens no JSON
        assert!(!s.to_json(true).contains("faça algo"));
    }

    #[test]
    fn codex_transcript() {
        let src = [
            r#"{"timestamp":"2026-01-01T10:00:00Z","type":"session_meta","payload":{"id":"c1","cwd":"/w/app"}}"#,
            r#"{"timestamp":"2026-01-01T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5","cwd":"/w/app"}}"#,
            r#"{"timestamp":"2026-01-01T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[]}}"#,
            r#"{"timestamp":"2026-01-01T10:00:03Z","type":"response_item","payload":{"type":"function_call","name":"shell"}}"#,
            r#"{"timestamp":"2026-01-01T10:00:04Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":7}}}}"#,
            r#"{"timestamp":"2026-01-01T10:00:05Z","type":"event_msg","payload":{"type":"task_complete"}}"#,
        ]
        .join("\n");
        let s = parse_codex(&src);
        assert_eq!(s.id, "c1");
        assert_eq!(s.model, "gpt-5");
        assert_eq!(s.prompts, 1);
        assert_eq!(s.tools["shell"], 1);
        assert_eq!((s.tokens_in, s.tokens_cache, s.tokens_out), (60, 40, 7));
        assert_eq!(s.timeline.last().unwrap().1, "done");
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
