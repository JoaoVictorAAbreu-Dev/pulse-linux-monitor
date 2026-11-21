//! Eventos ao vivo enviados pelo script `pulse-hook` e instalação dos hooks
//! no Claude Code (`~/.claude/settings.json`, com backup).

use pulse_core::json::{self, Obj, Value};
use std::collections::{HashMap, VecDeque};
use std::path::Path;

pub const STATES: [(&str, &str); 4] = [
    ("executando", "Executando"),
    ("atencao", "Pedindo atenção"),
    ("aguardando", "Aguardando"),
    ("concluido", "Concluído"),
];

pub fn normalize_state(s: &str) -> Option<&'static str> {
    let s = s.trim().to_lowercase();
    let s = s.as_str();
    Some(match s {
        "executando" | "running" | "working" => "executando",
        "atencao" | "atenção" | "pedindo-atencao" | "attention" | "notification" => "atencao",
        "aguardando" | "waiting" | "idle" | "stop" => "aguardando",
        "concluido" | "concluído" | "done" | "end" => "concluido",
        _ => return None,
    })
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

#[derive(Debug, Clone)]
pub struct HookEvent {
    pub ts: u64,
    pub agent: String,
    pub state: &'static str,
    pub session: String,
    pub cwd: String,
    pub event: String,
    pub tool: String,
    pub message: String,
}

impl HookEvent {
    pub fn to_json(&self) -> String {
        Obj::new()
            .u("ts", self.ts)
            .s("agent", &self.agent)
            .s("state", self.state)
            .s("session", &self.session)
            .s("cwd", &self.cwd)
            .s("event", &self.event)
            .s("tool", &self.tool)
            .s("message", &self.message)
            .finish()
    }
}

#[derive(Default)]
pub struct HookStore {
    pub events: VecDeque<HookEvent>,
    pub sessions: HashMap<String, HookEvent>,
    pub received: u64,
}

impl HookStore {
    /// Registra um evento. `payload` é o JSON que o agente passou ao hook
    /// (só alguns campos são extraídos; o resto é descartado).
    pub fn ingest(&mut self, ts: u64, agent: &str, state: &str, payload: &str) -> Result<HookEvent, String> {
        let state = normalize_state(state).ok_or("estado inválido")?;
        let v = if payload.trim().is_empty() { Value::Obj(vec![]) } else { json::parse(payload).unwrap_or(Value::Obj(vec![])) };
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let agent = {
            let a = clean(agent, 40);
            if a.is_empty() { "claude".to_string() } else { a }
        };
        let session = {
            // Claude Code usa session_id; o notify do Codex usa thread-id.
            let mut s = g("session_id");
            if s.is_empty() {
                s = g("thread-id");
            }
            if s.is_empty() {
                s = g("thread_id");
            }
            if s.is_empty() { format!("{}-sem-sessao", agent) } else { clean(&s, 80) }
        };
        let e = HookEvent {
            ts,
            agent,
            state,
            session: session.clone(),
            cwd: clean(&g("cwd"), 300),
            event: clean(&{
                let e = g("hook_event_name");
                if e.is_empty() { g("type") } else { e }
            }, 40),
            tool: clean(&g("tool_name"), 60),
            message: clean(&g("message"), 200),
        };
        if self.events.len() >= 300 {
            self.events.pop_front();
        }
        self.events.push_back(e.clone());
        self.sessions.insert(session, e.clone());
        if self.sessions.len() > 100 {
            if let Some(oldest) = self.sessions.iter().min_by_key(|(_, e)| e.ts).map(|(k, _)| k.clone()) {
                self.sessions.remove(&oldest);
            }
        }
        self.received += 1;
        Ok(e)
    }

    pub fn to_json(&self) -> String {
        let mut s: Vec<&HookEvent> = self.sessions.values().collect();
        s.sort_by(|a, b| b.ts.cmp(&a.ts));
        Obj::new()
            .u("received", self.received)
            .raw("sessions", &json::arr(s.into_iter().map(|e| e.to_json())))
            .raw("events", &json::arr(self.events.iter().rev().take(100).map(|e| e.to_json())))
            .finish()
    }
}

/// Eventos do Claude Code e o estado que cada um envia.
pub const CLAUDE_EVENTS: [(&str, &str, bool); 5] = [
    ("UserPromptSubmit", "executando", false),
    ("PreToolUse", "executando", true),
    ("Notification", "atencao", false),
    ("Stop", "aguardando", false),
    ("SessionEnd", "concluido", false),
];

fn hook_command(script: &str, state: &str) -> String {
    format!("\"{}\" {} claude", script.replace('"', "\\\""), state)
}

/// Trecho de configuração (para exibir ao usuário).
pub fn claude_snippet(script: &str) -> String {
    let mut hooks = Vec::new();
    for (ev, st, matcher) in CLAUDE_EVENTS {
        let mut entry = vec![];
        if matcher {
            entry.push(("matcher".to_string(), Value::Str("*".into())));
        }
        entry.push((
            "hooks".into(),
            Value::Arr(vec![Value::Obj(vec![
                ("type".into(), Value::Str("command".into())),
                ("command".into(), Value::Str(hook_command(script, st))),
                ("timeout".into(), Value::Num("5".into())),
            ])]),
        ));
        hooks.push((ev.to_string(), Value::Arr(vec![Value::Obj(entry)])));
    }
    Value::Obj(vec![("hooks".into(), Value::Obj(hooks))]).to_pretty()
}

/// Mescla os hooks do Pulse em `settings.json`, preservando o restante.
/// Idempotente: eventos que já chamam `pulse-hook` não são duplicados.
/// Retorna (eventos adicionados, caminho do backup se houve).
pub fn install_claude(settings: &Path, script: &str) -> Result<(Vec<String>, Option<String>), String> {
    let existing = match std::fs::read_to_string(settings) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("não foi possível ler {}: {}", settings.display(), e)),
    };
    let mut root = match &existing {
        Some(s) if !s.trim().is_empty() => {
            json::parse(s).map_err(|e| format!("settings.json inválido, nada foi alterado: {}", e))?
        }
        _ => Value::Obj(vec![]),
    };
    if !matches!(root, Value::Obj(_)) {
        return Err("settings.json não é um objeto JSON".into());
    }
    if root.get("hooks").is_none() {
        root.set("hooks", Value::Obj(vec![]));
    }
    let snippet = json::parse(&claude_snippet(script)).map_err(|e| e.to_string())?;
    let hooks = root.get_mut("hooks").ok_or("hooks ausente")?;
    if !matches!(hooks, Value::Obj(_)) {
        return Err("o campo \"hooks\" não é um objeto".into());
    }
    let mut added = Vec::new();
    for (ev, _, _) in CLAUDE_EVENTS {
        let ours = snippet.path(&["hooks", ev]).and_then(|x| x.as_array()).and_then(|a| a.first()).cloned().unwrap();
        match hooks.get_mut(ev) {
            Some(Value::Arr(list)) => {
                if list.iter().any(|x| x.to_json().contains("pulse-hook")) {
                    continue;
                }
                list.push(ours);
            }
            Some(_) => return Err(format!("hooks.{} tem formato inesperado; nada foi alterado", ev)),
            None => hooks.set(ev, Value::Arr(vec![ours])),
        }
        added.push(ev.to_string());
    }
    if added.is_empty() {
        return Ok((added, None));
    }
    let mut backup = None;
    if let Some(orig) = &existing {
        let b = settings.with_extension("json.pulse-bak");
        std::fs::write(&b, orig).map_err(|e| format!("falha no backup: {}", e))?;
        backup = Some(b.to_string_lossy().into_owned());
    }
    if let Some(dir) = settings.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = settings.with_extension("json.pulse-tmp");
    std::fs::write(&tmp, root.to_pretty() + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, settings).map_err(|e| e.to_string())?;
    Ok((added, backup))
}

/// Linha para `~/.codex/config.toml` (chave de nível superior, antes de
/// qualquer `[tabela]`). O Codex passa o JSON do evento como último argumento.
pub fn codex_snippet(script: &str) -> String {
    format!("notify = [\"{}\", \"aguardando\", \"codex\"]", script.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `notify` do Codex já aponta para o pulse-hook?
pub fn codex_status(config: &Path) -> bool {
    std::fs::read_to_string(config)
        .map(|s| s.lines().any(|l| l.trim_start().starts_with("notify") && l.contains("pulse-hook")))
        .unwrap_or(false)
}

/// Quais eventos do Claude Code já chamam o pulse-hook.
pub fn claude_status(settings: &Path) -> Vec<String> {
    let Ok(s) = std::fs::read_to_string(settings) else { return vec![] };
    let Ok(v) = json::parse(&s) else { return vec![] };
    CLAUDE_EVENTS
        .iter()
        .filter(|(ev, _, _)| v.path(&["hooks", ev]).map(|x| x.to_json().contains("pulse-hook")).unwrap_or(false))
        .map(|(ev, _, _)| ev.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_extracts_only_safe_fields() {
        let mut h = HookStore::default();
        let e = h
            .ingest(5, "claude", "atenção", r#"{"session_id":"s1","cwd":"/p","hook_event_name":"Notification","message":"Precisa de permissão","prompt":"segredo"}"#)
            .unwrap();
        assert_eq!(e.state, "atencao");
        assert_eq!(e.session, "s1");
        assert!(!h.to_json().contains("segredo"));
        assert!(h.ingest(6, "claude", "xyz", "{}").is_err());
        h.ingest(7, "codex", "done", "lixo").unwrap();
        let c = h.ingest(8, "codex", "aguardando", r#"{"type":"agent-turn-complete","thread-id":"t9","last-assistant-message":"texto privado"}"#).unwrap();
        assert_eq!((c.session.as_str(), c.event.as_str()), ("t9", "agent-turn-complete"));
        assert!(!h.to_json().contains("texto privado"));
        assert_eq!(codex_snippet("/x/pulse-hook"), r#"notify = ["/x/pulse-hook", "aguardando", "codex"]"#);
        assert_eq!(h.sessions.len(), 3);
    }

    #[test]
    fn install_is_idempotent_and_preserves_settings() {
        let dir = std::env::temp_dir().join(format!("pulse-hooks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("settings.json");
        std::fs::write(
            &f,
            r#"{"model":"opus","permissions":{"allow":["Bash(ls:*)"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo fim"}]}]},"n":1.50}"#,
        )
        .unwrap();
        let (added, backup) = install_claude(&f, "/opt/pulse/pulse-hook").unwrap();
        assert_eq!(added.len(), 5);
        assert!(backup.is_some());
        let v = json::parse(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v.get("model").unwrap().as_str(), Some("opus"));
        assert!(v.to_json().contains("1.50"));
        assert_eq!(v.path(&["hooks", "Stop"]).unwrap().as_array().unwrap().len(), 2, "hook existente preservado");
        assert_eq!(claude_status(&f).len(), 5);
        let (again, b2) = install_claude(&f, "/opt/pulse/pulse-hook").unwrap();
        assert!(again.is_empty() && b2.is_none());
        // JSON inválido: não altera nada
        std::fs::write(&f, "{ruim").unwrap();
        assert!(install_claude(&f, "/x/pulse-hook").is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{ruim");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
