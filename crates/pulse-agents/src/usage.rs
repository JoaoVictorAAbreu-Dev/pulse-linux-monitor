//! Índice de uso diário (tokens por dia, agente e modelo) a partir das
//! transcrições dos últimos N dias, com cache por arquivo (mtime + tamanho):
//! só arquivos alterados são relidos. O custo é calculado na interface a
//! partir da tabela de preços do usuário (`[prices]`).

use crate::sessions::{parse_iso_ms, read_capped, u, walk_jsonl};
use pulse_core::json::{self, Obj, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// (dia local desde a época, agente, modelo)
pub type Key = (i64, &'static str, String);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub messages: u64,
}

impl Tokens {
    fn add(&mut self, o: &Tokens) {
        self.input += o.input;
        self.output += o.output;
        self.cache_read += o.cache_read;
        self.cache_write += o.cache_write;
        self.messages += o.messages;
    }
}

/// Deslocamento do fuso local em segundos (via `date +%z`; UTC se falhar).
pub fn local_offset_s() -> i64 {
    let Ok((true, out, _)) = pulse_core::sys::run_cmd("date", &["+%z"], 2_000) else {
        return 0;
    };
    parse_offset(out.trim())
}

pub fn parse_offset(z: &str) -> i64 {
    let (sign, rest) = match z.as_bytes().first() {
        Some(b'-') => (-1, &z[1..]),
        Some(b'+') => (1, &z[1..]),
        _ => return 0,
    };
    if rest.len() != 4 {
        return 0;
    }
    let h: i64 = rest[..2].parse().unwrap_or(0);
    let m: i64 = rest[2..].parse().unwrap_or(0);
    sign * (h * 3600 + m * 60)
}

pub fn day_of(ts_ms: u64, offset_s: i64) -> i64 {
    (ts_ms as i64 / 1000 + offset_s).div_euclid(86_400)
}

/// Data (AAAA-MM-DD) de um dia local desde a época.
pub fn day_label(day: i64) -> String {
    pulse_core::sys::iso_utc((day.max(0) as u64) * 86_400_000)[..10].to_string()
}

fn ts(v: &Value) -> u64 {
    v.get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(parse_iso_ms)
        .unwrap_or(0)
}

/// Uso de uma transcrição do Claude Code (inclui subagentes).
pub fn claude_usage(src: &str, offset_s: i64) -> BTreeMap<Key, Tokens> {
    let mut out: BTreeMap<Key, Tokens> = BTreeMap::new();
    let mut seen = HashSet::new();
    for line in src.lines() {
        if !line.contains("\"usage\"") {
            continue; // atalho: a maioria das linhas não tem uso
        }
        let Ok(v) = json::parse(line) else { continue };
        if v.get("type").and_then(|x| x.as_str()) != Some("assistant") {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        let id = msg.get("id").and_then(|x| x.as_str()).unwrap_or("");
        if !id.is_empty() && !seen.insert(id.to_string()) {
            continue;
        }
        let Some(us) = msg.get("usage") else { continue };
        let t = ts(&v);
        if t == 0 {
            continue;
        }
        let model = msg
            .get("model")
            .and_then(|x| x.as_str())
            .filter(|m| !m.starts_with('<'))
            .unwrap_or("?")
            .to_string();
        let e = out
            .entry((day_of(t, offset_s), "Claude Code", model))
            .or_default();
        e.add(&Tokens {
            input: u(us.get("input_tokens")),
            output: u(us.get("output_tokens")),
            cache_read: u(us.get("cache_read_input_tokens")),
            cache_write: u(us.get("cache_creation_input_tokens")),
            messages: 1,
        });
    }
    out
}

/// Uso de um rollout do Codex: diferenças entre totais cumulativos
/// (`token_count`), atribuídas ao dia do evento e ao modelo vigente.
pub fn codex_usage(src: &str, offset_s: i64) -> BTreeMap<Key, Tokens> {
    let mut out: BTreeMap<Key, Tokens> = BTreeMap::new();
    let mut model = "?".to_string();
    let mut last = (0u64, 0u64, 0u64);
    for line in src.lines() {
        let is_ctx = line.contains("\"turn_context\"");
        if !is_ctx && !line.contains("\"token_count\"") {
            continue;
        }
        let Ok(v) = json::parse(line) else { continue };
        let p = v.get("payload");
        if is_ctx {
            if let Some(m) = p.and_then(|x| x.get("model")).and_then(|x| x.as_str()) {
                model = m.to_string();
            }
            continue;
        }
        let Some(tu) = p.and_then(|x| x.path(&["info", "total_token_usage"])) else {
            continue;
        };
        let cached = u(tu.get("cached_input_tokens"));
        let cur = (
            u(tu.get("input_tokens")).saturating_sub(cached),
            cached,
            u(tu.get("output_tokens")),
        );
        let d = (
            cur.0.saturating_sub(last.0),
            cur.1.saturating_sub(last.1),
            cur.2.saturating_sub(last.2),
        );
        last = cur;
        let t = ts(&v);
        if t == 0 || d == (0, 0, 0) {
            continue;
        }
        let e = out
            .entry((day_of(t, offset_s), "Codex", model.clone()))
            .or_default();
        e.add(&Tokens {
            input: d.0,
            cache_read: d.1,
            output: d.2,
            cache_write: 0,
            messages: 1,
        });
    }
    out
}

pub struct UsageIndex {
    claude_dir: PathBuf,
    codex_dir: PathBuf,
    offset_s: i64,
    cache: HashMap<PathBuf, (SystemTime, u64, BTreeMap<Key, Tokens>)>,
}

impl UsageIndex {
    pub fn new(home: &Path) -> Self {
        UsageIndex {
            claude_dir: home.join(".claude/projects"),
            codex_dir: home.join(".codex/sessions"),
            offset_s: local_offset_s(),
            cache: HashMap::new(),
        }
    }

    /// Reindexa (só arquivos novos ou alterados) e devolve o agregado.
    pub fn scan(&mut self, days: u64) -> BTreeMap<Key, Tokens> {
        let max_age = Duration::from_secs(days * 86_400);
        let now = SystemTime::now();
        let mut files = Vec::new();
        let mut c = Vec::new();
        walk_jsonl(&self.claude_dir, 1, &mut c);
        files.extend(c.into_iter().map(|f| (f, true)));
        let mut x = Vec::new();
        walk_jsonl(&self.codex_dir, 4, &mut x);
        files.extend(x.into_iter().map(|f| (f, false)));
        let mut keep = HashSet::new();
        for ((path, mtime, len), is_claude) in files {
            if now
                .duration_since(mtime)
                .map(|d| d > max_age)
                .unwrap_or(false)
            {
                continue;
            }
            keep.insert(path.clone());
            if let Some((m, l, _)) = self.cache.get(&path) {
                if *m == mtime && *l == len {
                    continue;
                }
            }
            // até 64 MB por arquivo (transcrições maiores: só o trecho final)
            let Some((src, _)) = read_capped(&path, 64 * 1024 * 1024) else {
                continue;
            };
            let rows = if is_claude {
                claude_usage(&src, self.offset_s)
            } else {
                codex_usage(&src, self.offset_s)
            };
            self.cache.insert(path, (mtime, len, rows));
        }
        self.cache.retain(|k, _| keep.contains(k));
        let today = day_of(pulse_core::sys::now_ms(), self.offset_s);
        let first = today - days as i64 + 1;
        let mut agg: BTreeMap<Key, Tokens> = BTreeMap::new();
        for (_, _, rows) in self.cache.values() {
            for (k, t) in rows {
                if k.0 >= first {
                    agg.entry(k.clone()).or_default().add(t);
                }
            }
        }
        agg
    }

    pub fn offset_s(&self) -> i64 {
        self.offset_s
    }
}

pub fn to_json(agg: &BTreeMap<Key, Tokens>, days: u64, offset_s: i64) -> String {
    let today = day_of(pulse_core::sys::now_ms(), offset_s);
    Obj::new()
        .u("days", days)
        .s("today", &day_label(today))
        .raw(
            "rows",
            &json::arr(agg.iter().map(|((d, agent, model), t)| {
                Obj::new()
                    .s("day", &day_label(*d))
                    .s("agent", agent)
                    .s("model", model)
                    .u("input", t.input)
                    .u("output", t.output)
                    .u("cache_read", t.cache_read)
                    .u("cache_write", t.cache_write)
                    .u("messages", t.messages)
                    .finish()
            })),
        )
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_and_days() {
        assert_eq!(parse_offset("-0300"), -10_800);
        assert_eq!(parse_offset("+0530"), 19_800);
        assert_eq!(parse_offset("lixo"), 0);
        // 2026-01-02T01:00Z ainda é dia 1 em UTC−3
        let t = parse_iso_ms("2026-01-02T01:00:00Z").unwrap();
        assert_eq!(day_label(day_of(t, -10_800)), "2026-01-01");
        assert_eq!(day_label(day_of(t, 0)), "2026-01-02");
    }

    #[test]
    fn claude_daily_usage_dedupes_messages() {
        let src = [
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:00Z","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:01Z","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":10,"output_tokens":5}}}"#,
            r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-01-02T10:00:00Z","message":{"id":"m2","model":"claude-haiku","usage":{"input_tokens":3,"output_tokens":1}}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T10:00:00Z","message":{"content":"usage aparece no texto"}}"#,
        ]
        .join("\n");
        let u = claude_usage(&src, 0);
        assert_eq!(u.len(), 2);
        let a = u.values().next().unwrap();
        assert_eq!(
            (a.input, a.output, a.cache_read, a.cache_write, a.messages),
            (10, 5, 100, 20, 1)
        );
        let j = to_json(&u, 30, 0);
        assert!(j.contains("\"day\":\"2026-01-01\"") && j.contains("claude-haiku"));
    }

    #[test]
    fn codex_daily_usage_uses_deltas() {
        let src = [
            r#"{"timestamp":"2026-01-01T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5"}}"#,
            r#"{"timestamp":"2026-01-01T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":7}}}}"#,
            r#"{"timestamp":"2026-01-02T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":150,"cached_input_tokens":50,"output_tokens":10}}}}"#,
            r#"{"timestamp":"2026-01-02T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":150,"cached_input_tokens":50,"output_tokens":10}}}}"#,
        ]
        .join("\n");
        let u = codex_usage(&src, 0);
        let v: Vec<&Tokens> = u.values().collect();
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].input, v[0].cache_read, v[0].output), (60, 40, 7));
        assert_eq!((v[1].input, v[1].cache_read, v[1].output), (40, 10, 3));
    }
}
