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
    /// Reconstrói um evento persistido; `None` se a linha for inválida.
    pub fn from_json(line: &str) -> Option<HookEvent> {
        let v = json::parse(line).ok()?;
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        Some(HookEvent {
            ts: v.get("ts")?.as_f64()? as u64,
            agent: clean(&g("agent"), 40),
            state: normalize_state(v.get("state")?.as_str()?)?,
            session: clean(v.get("session")?.as_str()?, 80),
            cwd: clean(&g("cwd"), 300),
            event: clean(&g("event"), 40),
            tool: clean(&g("tool"), 160),
            message: clean(&g("message"), 200),
        })
    }

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
    pub meta: HashMap<String, SessionMeta>,
    pub received: u64,
}

impl HookStore {
    /// Registra um evento. `payload` é o JSON que o agente passou ao hook
    /// (só alguns campos são extraídos; o resto é descartado).
    pub fn ingest(
        &mut self,
        ts: u64,
        agent: &str,
        state: &str,
        payload: &str,
        details: bool,
    ) -> Result<HookEvent, String> {
        let state = normalize_state(state).ok_or("estado inválido")?;
        let v = if payload.trim().is_empty() {
            Value::Obj(vec![])
        } else {
            json::parse(payload).unwrap_or(Value::Obj(vec![]))
        };
        let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let agent = {
            let a = clean(agent, 40);
            if a.is_empty() {
                "claude".to_string()
            } else {
                a
            }
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
            if s.is_empty() {
                format!("{}-sem-sessao", agent)
            } else {
                clean(&s, 80)
            }
        };
        let e = HookEvent {
            ts,
            agent,
            state,
            session: session.clone(),
            cwd: clean(&g("cwd"), 300),
            event: clean(
                &{
                    let e = g("hook_event_name");
                    if e.is_empty() {
                        g("type")
                    } else {
                        e
                    }
                },
                40,
            ),
            tool: {
                let name = clean(&g("tool_name"), 60);
                let summary = if details && !name.is_empty() {
                    v.get("tool_input")
                        .map(|i| crate::detail::tool_summary(&name, i))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                if summary.is_empty() {
                    name
                } else {
                    clean(&format!("{} · {}", name, summary), 160)
                }
            },
            message: clean(&g("message"), 200),
        };
        self.remember(e.clone());
        self.received += 1;
        Ok(e)
    }

    fn remember(&mut self, e: HookEvent) {
        let m = self.meta.entry(e.session.clone()).or_default();
        match e.event.as_str() {
            "SubagentStart" => {
                m.subagents_active += 1;
                m.subagents_total += 1;
            }
            "SubagentStop" => m.subagents_active = m.subagents_active.saturating_sub(1),
            "PostToolUseFailure" => {
                m.failures += 1;
                m.last_failure = e.tool.clone();
            }
            // fim de turno ou de sessão: nada mais está rodando
            "Stop" | "SessionEnd" => m.subagents_active = 0,
            _ => {}
        }
        if self.events.len() >= 300 {
            self.events.pop_front();
        }
        self.events.push_back(e.clone());
        self.sessions.insert(e.session.clone(), e);
        if self.sessions.len() > 100 {
            if let Some(oldest) = self
                .sessions
                .iter()
                .min_by_key(|(_, e)| e.ts)
                .map(|(k, _)| k.clone())
            {
                self.sessions.remove(&oldest);
                self.meta.remove(&oldest);
            }
        }
    }

    /// Recarrega eventos persistidos (linhas de `HookEvent::to_json`).
    pub fn restore<'a>(&mut self, lines: impl Iterator<Item = &'a str>) {
        for l in lines {
            if let Some(e) = HookEvent::from_json(l) {
                self.remember(e);
            }
        }
    }

    pub fn to_json(&self) -> String {
        let mut s: Vec<&HookEvent> = self.sessions.values().collect();
        s.sort_by_key(|e| std::cmp::Reverse(e.ts));
        Obj::new()
            .u("received", self.received)
            .raw(
                "sessions",
                &json::arr(s.into_iter().map(|e| {
                    let m = self.meta.get(&e.session).cloned().unwrap_or_default();
                    let o = e.to_json();
                    format!(
                        "{},\"subagents\":{},\"subagents_total\":{},\"failures\":{},\"last_failure\":{}}}",
                        &o[..o.len() - 1],
                        m.subagents_active,
                        m.subagents_total,
                        m.failures,
                        json::quote(&m.last_failure)
                    )
                })),
            )
            .raw(
                "events",
                &json::arr(self.events.iter().rev().take(100).map(|e| e.to_json())),
            )
            .finish()
    }
}

/// Eventos do Claude Code e o estado que cada um envia.
pub const CLAUDE_EVENTS: [(&str, &str, bool); 9] = [
    ("UserPromptSubmit", "executando", false),
    ("PreToolUse", "executando", true),
    ("PostToolUseFailure", "executando", true),
    ("PermissionRequest", "atencao", true),
    ("Notification", "atencao", false),
    ("SubagentStart", "executando", false),
    ("SubagentStop", "executando", false),
    ("Stop", "aguardando", false),
    ("SessionEnd", "concluido", false),
];

/// Contadores por sessão derivados dos eventos.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionMeta {
    /// Subagentes em execução agora (SubagentStart − SubagentStop).
    pub subagents_active: u32,
    pub subagents_total: u32,
    /// Ferramentas que falharam (PostToolUseFailure).
    pub failures: u32,
    pub last_failure: String,
}

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
pub fn install_claude(
    settings: &Path,
    script: &str,
) -> Result<(Vec<String>, Option<String>), String> {
    let existing = match std::fs::read_to_string(settings) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(format!(
                "não foi possível ler {}: {}",
                settings.display(),
                e
            ))
        }
    };
    let mut root = match &existing {
        Some(s) if !s.trim().is_empty() => json::parse(s)
            .map_err(|e| format!("settings.json inválido, nada foi alterado: {}", e))?,
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
        let ours = snippet
            .path(&["hooks", ev])
            .and_then(|x| x.as_array())
            .and_then(|a| a.first())
            .cloned()
            .unwrap();
        match hooks.get_mut(ev) {
            Some(Value::Arr(list)) => {
                if list.iter().any(|x| x.to_json().contains("pulse-hook")) {
                    continue;
                }
                list.push(ours);
            }
            Some(_) => {
                return Err(format!(
                    "hooks.{} tem formato inesperado; nada foi alterado",
                    ev
                ))
            }
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
    format!(
        "notify = [\"{}\", \"aguardando\", \"codex\"]",
        script.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// Região de topo de um `config.toml` (chaves antes da primeira `[tabela]`).
#[derive(Debug, PartialEq)]
struct TomlTop {
    /// Índice da primeira linha de cabeçalho de tabela (ou o total de linhas).
    end: usize,
    /// Linha logo após a última instrução da região de topo.
    after_last: usize,
    /// Texto (sem comentários) do `notify` de topo, se houver.
    notify: Option<String>,
}

/// Código de uma linha sem o comentário, e a variação de profundidade de
/// colchetes fora de strings (strings de uma linha só; as multilinha são
/// recusadas antes).
fn toml_code(line: &str) -> (String, i32) {
    let mut out = String::new();
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut esc = false;
    for c in line.chars() {
        match quote {
            Some(q) => {
                if esc {
                    esc = false;
                } else if c == '\\' && q == '"' {
                    esc = true;
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '#' => break,
                '"' | '\'' => quote = Some(c),
                '[' => depth += 1,
                ']' => depth -= 1,
                _ => {}
            },
        }
        out.push(c);
    }
    (out, depth)
}

fn is_notify_key(code: &str) -> bool {
    let t = code.trim_start();
    let rest = t
        .strip_prefix("notify")
        .or_else(|| t.strip_prefix("\"notify\""))
        .or_else(|| t.strip_prefix("'notify'"));
    rest.map(|r| r.trim_start().starts_with('='))
        .unwrap_or(false)
}

/// Analisa a região de topo. Erro se houver strings multilinha (não dá para
/// saber com segurança onde as instruções terminam).
fn toml_top(lines: &[&str]) -> Result<TomlTop, String> {
    let mut depth = 0i32;
    let mut after_last = 0;
    let mut notify: Option<String> = None;
    let mut in_notify = false;
    for (i, line) in lines.iter().enumerate() {
        if depth <= 0 && line.trim_start().starts_with('[') {
            return Ok(TomlTop {
                end: i,
                after_last,
                notify,
            });
        }
        if line.contains("\"\"\"") || line.contains("'''") {
            return Err(
                "o início do config.toml tem strings multilinha (\"\"\" ou '''); adicione a linha do notify manualmente"
                    .into(),
            );
        }
        let (code, d) = toml_code(line);
        if code.trim().is_empty() && depth <= 0 {
            continue;
        }
        if depth <= 0 {
            in_notify = is_notify_key(&code);
            if in_notify {
                if notify.is_some() {
                    return Err("o config.toml tem mais de um notify; nada foi alterado".into());
                }
                notify = Some(String::new());
            }
        }
        if in_notify {
            if let Some(n) = notify.as_mut() {
                n.push_str(&code);
                n.push('\n');
            }
        }
        depth = (depth + d).max(0);
        after_last = i + 1;
    }
    Ok(TomlTop {
        end: lines.len(),
        after_last,
        notify,
    })
}

/// Adiciona o `notify` do Pulse ao `~/.codex/config.toml`, só na região de
/// topo. Idempotente; recusa (sem tocar no arquivo) se já houver outro
/// `notify` ou se o topo tiver strings multilinha. Faz backup
/// (`config.toml.pulse-bak`) e grava de forma atômica preservando as permissões.
/// Retorna (alterou?, caminho do backup se houve).
pub fn install_codex(config: &Path, script: &str) -> Result<(bool, Option<String>), String> {
    // arquivo gerenciado por link simbólico (dotfiles): grava no destino
    let config = std::fs::canonicalize(config).unwrap_or_else(|_| config.to_path_buf());
    let config = config.as_path();
    let existing = match std::fs::read_to_string(config) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("não foi possível ler {}: {}", config.display(), e)),
    };
    let src = existing.clone().unwrap_or_default();
    let mut lines: Vec<&str> = src.split_inclusive('\n').collect();
    let top = toml_top(&lines)?;
    if let Some(n) = &top.notify {
        if n.contains("pulse-hook") {
            return Ok((false, None));
        }
        return Err(
            "o config.toml já tem um notify de outro programa; nada foi alterado (o Codex aceita só um notify)"
                .into(),
        );
    }
    let ours = format!("{}\n", codex_snippet(script));
    let at = top.after_last;
    let mut fixed_prev = String::new();
    if at > 0 && !lines[at - 1].ends_with('\n') {
        fixed_prev = format!("{}\n", lines[at - 1]);
    }
    if !fixed_prev.is_empty() {
        lines[at - 1] = &fixed_prev;
    }
    let mut insert = vec![ours.as_str()];
    if at < lines.len() && !lines[at].trim().is_empty() {
        insert.push("\n");
    }
    let body: String = lines[..at]
        .iter()
        .chain(insert.iter())
        .chain(lines[at..].iter())
        .copied()
        .collect();
    let perms = std::fs::metadata(config).ok().map(|m| m.permissions());
    let mut backup = None;
    if existing.is_some() {
        let b = config.with_extension("toml.pulse-bak");
        // copy preserva as permissões do original (o arquivo pode ter tokens)
        std::fs::copy(config, &b).map_err(|e| format!("falha no backup: {}", e))?;
        backup = Some(b.to_string_lossy().into_owned());
    }
    if let Some(dir) = config.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = config.with_extension("toml.pulse-tmp");
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    if let Some(p) = perms {
        std::fs::set_permissions(&tmp, p).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, config).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok((true, backup))
}

/// `notify` de topo do Codex já aponta para o pulse-hook? (comentários e
/// chaves dentro de tabelas não contam)
pub fn codex_status(config: &Path) -> bool {
    let Ok(s) = std::fs::read_to_string(config) else {
        return false;
    };
    let lines: Vec<&str> = s.split_inclusive('\n').collect();
    toml_top(&lines)
        .ok()
        .and_then(|t| t.notify)
        .map(|n| n.contains("pulse-hook"))
        .unwrap_or(false)
}

/// Quais eventos do Claude Code já chamam o pulse-hook.
pub fn claude_status(settings: &Path) -> Vec<String> {
    let Ok(s) = std::fs::read_to_string(settings) else {
        return vec![];
    };
    let Ok(v) = json::parse(&s) else {
        return vec![];
    };
    CLAUDE_EVENTS
        .iter()
        .filter(|(ev, _, _)| {
            v.path(&["hooks", ev])
                .map(|x| x.to_json().contains("pulse-hook"))
                .unwrap_or(false)
        })
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
            .ingest(5, "claude", "atenção", r#"{"session_id":"s1","cwd":"/p","hook_event_name":"Notification","message":"Precisa de permissão","prompt":"segredo"}"#, false)
            .unwrap();
        assert_eq!(e.state, "atencao");
        assert_eq!(e.session, "s1");
        assert!(!h.to_json().contains("segredo"));
        assert!(h.ingest(6, "claude", "xyz", "{}", false).is_err());
        h.ingest(7, "codex", "done", "lixo", false).unwrap();
        let c = h.ingest(8, "codex", "aguardando", r#"{"type":"agent-turn-complete","thread-id":"t9","last-assistant-message":"texto privado"}"#, false).unwrap();
        assert_eq!(
            (c.session.as_str(), c.event.as_str()),
            ("t9", "agent-turn-complete")
        );
        assert!(!h.to_json().contains("texto privado"));
        assert_eq!(
            codex_snippet("/x/pulse-hook"),
            r#"notify = ["/x/pulse-hook", "aguardando", "codex"]"#
        );
        assert_eq!(h.sessions.len(), 3);
        let lines: Vec<String> = h.events.iter().map(|e| e.to_json()).collect();
        let mut r = HookStore::default();
        r.restore(
            lines
                .iter()
                .map(|s| s.as_str())
                .chain(["lixo", r#"{"ts":1,"state":"xyz","session":"a"}"#]),
        );
        assert_eq!(r.events.len(), h.events.len());
        assert_eq!(r.sessions.len(), 3);
        assert_eq!(r.sessions["t9"].state, "aguardando");
    }

    #[test]
    fn subagents_failures_and_details() {
        let mut h = HookStore::default();
        let ev = |name: &str, extra: &str| {
            format!(
                r#"{{"session_id":"s","hook_event_name":"{}"{}}}"#,
                name, extra
            )
        };
        h.ingest(1, "claude", "executando", &ev("SubagentStart", ""), false)
            .unwrap();
        h.ingest(2, "claude", "executando", &ev("SubagentStart", ""), false)
            .unwrap();
        h.ingest(3, "claude", "executando", &ev("SubagentStop", ""), false)
            .unwrap();
        h.ingest(
            4,
            "claude",
            "executando",
            &ev("PostToolUseFailure", r#","tool_name":"Bash""#),
            false,
        )
        .unwrap();
        let m = h.meta["s"].clone();
        assert_eq!(
            (m.subagents_active, m.subagents_total, m.failures),
            (1, 2, 1)
        );
        assert_eq!(m.last_failure, "Bash");
        assert!(h.to_json().contains("\"subagents\":1"));
        // eventos persistidos reconstroem os contadores
        let lines: Vec<String> = h.events.iter().map(|e| e.to_json()).collect();
        let mut r = HookStore::default();
        r.restore(lines.iter().map(|s| s.as_str()));
        assert_eq!(r.meta["s"], m);
        h.ingest(5, "claude", "aguardando", &ev("Stop", ""), false)
            .unwrap();
        assert_eq!(h.meta["s"].subagents_active, 0);
        // detalhes: só com opt-in, mascarados
        let pre = ev(
            "PreToolUse",
            r#","tool_name":"Bash","tool_input":{"command":"deploy --token SEGREDO"}"#,
        );
        assert_eq!(
            h.ingest(6, "claude", "executando", &pre, false)
                .unwrap()
                .tool,
            "Bash"
        );
        let d = h.ingest(7, "claude", "executando", &pre, true).unwrap();
        assert_eq!(d.tool, "Bash · deploy --token ***");
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
        assert_eq!(added.len(), CLAUDE_EVENTS.len());
        assert!(backup.is_some());
        let v = json::parse(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v.get("model").unwrap().as_str(), Some("opus"));
        assert!(v.to_json().contains("1.50"));
        assert_eq!(
            v.path(&["hooks", "Stop"])
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2,
            "hook existente preservado"
        );
        assert_eq!(claude_status(&f).len(), CLAUDE_EVENTS.len());
        let (again, b2) = install_claude(&f, "/opt/pulse/pulse-hook").unwrap();
        assert!(again.is_empty() && b2.is_none());
        // JSON inválido: não altera nada
        std::fs::write(&f, "{ruim").unwrap();
        assert!(install_claude(&f, "/x/pulse-hook").is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{ruim");
        let _ = std::fs::remove_dir_all(&dir);
    }
    fn codex_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pulse-codex-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const HOOK: &str = "/usr/bin/pulse-hook";

    #[test]
    fn codex_missing_and_empty() {
        let dir = codex_dir("novo");
        let f = dir.join(".codex/config.toml");
        assert!(!codex_status(&f));
        assert_eq!(install_codex(&f, HOOK).unwrap(), (true, None));
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            format!("{}\n", codex_snippet(HOOK))
        );
        assert!(codex_status(&f));
        assert!(!dir.join(".codex/config.toml.pulse-tmp").exists());
        // vazio: grava, com backup do arquivo (vazio) original
        std::fs::write(&f, "").unwrap();
        let (changed, backup) = install_codex(&f, HOOK).unwrap();
        assert!(changed && backup.is_some());
        assert!(codex_status(&f));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn codex_tables_idempotent_backup_and_perms() {
        use std::os::unix::fs::PermissionsExt;
        let dir = codex_dir("tabelas");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("config.toml");
        let orig = "# Codex\nmodel = \"gpt-5\"\nsandbox = [\n  \"a\",\n  [1, 2],\n]\n\n# perfis\n[profiles.x]\nnotify = [\"outro\"]\n[[mcp]]\nname = \"m\"\n";
        std::fs::write(&f, orig).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!codex_status(&f), "notify dentro de tabela não conta");
        let (changed, backup) = install_codex(&f, HOOK).unwrap();
        assert!(changed);
        let b = backup.unwrap();
        assert!(b.ends_with("config.toml.pulse-bak"));
        assert_eq!(std::fs::read_to_string(&b).unwrap(), orig);
        let now = std::fs::read_to_string(&f).unwrap();
        assert_eq!(
            now,
            format!(
                "# Codex\nmodel = \"gpt-5\"\nsandbox = [\n  \"a\",\n  [1, 2],\n]\n{}\n\n# perfis\n[profiles.x]\nnotify = [\"outro\"]\n[[mcp]]\nname = \"m\"\n",
                codex_snippet(HOOK)
            )
        );
        assert_eq!(
            std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(codex_status(&f));
        // segunda vez: nada muda, sem novo backup
        std::fs::remove_file(&b).unwrap();
        assert_eq!(install_codex(&f, HOOK).unwrap(), (false, None));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), now);
        assert!(!std::path::Path::new(&b).exists());
        // sem quebra de linha final e tabela logo no início
        std::fs::write(&f, "model = \"x\"").unwrap();
        install_codex(&f, HOOK).unwrap();
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            format!("model = \"x\"\n{}\n", codex_snippet(HOOK))
        );
        std::fs::write(&f, "[tui]\nx = 1\n").unwrap();
        install_codex(&f, HOOK).unwrap();
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            format!("{}\n\n[tui]\nx = 1\n", codex_snippet(HOOK))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn codex_foreign_notify_and_comments() {
        let dir = codex_dir("alheio");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("config.toml");
        for src in [
            "model = \"x\"\nnotify = [\"notify-send\", \"Codex\"]\n",
            "notify = [\n  \"python3\",\n  \"/home/u/notify.py\",\n]\n[tui]\n",
            "\"notify\" = [\"x\"]\n",
            "a = \"\"\"\nmulti\n\"\"\"\n",
        ] {
            std::fs::write(&f, src).unwrap();
            assert!(install_codex(&f, HOOK).is_err(), "{}", src);
            assert_eq!(std::fs::read_to_string(&f).unwrap(), src, "intocado");
            assert!(!dir.join("config.toml.pulse-bak").exists());
            assert!(!codex_status(&f));
        }
        // notify multilinha que já é o do Pulse: ok sem escrever
        let ours = format!(
            "notify = [\n  \"{}\",\n  \"aguardando\", \"codex\",\n]\n",
            HOOK
        );
        std::fs::write(&f, &ours).unwrap();
        assert!(codex_status(&f));
        assert_eq!(install_codex(&f, HOOK).unwrap(), (false, None));
        // comentário com notify não conta e não bloqueia
        let c = "# notify = [\"/usr/bin/pulse-hook\"]\nmodel = \"x\" # notify = 1\n";
        std::fs::write(&f, c).unwrap();
        assert!(!codex_status(&f));
        assert!(install_codex(&f, HOOK).unwrap().0);
        assert!(std::fs::read_to_string(&f).unwrap().starts_with(&format!(
            "{}{}\n",
            c,
            codex_snippet(HOOK)
        )));
        assert!(codex_status(&f));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
