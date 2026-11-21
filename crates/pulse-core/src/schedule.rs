//! Agendamentos (timers do systemd e cron) e contêineres Docker.
//! Tudo somente leitura, exceto iniciar/parar/reiniciar contêiner.

use crate::json::{self, Obj, Value};
use crate::services::Scope;
use crate::sys;

// ------------------------------------------------------------------ timers

#[derive(Debug, Clone, PartialEq)]
pub struct Timer {
    pub unit: String,
    pub activates: String,
    /// Próxima e última execução em ms desde a época (0 = nunca/indefinido).
    pub next_ms: u64,
    pub last_ms: u64,
}

/// Saída de `systemctl list-timers -o json` (tempos em µs).
pub fn parse_timers_json(src: &str) -> Vec<Timer> {
    let Ok(Value::Arr(items)) = json::parse(src.trim()) else {
        return vec![];
    };
    let us = |v: &Value, k: &str| {
        v.get(k)
            .and_then(|x| x.as_f64())
            .map(|x| (x / 1000.0) as u64)
            .unwrap_or(0)
    };
    items
        .iter()
        .filter_map(|v| {
            Some(Timer {
                unit: v.get("unit")?.as_str()?.to_string(),
                activates: v
                    .get("activates")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                next_ms: us(v, "next"),
                last_ms: us(v, "last"),
            })
        })
        .collect()
}

pub fn timers(scope: Scope) -> Result<Vec<Timer>, String> {
    let mut args = vec!["--no-pager", "list-timers", "--all", "-o", "json"];
    if scope == Scope::User {
        args.insert(0, "--user");
    }
    let (ok, out, err) = sys::run_cmd("systemctl", &args, 10_000)?;
    if !ok && out.trim().is_empty() {
        return Err(err.trim().to_string());
    }
    let mut t = parse_timers_json(&out);
    t.sort_by_key(|x| if x.next_ms == 0 { u64::MAX } else { x.next_ms });
    Ok(t)
}

pub fn timers_json(t: &[Timer]) -> String {
    json::arr(t.iter().map(|x| {
        Obj::new()
            .s("unit", &x.unit)
            .s("activates", &x.activates)
            .opt_n(
                "next",
                if x.next_ms > 0 {
                    Some(x.next_ms as f64)
                } else {
                    None
                },
            )
            .opt_n(
                "last",
                if x.last_ms > 0 {
                    Some(x.last_ms as f64)
                } else {
                    None
                },
            )
            .finish()
    }))
}

// -------------------------------------------------------------------- cron

/// Mascara valores que parecem segredos (`token=...`, `--password x`...).
pub fn redact(s: &str) -> String {
    const KEYS: [&str; 8] = [
        "password", "passwd", "pass", "token", "secret", "apikey", "api_key", "key",
    ];
    let mut out: Vec<String> = Vec::new();
    let mut mask_next = false;
    for w in s.split(' ') {
        if mask_next {
            out.push("***".into());
            mask_next = false;
            continue;
        }
        let lower = w.to_lowercase();
        let bare = lower.trim_start_matches('-');
        if let Some((k, _)) = w.split_once('=') {
            let kl = k.to_lowercase();
            let kl = kl.trim_start_matches('-');
            if KEYS.iter().any(|x| kl.ends_with(x)) {
                out.push(format!("{}=***", k));
                continue;
            }
        } else if lower.starts_with('-') && KEYS.contains(&bare) {
            mask_next = true;
        }
        // credenciais em URLs: esquema://usuario:senha@host
        if let Some(i) = w.find("://") {
            if let Some(at) = w[i + 3..].find('@') {
                let cred = &w[i + 3..i + 3 + at];
                if cred.contains(':') {
                    out.push(format!("{}://***@{}", &w[..i], &w[i + 3 + at + 1..]));
                    continue;
                }
            }
        }
        out.push(w.to_string());
    }
    out.join(" ")
}

#[derive(Debug, Clone, PartialEq)]
pub struct CronEntry {
    pub source: String,
    pub schedule: String,
    pub user: String,
    pub command: String,
}

/// Analisa um crontab. `system` = formato com campo de usuário (/etc/crontab, cron.d).
/// Linhas de variáveis (`X=y`) são ignoradas — podem conter segredos.
pub fn parse_crontab(src: &str, source: &str, system: bool, owner: &str) -> Vec<CronEntry> {
    let mut v = Vec::new();
    for line in src.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let first = l.split_whitespace().next().unwrap_or("");
        if first.contains('=') && !first.starts_with('@') {
            continue;
        }
        let parts: Vec<&str> = l.split_whitespace().collect();
        let nsched = if first.starts_with('@') { 1 } else { 5 };
        let need = nsched + if system { 2 } else { 1 };
        if parts.len() < need {
            continue;
        }
        let schedule = parts[..nsched].join(" ");
        let (user, cmd_start) = if system {
            (parts[nsched].to_string(), nsched + 1)
        } else {
            (owner.to_string(), nsched)
        };
        v.push(CronEntry {
            source: source.into(),
            schedule,
            user,
            command: redact(&parts[cmd_start..].join(" "))
                .chars()
                .take(300)
                .collect(),
        });
    }
    v
}

pub fn cron_json() -> String {
    let me = sys::users().get(&sys::uid()).cloned().unwrap_or_default();
    let mut entries = Vec::new();
    if let Ok((true, out, _)) = sys::run_cmd("crontab", &["-l"], 5_000) {
        entries.extend(parse_crontab(&out, "crontab do usuário", false, &me));
    }
    if let Some(s) = sys::read("/etc/crontab") {
        entries.extend(parse_crontab(&s, "/etc/crontab", true, ""));
    }
    if let Ok(rd) = std::fs::read_dir("/etc/cron.d") {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            if let Ok(s) = std::fs::read_to_string(&f) {
                entries.extend(parse_crontab(&s, &f.to_string_lossy(), true, ""));
            }
        }
    }
    let mut periodic = Vec::new();
    for p in ["hourly", "daily", "weekly", "monthly"] {
        if let Ok(rd) = std::fs::read_dir(format!("/etc/cron.{}", p)) {
            let mut names: Vec<String> = rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.') && n != "placeholder")
                .collect();
            names.sort();
            periodic.push(
                Obj::new()
                    .s("period", p)
                    .raw("scripts", &json::arr(names.iter().map(|n| json::quote(n))))
                    .finish(),
            );
        }
    }
    Obj::new()
        .raw(
            "entries",
            &json::arr(entries.iter().map(|e| {
                Obj::new()
                    .s("source", &e.source)
                    .s("schedule", &e.schedule)
                    .s("user", &e.user)
                    .s("command", &e.command)
                    .finish()
            })),
        )
        .raw("periodic", &json::arr(periodic))
        .finish()
}

// ---------------------------------------------------------------- contêineres

#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: String,
    pub created: String,
    pub compose: String,
}

/// Saída de `docker ps -a --format '{{json .}}'` (um objeto por linha).
pub fn parse_docker_ps(src: &str) -> Vec<Container> {
    src.lines()
        .filter_map(|l| json::parse(l).ok())
        .map(|v| {
            let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let labels = g("Labels");
            let compose = labels
                .split(',')
                .find_map(|kv| kv.strip_prefix("com.docker.compose.project="))
                .unwrap_or("")
                .to_string();
            let status = g("Status");
            let mut state = g("State");
            if state.is_empty() {
                state = if status.starts_with("Up") {
                    "running".into()
                } else {
                    "exited".into()
                };
            }
            Container {
                id: g("ID"),
                name: g("Names"),
                image: g("Image"),
                state,
                status,
                ports: g("Ports"),
                created: g("CreatedAt"),
                compose,
            }
        })
        .filter(|c| !c.id.is_empty())
        .collect()
}

/// `docker stats --no-stream`: id curto → (CPU %, uso de memória em texto).
pub fn parse_docker_stats(src: &str) -> Vec<(String, f64, String)> {
    src.lines()
        .filter_map(|l| json::parse(l).ok())
        .filter_map(|v| {
            let id = v.get("ID").and_then(|x| x.as_str())?.to_string();
            let cpu = v
                .get("CPUPerc")
                .and_then(|x| x.as_str())
                .and_then(|s| s.trim_end_matches('%').parse().ok())
                .unwrap_or(0.0);
            let mem = v
                .get("MemUsage")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            Some((id, cpu, mem))
        })
        .collect()
}

pub fn containers_json(with_stats: bool) -> String {
    let available = sys::run_cmd(
        "docker",
        &["version", "--format", "{{.Server.Version}}"],
        5_000,
    );
    let (ok, version, err) = match available {
        Ok(x) => x,
        Err(e) => {
            return Obj::new()
                .b("available", false)
                .s("error", &e)
                .raw("containers", "[]")
                .finish()
        }
    };
    if !ok {
        let msg = if err.contains("permission denied") {
            "sem permissão no socket do Docker (adicione o usuário ao grupo docker)".to_string()
        } else {
            err.lines()
                .next()
                .unwrap_or("Docker indisponível")
                .to_string()
        };
        return Obj::new()
            .b("available", false)
            .s("error", &msg)
            .raw("containers", "[]")
            .finish();
    }
    let list = match sys::run_cmd(
        "docker",
        &["ps", "-a", "--no-trunc=false", "--format", "{{json .}}"],
        10_000,
    ) {
        Ok((_, out, _)) => parse_docker_ps(&out),
        Err(_) => vec![],
    };
    let stats = if with_stats && list.iter().any(|c| c.state == "running") {
        sys::run_cmd(
            "docker",
            &["stats", "--no-stream", "--format", "{{json .}}"],
            15_000,
        )
        .map(|(_, o, _)| parse_docker_stats(&o))
        .unwrap_or_default()
    } else {
        vec![]
    };
    Obj::new()
        .b("available", true)
        .s("version", version.trim())
        .raw(
            "containers",
            &json::arr(list.iter().map(|c| {
                let st = stats
                    .iter()
                    .find(|s| c.id.starts_with(&s.0) || s.0.starts_with(&c.id));
                Obj::new()
                    .s("id", &c.id)
                    .s("name", &c.name)
                    .s("image", &c.image)
                    .s("state", &c.state)
                    .s("status", &c.status)
                    .s("ports", &c.ports)
                    .s("created", &c.created)
                    .s("compose", &c.compose)
                    .opt_n("cpu", st.map(|s| s.1))
                    .opt_s("mem", st.map(|s| s.2.as_str()))
                    .finish()
            })),
        )
        .finish()
}

pub fn valid_container(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
}

pub fn container_action(verb: &str, id: &str) -> Result<String, String> {
    if !matches!(verb, "start" | "stop" | "restart") {
        return Err("ação inválida".into());
    }
    if !valid_container(id) {
        return Err("identificador de contêiner inválido".into());
    }
    let (ok, out, err) = sys::run_cmd("docker", &[verb, "--", id], 60_000)?;
    if ok {
        Ok(out.trim().to_string())
    } else {
        Err(err.trim().to_string())
    }
}

/// Últimas linhas de log de um contêiner (stdout + stderr, com horário).
pub fn container_logs(id: &str, lines: u32) -> Result<String, String> {
    if !valid_container(id) {
        return Err("identificador de contêiner inválido".into());
    }
    let n = lines.clamp(10, 2000).to_string();
    let (ok, out, err) = sys::run_cmd(
        "docker",
        &["logs", "--tail", &n, "--timestamps", "--", id],
        15_000,
    )?;
    if !ok && out.is_empty() {
        return Err(err
            .lines()
            .next()
            .unwrap_or("docker logs falhou")
            .to_string());
    }
    // docker logs escreve o stderr do contêiner no stderr: junta os dois
    let mut all: Vec<&str> = out.lines().chain(err.lines()).collect();
    all.sort(); // linhas começam com o horário RFC 3339
    let mut text = all.join("\n");
    if text.len() > 1_000_000 {
        let cut = text.len() - 1_000_000;
        let cut = (cut..text.len())
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(cut);
        text = text[cut..].to_string();
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timers_from_json() {
        let src = r#"[{"next":1795230000000000,"left":10,"last":null,"passed":null,"unit":"a.timer","activates":"a.service"},{"next":null,"last":1795220000000000,"unit":"b.timer","activates":"b.service"}]"#;
        let t = parse_timers_json(src);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].next_ms, 1_795_230_000_000);
        assert_eq!(t[0].last_ms, 0);
        assert_eq!(t[1].activates, "b.service");
        assert!(parse_timers_json("lixo").is_empty());
    }

    #[test]
    fn crontab_parsing_skips_env_and_redacts() {
        let user = "# c\nMAILTO=x@y\nAPI_TOKEN=abc\n*/5 * * * * /bin/backup --password hunter2 https://u:p@host/x\n@reboot run.sh token=zzz\nbad\n";
        let v = parse_crontab(user, "crontab", false, "ana");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].schedule, "*/5 * * * *");
        assert_eq!(v[0].user, "ana");
        assert!(!v[0].command.contains("hunter2"));
        assert!(!v[0].command.contains("u:p@"));
        assert_eq!(v[1].command, "run.sh token=***");
        let sys_tab =
            "SHELL=/bin/sh\n17 * * * * root cd / && run-parts --report /etc/cron.hourly\n";
        let s = parse_crontab(sys_tab, "/etc/crontab", true, "");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].user, "root");
        assert!(s[0].command.starts_with("cd /"));
    }

    #[test]
    fn docker_ps_and_stats() {
        let ps = "{\"ID\":\"1bad7bde7443\",\"Image\":\"postgres:17\",\"Names\":\"db\",\"State\":\"running\",\"Status\":\"Up 2 hours\",\"Ports\":\"5432/tcp\",\"Labels\":\"a=b,com.docker.compose.project=app\"}\n{\"ID\":\"eb64\",\"Image\":\"mongo\",\"Names\":\"m\",\"Status\":\"Exited (0) 1 day ago\"}\n";
        let c = parse_docker_ps(ps);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].compose, "app");
        assert_eq!(c[1].state, "exited");
        let st = parse_docker_stats(
            "{\"ID\":\"1bad7bde7443\",\"CPUPerc\":\"1.25%\",\"MemUsage\":\"20MiB / 1GiB\"}\n",
        );
        assert_eq!(st[0].1, 1.25);
    }

    #[test]
    fn container_validation() {
        assert!(valid_container("db"));
        assert!(valid_container("1bad7bde7443"));
        assert!(!valid_container("--rm"));
        assert!(!valid_container("a;b"));
        assert!(container_action("rm", "db").is_err());
        assert!(container_logs("--help", 10).is_err());
    }
}
