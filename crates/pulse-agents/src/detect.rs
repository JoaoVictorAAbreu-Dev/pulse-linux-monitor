//! Detecção de processos de agentes de IA e servidores MCP.

use pulse_core::json::{self, Obj};
use pulse_core::procs::{self, Proc};
use std::collections::HashMap;

/// Regra de detecção. Padrão `cmd:texto` casa com substring da linha de
/// comando; os demais casam com o nome-base do executável/argv0/comm.
#[derive(Debug, Clone)]
pub struct AgentRule {
    pub name: String,
    pub patterns: Vec<String>,
}

pub fn default_rules() -> Vec<AgentRule> {
    let r = |n: &str, p: &[&str]| AgentRule { name: n.into(), patterns: p.iter().map(|s| s.to_string()).collect() };
    vec![
        r("Claude Code", &["claude", "cmd:@anthropic-ai/claude-code", "cmd:/claude-code/cli"]),
        r("Codex", &["codex", "cmd:@openai/codex", "cmd:/codex/codex"]),
        r("OpenClaw (Ponto)", &["openclaw", "ponto", "cmd:openclaw/dist/index.js", "cmd:/bin/openclaw"]),
    ]
}

/// Lê `agents.conf`: linhas `Nome = padrão1, padrão2`; `#` comenta.
pub fn parse_conf(src: &str) -> Vec<AgentRule> {
    src.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let (n, p) = l.split_once('=')?;
            let patterns: Vec<String> = p.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
            if n.trim().is_empty() || patterns.is_empty() {
                return None;
            }
            Some(AgentRule { name: n.trim().to_string(), patterns })
        })
        .collect()
}

fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

pub fn match_rule<'a>(rules: &'a [AgentRule], p: &Proc) -> Option<&'a AgentRule> {
    let argv0 = basename(p.cmd.split_whitespace().next().unwrap_or(""));
    // Exclui o app desktop/Electron e ferramentas auxiliares homônimas.
    if p.name.contains("desktop") || argv0.contains("desktop") || p.cmd.contains("--type=") {
        return None;
    }
    rules.iter().find(|r| {
        r.patterns.iter().any(|pat| match pat.strip_prefix("cmd:") {
            Some(sub) => p.cmd.contains(sub),
            None => p.name == *pat || argv0 == pat,
        })
    })
}

pub struct AgentProc {
    pub agent: String,
    pub root: Proc,
    pub children: usize,
    pub tree_cpu: f64,
    pub tree_rss: u64,
    pub cwd: Option<String>,
    pub mcp: Vec<Proc>,
}

const SHELLS: [&str; 6] = ["sh", "bash", "zsh", "dash", "fish", "timeout"];

/// Servidor MCP: algum argumento (executável ou script) tem "mcp" no nome.
/// Shells são ignorados — um `bash -c "... mcp ..."` não é servidor.
fn is_mcp(p: &Proc) -> bool {
    !SHELLS.contains(&p.name.as_str()) && mcp_label(p).is_some()
}

/// Nome legível do servidor MCP (ex.: `mcp-server-filesystem`).
pub fn mcp_label(p: &Proc) -> Option<String> {
    p.cmd
        .split_whitespace()
        .take(6)
        .filter(|a| !a.starts_with('-'))
        .map(|a| a.trim_end_matches('/').rsplit('/').next().unwrap_or(a))
        .find(|b| b.to_lowercase().contains("mcp") && !b.to_lowercase().contains("pulse"))
        .map(|b| b.trim_end_matches(".js").trim_end_matches(".py").to_string())
}

/// Agrupa processos de agentes por raiz (processo casado cujo pai não casa
/// com o mesmo agente) e soma a árvore de descendentes.
pub fn detect(rules: &[AgentRule], list: &[Proc]) -> Vec<AgentProc> {
    let by_pid: HashMap<i32, &Proc> = list.iter().map(|p| (p.pid, p)).collect();
    let mut kids: HashMap<i32, Vec<&Proc>> = HashMap::new();
    for p in list {
        kids.entry(p.ppid).or_default().push(p);
    }
    let matched: HashMap<i32, &AgentRule> =
        list.iter().filter_map(|p| match_rule(rules, p).map(|r| (p.pid, r))).collect();
    let mut out = Vec::new();
    for p in list {
        let Some(rule) = matched.get(&p.pid) else { continue };
        // Sobe a árvore: se algum ancestral é do mesmo agente, não é raiz.
        let mut anc = p.ppid;
        let mut is_root = true;
        let mut guard = 0;
        while anc > 1 && guard < 64 {
            if matched.get(&anc).map(|r| r.name == rule.name).unwrap_or(false) {
                is_root = false;
                break;
            }
            anc = by_pid.get(&anc).map(|x| x.ppid).unwrap_or(0);
            guard += 1;
        }
        if !is_root {
            continue;
        }
        let mut stack = vec![p.pid];
        let mut desc: Vec<&Proc> = Vec::new();
        while let Some(pid) = stack.pop() {
            if let Some(ch) = kids.get(&pid) {
                for c in ch {
                    if desc.len() < 2000 {
                        desc.push(c);
                        stack.push(c.pid);
                    }
                }
            }
        }
        out.push(AgentProc {
            agent: rule.name.clone(),
            children: desc.len(),
            tree_cpu: p.cpu + desc.iter().map(|d| d.cpu).sum::<f64>(),
            tree_rss: p.rss + desc.iter().map(|d| d.rss).sum::<u64>(),
            cwd: procs::cwd(p.pid),
            mcp: desc.iter().filter(|d| is_mcp(d)).map(|d| (*d).clone()).collect(),
            root: p.clone(),
        });
    }
    out.sort_by(|a, b| b.tree_cpu.partial_cmp(&a.tree_cpu).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub fn to_json(list: &[AgentProc]) -> String {
    json::arr(list.iter().map(|a| {
        Obj::new()
            .s("agent", &a.agent)
            .i("pid", a.root.pid as i64)
            .s("name", &a.root.name)
            .s("cmd", &a.root.cmd)
            .s("user", &a.root.user)
            .s("state", &a.root.state.to_string())
            .n("cpu", a.root.cpu)
            .u("rss", a.root.rss)
            .n("tree_cpu", a.tree_cpu)
            .u("tree_rss", a.tree_rss)
            .u("children", a.children as u64)
            .opt_s("cwd", a.cwd.as_deref())
            .raw(
                "mcp",
                &json::arr(a.mcp.iter().map(|m| {
                    Obj::new().i("pid", m.pid as i64).s("name", &mcp_label(m).unwrap_or_else(|| m.name.clone())).s("cmd", &m.cmd).n("cpu", m.cpu).u("rss", m.rss).finish()
                })),
            )
            .finish()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: i32, ppid: i32, name: &str, cmd: &str, cpu: f64) -> Proc {
        Proc { pid, ppid, name: name.into(), cmd: cmd.into(), cpu, rss: 1000, state: 'S', ..Default::default() }
    }

    #[test]
    fn conf_parsing() {
        let r = parse_conf("# comentário\nAider = aider, cmd:aider-chat\nruim\n = x\n");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "Aider");
        assert_eq!(r[0].patterns, vec!["aider", "cmd:aider-chat"]);
    }

    #[test]
    fn detects_roots_children_and_mcp() {
        let rules = default_rules();
        let list = vec![
            p(10, 1, "bash", "/bin/bash", 0.0),
            p(11, 10, "claude", "/home/u/.local/bin/claude --resume", 5.0),
            p(12, 11, "node", "node /x/mcp-server-filesystem /tmp", 1.0),
            p(13, 11, "bash", "bash -c 'grep mcp x'", 0.5),
            p(14, 11, "claude", "claude --child", 0.0),
            p(20, 1, "node-MainThread", "/opt/node/bin/node /x/lib/node_modules/openclaw/dist/index.js gateway", 2.0),
            p(30, 1, "claude-desktop", "/usr/lib/claude-desktop/claude-desktop", 9.0),
            p(40, 1, "codex", "codex exec", 0.1),
        ];
        let a = detect(&rules, &list);
        assert_eq!(a.len(), 3, "claude, openclaw, codex (desktop excluído)");
        let c = a.iter().find(|x| x.agent == "Claude Code").unwrap();
        assert_eq!(c.root.pid, 11);
        assert_eq!(c.children, 3);
        assert_eq!(c.mcp.len(), 1, "shell que menciona mcp não conta");
        assert_eq!(mcp_label(&c.mcp[0]).as_deref(), Some("mcp-server-filesystem"));
        assert!((c.tree_cpu - 6.5).abs() < 1e-9);
        assert!(a.iter().any(|x| x.agent == "OpenClaw (Ponto)"));
    }
}
