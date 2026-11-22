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

/// Motor de contêineres: "docker" se `docker version` responde, senão
/// "podman". Só a escolha que funcionou fica em cache (um daemon do Docker
/// iniciado depois do Pulse ainda é detectado).
pub fn engine() -> &'static str {
    static ENGINE: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    if let Some(e) = ENGINE.get() {
        return e;
    }
    let works = |prog: &str, fmt: &str| {
        matches!(
            sys::run_cmd(prog, &["version", "--format", fmt], 5_000),
            Ok((true, _, _))
        )
    };
    if works("docker", "{{.Server.Version}}") {
        return ENGINE.get_or_init(|| "docker");
    }
    if works("podman", "{{.Client.Version}}") {
        return ENGINE.get_or_init(|| "podman");
    }
    // nenhum responde: sem Docker instalado mas com Podman, o erro do Podman
    // é o mais útil; nos demais casos, o do Docker
    if sys::run_cmd("docker", &["--version"], 3_000).is_err()
        && sys::run_cmd("podman", &["--version"], 3_000).is_ok()
    {
        "podman"
    } else {
        "docker"
    }
}

fn engine_label(engine: &str) -> &'static str {
    if engine == "podman" {
        "Podman"
    } else {
        "Docker"
    }
}

/// Lê objetos JSON de um por linha (`{{json .}}`) ou de um array (`--format json`).
fn json_items(src: &str) -> Vec<Value> {
    let t = src.trim();
    if t.starts_with('[') {
        return match json::parse(t) {
            Ok(Value::Arr(v)) => v,
            _ => vec![],
        };
    }
    t.lines().filter_map(|l| json::parse(l).ok()).collect()
}

fn first_str(v: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| v.get(k).and_then(|x| x.as_str()).filter(|s| !s.is_empty()))
        .unwrap_or("")
        .to_string()
}

/// Portas no formato do Docker ("0.0.0.0:8080->80/tcp, …"); o Podman manda
/// uma lista de objetos.
fn ports_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Arr(list)) => list
            .iter()
            .map(|p| {
                let n = |k: &str| p.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
                let ip = p.get("host_ip").and_then(|x| x.as_str()).unwrap_or("");
                let proto = p.get("protocol").and_then(|x| x.as_str()).unwrap_or("tcp");
                let (host, cont, range) = (n("host_port"), n("container_port"), n("range").max(1));
                let span = |a: u64| {
                    if range > 1 {
                        format!("{}-{}", a, a + range - 1)
                    } else {
                        a.to_string()
                    }
                };
                let ip = if ip.is_empty() {
                    String::new()
                } else {
                    format!("{}:", ip)
                };
                if host == 0 {
                    format!("{}/{}", span(cont), proto)
                } else {
                    format!("{}{}->{}/{}", ip, span(host), span(cont), proto)
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

/// Projeto do Compose (Docker Compose ou podman-compose) pelos rótulos, que
/// vêm como texto "k=v,k=v" (Docker) ou objeto (Podman).
fn compose_project(v: Option<&Value>) -> String {
    const KEYS: [&str; 2] = ["com.docker.compose.project", "io.podman.compose.project"];
    match v {
        Some(Value::Str(s)) => s
            .split(',')
            .find_map(|kv| {
                let (k, val) = kv.split_once('=')?;
                KEYS.contains(&k).then(|| val.to_string())
            })
            .unwrap_or_default(),
        Some(o @ Value::Obj(_)) => KEYS
            .iter()
            .find_map(|k| o.get(k).and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

/// Saída de `docker ps -a --format '{{json .}}'` (um objeto por linha) ou de
/// `podman ps -a --format json` (array; `Id`, `Names` lista, `Labels` objeto).
pub fn parse_docker_ps(src: &str) -> Vec<Container> {
    json_items(src)
        .iter()
        .map(|v| {
            let g = |k: &str| first_str(v, &[k]);
            let mut status = g("Status");
            let mut state = g("State");
            if state.is_empty() {
                state = if status.starts_with("Up") {
                    "running".into()
                } else {
                    "exited".into()
                };
            }
            if status.is_empty() {
                // podman `{{json .}}` deixa Status vazio
                let mut c = state.chars();
                status = c
                    .next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default();
            }
            let name = match v.get("Names") {
                Some(Value::Arr(a)) => a
                    .iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                _ => g("Names"),
            };
            // Docker: CreatedAt é a data; Podman: Created (segundos Unix ou
            // data) e CreatedAt relativo ("2 hours ago")
            let created = match v.get("Created") {
                Some(Value::Num(n)) => n
                    .parse::<f64>()
                    .ok()
                    .filter(|x| *x > 0.0)
                    .map(|x| sys::iso_utc(x as u64 * 1000))
                    .unwrap_or_default(),
                Some(Value::Str(s)) if !s.is_empty() => s.clone(),
                _ => g("CreatedAt"),
            };
            let mut id = first_str(v, &["ID", "Id"]);
            id.truncate(12);
            Container {
                id,
                name,
                image: g("Image"),
                state,
                status,
                ports: ports_text(v.get("Ports")),
                created,
                compose: compose_project(v.get("Labels")),
            }
        })
        .filter(|c| !c.id.is_empty())
        .collect()
}

/// `docker stats --no-stream` (`ID`, `CPUPerc`, `MemUsage`) ou
/// `podman stats --no-stream --format json` (`id`, `cpu_percent`, `mem_usage`):
/// id curto → (CPU %, uso de memória em texto).
pub fn parse_docker_stats(src: &str) -> Vec<(String, f64, String)> {
    json_items(src)
        .iter()
        .filter_map(|v| {
            let id = first_str(v, &["ID", "id"]);
            if id.is_empty() {
                return None;
            }
            let cpu = first_str(v, &["CPUPerc", "cpu_percent"])
                .trim_end_matches('%')
                .parse()
                .unwrap_or(0.0);
            Some((id, cpu, first_str(v, &["MemUsage", "mem_usage"])))
        })
        .collect()
}

pub fn containers_json(with_stats: bool) -> String {
    let eng = engine();
    let label = engine_label(eng);
    let unavailable = |msg: &str| {
        Obj::new()
            .b("available", false)
            .s("engine", eng)
            .s("error", msg)
            .raw("containers", "[]")
            .finish()
    };
    let fmt = if eng == "podman" {
        "{{.Client.Version}}"
    } else {
        "{{.Server.Version}}"
    };
    let (ok, version, err) = match sys::run_cmd(eng, &["version", "--format", fmt], 5_000) {
        Ok(x) => x,
        Err(_) => return unavailable("nem o Docker nem o Podman estão instalados"),
    };
    if !ok {
        let msg = if err.to_lowercase().contains("permission denied") {
            if eng == "docker" {
                "sem permissão no socket do Docker (adicione o usuário ao grupo docker)".to_string()
            } else {
                "sem permissão para falar com o Podman".to_string()
            }
        } else {
            err.lines()
                .find(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .unwrap_or_else(|| format!("{} indisponível", label))
        };
        return unavailable(&msg);
    }
    let ps: &[&str] = if eng == "podman" {
        &["ps", "-a", "--format", "json"]
    } else {
        &["ps", "-a", "--no-trunc=false", "--format", "{{json .}}"]
    };
    let list = match sys::run_cmd(eng, ps, 10_000) {
        Ok((_, out, _)) => parse_docker_ps(&out),
        Err(_) => vec![],
    };
    let stats = if with_stats && list.iter().any(|c| c.state == "running") {
        let fmt = if eng == "podman" {
            "json"
        } else {
            "{{json .}}"
        };
        sys::run_cmd(eng, &["stats", "--no-stream", "--format", fmt], 15_000)
            .map(|(_, o, _)| parse_docker_stats(&o))
            .unwrap_or_default()
    } else {
        vec![]
    };
    Obj::new()
        .b("available", true)
        .s("engine", eng)
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
    let (ok, out, err) = sys::run_cmd(engine(), &[verb, "--", id], 60_000)?;
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
    let eng = engine();
    let (ok, out, err) = sys::run_cmd(
        eng,
        &["logs", "--tail", &n, "--timestamps", "--", id],
        15_000,
    )?;
    if !ok && out.is_empty() {
        return Err(err
            .lines()
            .next()
            .map(|l| l.to_string())
            .unwrap_or_else(|| format!("{} logs falhou", eng)));
    }
    // docker/podman logs escrevem o stderr do contêiner no stderr: junta os dois
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
    fn podman_ps_and_stats_real_output() {
        // saída real do podman 4.9.3 (Ubuntu 24.04): `ps -a --format json`
        let c = parse_docker_ps(include_str!("../testdata/podman-ps.json"));
        assert_eq!(c.len(), 2);
        let web = c.iter().find(|c| c.name == "web").unwrap();
        assert_eq!(web.id, "b0c6c16c289a");
        assert_eq!(web.state, "running");
        assert!(web.status.starts_with("Up"));
        assert_eq!(web.compose, "demo");
        assert_eq!(web.ports, "8080->80/tcp");
        assert_eq!(web.image, "docker.io/library/alpine:latest");
        assert!(web.created.starts_with("2026-"), "{}", web.created);
        let done = c.iter().find(|c| c.name == "done").unwrap();
        assert_eq!((done.state.as_str(), done.compose.as_str()), ("exited", ""));
        assert_eq!(done.ports, "");
        // `ps -a --format '{{json .}}'`: Status e CreatedAt vêm vazios
        let l = parse_docker_ps(include_str!("../testdata/podman-ps-lines.json"));
        assert_eq!(l.len(), 2);
        let web = l.iter().find(|c| c.name == "web").unwrap();
        assert_eq!(
            (web.state.as_str(), web.status.as_str()),
            ("running", "Running")
        );
        assert!(web.created.starts_with("2026-"));
        assert_eq!(web.compose, "demo");
        // `stats --no-stream --format json`
        let st = parse_docker_stats(include_str!("../testdata/podman-stats.json"));
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].0, "b0c6c16c289a");
        assert_eq!(st[0].1, 0.05);
        assert_eq!(st[0].2, "0B / 14.06GB");
        assert_eq!(
            ports_text(json::parse(r#"[{"host_ip":"127.0.0.1","container_port":5000,"host_port":5000,"range":2,"protocol":"udp"}]"#).ok().as_ref()),
            "127.0.0.1:5000-5001->5000-5001/udp"
        );
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
