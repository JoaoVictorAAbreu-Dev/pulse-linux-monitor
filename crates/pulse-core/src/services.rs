//! systemd (systemctl) e journald (journalctl).

use crate::json::{self, Obj, Value};
use crate::sys;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    User,
    System,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Scope> {
        match s {
            "user" | "usuario" => Some(Scope::User),
            "system" | "sistema" => Some(Scope::System),
            _ => None,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::System => "system",
        }
    }
    fn flag(&self) -> Option<&'static str> {
        match self {
            Scope::User => Some("--user"),
            Scope::System => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub name: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
    pub enabled: String,
}

pub fn parse_list_units(src: &str) -> Vec<Unit> {
    src.lines()
        .filter_map(|l| {
            let l = l.trim_start_matches(['●', '*', ' ']);
            let mut it = l.split_whitespace();
            let name = it.next()?.to_string();
            if !name.ends_with(".service") {
                return None;
            }
            let load = it.next()?.to_string();
            let active = it.next()?.to_string();
            let sub = it.next()?.to_string();
            let description = it.collect::<Vec<_>>().join(" ");
            Some(Unit {
                name,
                load,
                active,
                sub,
                description,
                enabled: String::new(),
            })
        })
        .collect()
}

pub fn parse_unit_files(src: &str) -> HashMap<String, String> {
    src.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

fn base_args(scope: Scope) -> Vec<&'static str> {
    let mut v = vec!["--no-pager", "--plain", "--no-legend"];
    if let Some(f) = scope.flag() {
        v.insert(0, f);
    }
    v
}

pub fn list(scope: Scope) -> Result<Vec<Unit>, String> {
    let mut args = base_args(scope);
    args.extend(["list-units", "--type=service", "--all"]);
    let (ok, out, err) = sys::run_cmd("systemctl", &args, 10_000)?;
    if !ok && out.trim().is_empty() {
        return Err(err.trim().to_string());
    }
    let mut units = parse_list_units(&out);
    let mut args = base_args(scope);
    args.extend(["list-unit-files", "--type=service"]);
    if let Ok((_, files, _)) = sys::run_cmd("systemctl", &args, 10_000) {
        let m = parse_unit_files(&files);
        for u in &mut units {
            if let Some(e) = m.get(&u.name) {
                u.enabled = e.clone();
            }
        }
    }
    units.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(units)
}

pub fn list_json(units: &[Unit]) -> String {
    json::arr(units.iter().map(|u| {
        Obj::new()
            .s("name", &u.name)
            .s("load", &u.load)
            .s("active", &u.active)
            .s("sub", &u.sub)
            .s("description", &u.description)
            .s("enabled", &u.enabled)
            .finish()
    }))
}

/// Nome de unidade seguro para passar ao systemctl.
pub fn valid_unit(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('-')
        && name.ends_with(".service")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._:-\\".contains(c))
}

pub fn action(scope: Scope, verb: &str, unit: &str) -> Result<String, String> {
    if !matches!(verb, "start" | "stop" | "restart") {
        return Err("ação inválida".into());
    }
    if !valid_unit(unit) {
        return Err("nome de unidade inválido".into());
    }
    // Escopo do sistema com o pacote instalado: helper próprio via pkexec,
    // com política polkit e mensagem do Pulse (auth_admin_keep).
    if scope == Scope::System && std::path::Path::new(HELPER).exists() {
        let (ok, out, err) = sys::run_cmd("pkexec", &[HELPER, "unit", verb, unit], 180_000)?;
        if ok {
            return Ok(out.trim().to_string());
        }
        // pkexec bloqueado (ex.: unidade com NoNewPrivileges=yes): cai no
        // systemctl, que pede autorização ao polkit pelo D-Bus.
        if !err.contains("must be setuid root") {
            return Err(pkexec_error(&err));
        }
    }
    let mut args: Vec<&str> = Vec::new();
    if let Some(f) = scope.flag() {
        args.push(f);
    }
    // Sem o helper, o systemctl pede autorização ao polkit (agente gráfico
    // da sessão). Não usamos sudo.
    args.extend([verb, "--", unit]);
    let (ok, out, err) = sys::run_cmd("systemctl", &args, 180_000)?;
    if ok {
        Ok(out.trim().to_string())
    } else {
        Err(if err.trim().is_empty() {
            "systemctl falhou".into()
        } else {
            err.trim().to_string()
        })
    }
}

/// Helper privilegiado instalado pelo pacote .deb.
pub const HELPER: &str = "/usr/libexec/pulse-monitor/pulse-helper";

/// Traduz as falhas do pkexec (cancelamento, sem autorização, sem agente).
pub fn pkexec_error(err: &str) -> String {
    let e = err.trim();
    if e.contains("must be setuid root") {
        "o pkexec não pode ganhar privilégio neste processo (serviço com NoNewPrivileges=yes?)"
            .into()
    } else if e.contains("dismissed") || e.contains("cancel") {
        "autenticação cancelada".into()
    } else if e.contains("Not authorized") || e.contains("not authorized") {
        "não autorizado pelo polkit".into()
    } else if e.contains("No authentication agent") || e.contains("no authentication agent") {
        "nenhum agente de autenticação na sessão (a janela de senha não pôde ser exibida)".into()
    } else if e.is_empty() {
        "a ação privilegiada falhou".into()
    } else {
        e.lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or(e)
            .trim_start_matches("pulse-helper: ")
            .to_string()
    }
}

// ------------------------------------------------------------------ journald

pub struct LogQuery {
    pub scope: Scope,
    pub unit: Option<String>,
    pub priority: Option<u8>,
    pub lines: u32,
    pub grep: Option<String>,
    pub after_cursor: Option<String>,
}

fn field_str(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Num(n)) => n.clone(),
        Some(Value::Arr(bytes)) => {
            // journald entrega mensagens binárias como array de bytes.
            let b: Vec<u8> = bytes
                .iter()
                .filter_map(|x| x.as_f64())
                .map(|x| x as u8)
                .collect();
            String::from_utf8_lossy(&b).into_owned()
        }
        _ => String::new(),
    }
}

pub struct LogLine {
    pub ts_us: u64,
    pub priority: u8,
    pub unit: String,
    pub ident: String,
    pub pid: String,
    pub message: String,
    pub cursor: String,
}

pub fn parse_journal_json(src: &str) -> Vec<LogLine> {
    src.lines()
        .filter_map(|l| json::parse(l).ok())
        .map(|v| {
            let unit = {
                let u = field_str(&v, "_SYSTEMD_USER_UNIT");
                if u.is_empty() {
                    field_str(&v, "_SYSTEMD_UNIT")
                } else {
                    u
                }
            };
            LogLine {
                ts_us: field_str(&v, "__REALTIME_TIMESTAMP").parse().unwrap_or(0),
                priority: field_str(&v, "PRIORITY").parse().unwrap_or(6),
                unit,
                ident: field_str(&v, "SYSLOG_IDENTIFIER"),
                pid: field_str(&v, "_PID"),
                message: field_str(&v, "MESSAGE"),
                cursor: field_str(&v, "__CURSOR"),
            }
        })
        .collect()
}

pub fn query_logs(q: &LogQuery) -> Result<String, String> {
    let lines = q.lines.clamp(10, 2000).to_string();
    let mut args: Vec<String> = Vec::new();
    if let Some(f) = q.scope.flag() {
        args.push(f.into());
    }
    args.extend([
        "--no-pager".into(),
        "-o".into(),
        "json".into(),
        "-n".into(),
        lines,
    ]);
    if let Some(u) = &q.unit {
        if !u.is_empty() {
            if !u
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "@._:-\\*".contains(c))
                || u.starts_with('-')
            {
                return Err("unidade inválida".into());
            }
            args.push(format!("--unit={}", u));
        }
    }
    if let Some(p) = q.priority {
        args.push(format!("--priority={}", p.min(7)));
    }
    if let Some(c) = &q.after_cursor {
        if !c.is_empty() {
            args.push(format!("--after-cursor={}", c));
        }
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (_ok, out, err) = sys::run_cmd("journalctl", &refs, 15_000)?;
    let mut entries = parse_journal_json(&out);
    let last_cursor = entries
        .last()
        .map(|e| e.cursor.clone())
        .or_else(|| q.after_cursor.clone())
        .unwrap_or_default();
    if let Some(g) = &q.grep {
        let g = g.to_lowercase();
        if !g.is_empty() {
            entries.retain(|e| {
                e.message.to_lowercase().contains(&g)
                    || e.unit.to_lowercase().contains(&g)
                    || e.ident.to_lowercase().contains(&g)
            });
        }
    }
    let note = if out.trim().is_empty() && !err.trim().is_empty() {
        err.trim().to_string()
    } else {
        String::new()
    };
    Ok(Obj::new()
        .s("cursor", &last_cursor)
        .s("note", &note)
        .raw(
            "lines",
            &json::arr(entries.iter().map(|e| {
                Obj::new()
                    .u("ts", e.ts_us / 1000)
                    .u("priority", e.priority as u64)
                    .s("unit", &e.unit)
                    .s("ident", &e.ident)
                    .s("pid", &e.pid)
                    .s("message", &e.message)
                    .finish()
            })),
        )
        .finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_units() {
        let src = "cron.service loaded active running Regular background program processing daemon\n● bad.service loaded failed failed Bad thing\nfoo.socket loaded active listening x\n";
        let u = parse_list_units(src);
        assert_eq!(u.len(), 2);
        assert_eq!(
            u[0].description,
            "Regular background program processing daemon"
        );
        assert_eq!(u[1].name, "bad.service");
        assert_eq!(u[1].active, "failed");
    }

    #[test]
    fn pkexec_errors() {
        assert_eq!(
            pkexec_error("Error executing command as another user: Request dismissed"),
            "autenticação cancelada"
        );
        assert_eq!(
            pkexec_error("Error executing command as another user: Not authorized"),
            "não autorizado pelo polkit"
        );
        assert!(pkexec_error(
            "Error executing command as another user: No authentication agent found."
        )
        .contains("agente"));
        assert!(pkexec_error("pkexec must be setuid root").contains("NoNewPrivileges"));
        assert_eq!(
            pkexec_error("pulse-helper: nome de unidade inválido\n"),
            "nome de unidade inválido"
        );
    }

    #[test]
    fn unit_validation() {
        assert!(valid_unit("nginx.service"));
        assert!(valid_unit("getty@tty1.service"));
        assert!(!valid_unit("--now.service"));
        assert!(!valid_unit("foo;rm.service"));
        assert!(!valid_unit("foo.socket"));
    }

    #[test]
    fn journal_json() {
        let src = "{\"__CURSOR\":\"c1\",\"__REALTIME_TIMESTAMP\":\"1700000000000000\",\"PRIORITY\":\"3\",\"_SYSTEMD_UNIT\":\"x.service\",\"MESSAGE\":\"falhou\"}\n{\"__CURSOR\":\"c2\",\"MESSAGE\":[111,105],\"PRIORITY\":\"6\"}\nlixo\n";
        let v = parse_journal_json(src);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].priority, 3);
        assert_eq!(v[0].ts_us / 1000, 1_700_000_000_000);
        assert_eq!(v[1].message, "oi");
        assert_eq!(v[1].cursor, "c2");
    }
}
