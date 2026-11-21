//! Sessões do OpenClaw ("Ponto"), lidas do SQLite de cada agente em
//! `~/.openclaw/agents/<nome>/agent/openclaw-agent.sqlite` pelo programa
//! `sqlite3` (opcional), em modo somente leitura.
//!
//! Só metadados: janela da sessão (início/fim), quantidade de mensagens e o
//! modelo (`modelId` dos eventos `model_change`). Nenhum texto de conversa.

use crate::sessions::Session;
use pulse_core::json::{self, Value};
use pulse_core::sys;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const QUERY: &str = "select w.session_id as id, w.session_key as key, w.created_at as created, \
max(coalesce(w.updated_at,0), coalesce(w.transcript_updated_at,0)) as updated, \
(select count(*) from transcript_event_identities i where i.session_id = w.session_id and i.event_type = 'message') as messages, \
(select json_extract(e.event_json, '$.provider') || '/' || json_extract(e.event_json, '$.modelId') \
   from transcript_event_identities i join transcript_events e on e.session_id = i.session_id and e.seq = i.seq \
   where i.session_id = w.session_id and i.event_type = 'model_change' order by i.seq desc limit 1) as model \
from session_windows w order by updated desc limit 20;";

/// Converte a saída de `sqlite3 -json` em sessões.
pub fn parse_rows(agent_dir: &str, src: &str) -> Vec<Session> {
    let Ok(Value::Arr(rows)) = json::parse(src.trim()) else {
        return vec![];
    };
    rows.iter()
        .filter_map(|r| {
            let id = r.get("id")?.as_str()?.to_string();
            let key = r
                .get("key")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let n = |k: &str| r.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
            // chave "agent:<agente>:<rótulo>"
            let label = key.splitn(3, ':').nth(2).unwrap_or(&key).to_string();
            Some(Session {
                agent: "OpenClaw (Ponto)".into(),
                id,
                project: format!("~/.openclaw/agents/{}", agent_dir),
                title: label,
                model: r
                    .get("model")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                started: n("created"),
                updated: n("updated").max(n("created")),
                messages: n("messages"),
                ..Default::default()
            })
        })
        .collect()
}

/// (mtime do banco, mtime do -wal, sessões lidas)
type CacheEntry = (Option<SystemTime>, Option<SystemTime>, Vec<Session>);

pub struct Reader {
    root: PathBuf,
    available: Option<bool>,
    /// banco → (mtime do banco, mtime do -wal, sessões)
    cache: HashMap<PathBuf, CacheEntry>,
    pub note: Option<String>,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl Reader {
    pub fn new(home: &Path) -> Self {
        Reader {
            root: home.join(".openclaw/agents"),
            available: None,
            cache: HashMap::new(),
            note: None,
        }
    }

    fn dbs(&self) -> Vec<(String, PathBuf)> {
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return vec![];
        };
        let mut v: Vec<(String, PathBuf)> = rd
            .flatten()
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    e.path().join("agent/openclaw-agent.sqlite"),
                )
            })
            .filter(|(_, p)| p.is_file())
            .collect();
        v.sort();
        v
    }

    pub fn sessions(&mut self) -> Vec<Session> {
        let dbs = self.dbs();
        if dbs.is_empty() {
            self.note = None;
            return vec![];
        }
        if self.available.is_none() {
            self.available = Some(
                sys::run_cmd("sqlite3", &["-version"], 3_000)
                    .map(|r| r.0)
                    .unwrap_or(false),
            );
        }
        if self.available != Some(true) {
            self.note = Some(
                "há sessões do OpenClaw, mas lê-las exige o programa sqlite3 (sudo apt install sqlite3)".into(),
            );
            return vec![];
        }
        self.note = None;
        let mut out = Vec::new();
        for (agent, db) in dbs {
            let m = mtime(&db);
            let w = mtime(&db.with_extension("sqlite-wal"));
            if let Some((cm, cw, s)) = self.cache.get(&db) {
                if *cm == m && *cw == w {
                    out.extend(s.iter().cloned());
                    continue;
                }
            }
            let path = db.to_string_lossy().into_owned();
            let rows = match sys::run_cmd(
                "sqlite3",
                &["-readonly", "-json", "-cmd", ".timeout 500", &path, QUERY],
                5_000,
            ) {
                Ok((true, txt, _)) => parse_rows(&agent, &txt),
                Ok((false, _, err)) => {
                    self.note = Some(format!(
                        "não foi possível ler o OpenClaw ({}): {}",
                        agent,
                        err.lines().next().unwrap_or("erro do sqlite3")
                    ));
                    vec![]
                }
                Err(e) => {
                    self.note = Some(e);
                    vec![]
                }
            };
            self.cache.insert(db, (m, w, rows.clone()));
            out.extend(rows);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_from_sqlite_json() {
        let src = r#"[{"id":"3f2a","key":"agent:main:gs-notes-validation","created":1793831565281,"updated":1793831791630,"messages":5,"model":"ollama/qwen3:4b-instruct"},
                      {"id":"x","key":"agent:gs:main","created":10,"updated":null,"messages":0,"model":null},
                      {"semid":1}]"#;
        let v = parse_rows("main", src);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].title, "gs-notes-validation");
        assert_eq!(v[0].model, "ollama/qwen3:4b-instruct");
        assert_eq!(
            (v[0].started, v[0].updated, v[0].messages),
            (1793831565281, 1793831791630, 5)
        );
        assert_eq!(v[1].updated, 10, "sem updated usa o início");
        assert!(parse_rows("main", "lixo").is_empty());
        assert!(
            parse_rows("main", "").is_empty(),
            "sqlite3 imprime nada quando não há linhas"
        );
    }

    #[test]
    fn reader_without_openclaw_is_quiet() {
        let home = std::env::temp_dir().join(format!("pulse-oc-{}", std::process::id()));
        let mut r = Reader::new(&home);
        assert!(r.sessions().is_empty());
        assert!(r.note.is_none());
    }
}
