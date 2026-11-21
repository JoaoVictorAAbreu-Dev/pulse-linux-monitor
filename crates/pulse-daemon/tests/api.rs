//! Testes de integração: sobem o binário `pulse` de verdade numa porta livre,
//! com HOME/XDG temporários (nunca tocam nos arquivos do usuário), e falam
//! HTTP com ele.

use pulse_core::json::{self, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_pulse");

struct Env {
    root: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let root = std::env::temp_dir().join(format!("pulse-it-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        Env { root }
    }
    fn cmd(&self) -> Command {
        let mut c = Command::new(BIN);
        c.env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        c
    }
    fn state(&self) -> PathBuf {
        self.root.join("state/pulse")
    }
    fn config(&self) -> PathBuf {
        self.root.join("config/pulse")
    }
    fn token(&self) -> String {
        std::fs::read_to_string(self.config().join("token"))
            .unwrap()
            .trim()
            .to_string()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Daemon {
    child: Child,
    port: u16,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start(env: &Env, extra: &[&str]) -> Daemon {
    let port = free_port();
    let child = env
        .cmd()
        .args(["--port", &port.to_string(), "--interval-ms", "250"])
        .args(extra)
        .spawn()
        .expect("iniciar pulse");
    let d = Daemon { child, port };
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(10) {
        if let Ok((200, _, b)) = try_req(port, "GET", "/api/health", &[], "") {
            if b.contains("\"seq\":") && !b.contains("\"seq\":0") {
                return d;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("daemon não respondeu na porta {}", port);
}

fn try_req(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> std::io::Result<(u16, String, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut h = format!("{} {} HTTP/1.1\r\n", method, path);
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        h.push_str(&format!("Host: 127.0.0.1:{}\r\n", port));
    }
    for (k, v) in headers {
        h.push_str(&format!("{}: {}\r\n", k, v));
    }
    h.push_str(&format!("Content-Length: {}\r\n\r\n{}", body.len(), body));
    s.write_all(h.as_bytes())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let out = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    Ok((status, head.to_string(), body.to_string()))
}

fn req(
    d: &Daemon,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String, String) {
    try_req(d.port, method, path, headers, body).expect("requisição")
}

fn j(s: &str) -> Value {
    json::parse(s).unwrap_or_else(|e| panic!("JSON inválido ({}): {}", e, &s[..s.len().min(200)]))
}

fn wait_for(cond: impl Fn() -> bool, secs: u64) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn auth_host_origin_and_cookie_rules() {
    let env = Env::new("auth");
    let d = start(&env, &[]);
    let tok = env.token();
    let t = tok.as_str();

    let (st, _, b) = req(&d, "GET", "/api/health", &[], "");
    assert_eq!(st, 200);
    assert!(b.contains("\"authenticated\":false"));
    assert_eq!(req(&d, "GET", "/api/snapshot", &[], "").0, 401);
    let (st, _, b) = req(&d, "GET", "/api/snapshot", &[("X-Pulse-Token", t)], "");
    assert_eq!(st, 200);
    assert!(j(&b).get("cpu").is_some());

    // DNS rebinding e CSRF
    assert_eq!(
        req(
            &d,
            "GET",
            "/api/snapshot",
            &[("Host", "evil.example:80"), ("X-Pulse-Token", t)],
            ""
        )
        .0,
        421
    );
    assert_eq!(
        req(
            &d,
            "GET",
            "/api/snapshot",
            &[("Origin", "http://evil.example"), ("X-Pulse-Token", t)],
            ""
        )
        .0,
        403
    );

    // login → cookie HttpOnly/SameSite; GET com cookie ok; POST com cookie não
    let (st, head, _) = req(&d, "POST", "/api/login", &[("X-Pulse-Token", t)], "");
    assert_eq!(st, 200);
    assert!(head.contains("HttpOnly; SameSite=Strict"));
    let cookie = format!("pulse_token={}", t);
    assert_eq!(
        req(&d, "GET", "/api/history", &[("Cookie", &cookie)], "").0,
        200
    );
    assert_eq!(
        req(
            &d,
            "POST",
            "/api/config/alerts",
            &[("Cookie", &cookie)],
            "{}"
        )
        .0,
        401
    );

    // token nunca aparece no stderr do daemon
    drop(d);
    // (stderr é verificado no teste de rotação)
}

#[test]
fn sse_and_history() {
    let env = Env::new("sse");
    let d = start(&env, &[]);
    let tok = env.token();
    let mut s = TcpStream::connect(("127.0.0.1", d.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "GET /api/stream HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Pulse-Token: {}\r\n\r\n",
        d.port, tok
    )
    .unwrap();
    let mut r = BufReader::new(s);
    let mut events = 0;
    let mut line = String::new();
    let t0 = Instant::now();
    while events < 2 && t0.elapsed() < Duration::from_secs(5) {
        line.clear();
        if r.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        if line.starts_with("data: {") && line.contains("\"cpu\"") {
            j(line.trim_start_matches("data: ").trim());
            events += 1;
        }
    }
    assert_eq!(events, 2, "duas amostras via SSE");
    let (st, _, b) = req(&d, "GET", "/api/history", &[("X-Pulse-Token", &tok)], "");
    assert_eq!(st, 200);
    assert!(j(&b)
        .get("ts")
        .and_then(|x| x.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false));
}

#[test]
fn protected_processes() {
    let env = Env::new("procs");
    let d = start(&env, &[]);
    let tok = env.token();
    let h = [("X-Pulse-Token", tok.as_str())];
    assert_eq!(req(&d, "POST", "/api/processes/1/terminate", &h, "").0, 403);
    let own = format!("/api/processes/{}/pause", d.child.id());
    assert_eq!(req(&d, "POST", &own, &h, "").0, 403);
    assert_eq!(req(&d, "POST", "/api/processes/abc/pause", &h, "").0, 400);
    let audit = std::fs::read_to_string(env.state().join("audit.log")).unwrap_or_default();
    assert!(!audit.contains(&tok), "token não vai para a auditoria");
}

#[test]
fn hooks_and_alert_config_persist_across_restart() {
    let env = Env::new("persist");
    {
        let d = start(&env, &[]);
        let tok = env.token();
        let h = [
            ("X-Pulse-Token", tok.as_str()),
            ("X-Pulse-State", "atencao"),
            ("X-Pulse-Agent", "claude"),
        ];
        let body =
            r#"{"session_id":"sess-it","hook_event_name":"Notification","prompt":"SEGREDO"}"#;
        assert_eq!(req(&d, "POST", "/api/agents/hook", &h, body).0, 200);

        let th = [("X-Pulse-Token", tok.as_str())];
        let (st, _, _) = req(
            &d,
            "POST",
            "/api/config/alerts",
            &th,
            r#"{"cpu_pct":70,"cpu_samples":5}"#,
        );
        assert_eq!(st, 200);
        let (st, _, b) = req(&d, "POST", "/api/config/alerts", &th, r#"{"mem_warn":99}"#);
        assert_eq!(st, 400, "{}", b);
        assert_eq!(
            req(&d, "POST", "/api/config/alerts", &th, r#"{"cpu_pct":"x"}"#).0,
            400
        );
    }
    let cfg = std::fs::read_to_string(env.config().join("config.toml")).unwrap();
    assert!(cfg.contains("[alerts]") && cfg.contains("cpu_pct = 70"));
    let hooks = std::fs::read_to_string(env.state().join("hooks.log")).unwrap();
    assert!(hooks.contains("sess-it") && !hooks.contains("SEGREDO"));

    let d = start(&env, &[]);
    let tok = env.token();
    let th = [("X-Pulse-Token", tok.as_str())];
    let (_, _, b) = req(&d, "GET", "/api/agents", &th, "");
    assert!(b.contains("sess-it"), "evento de hook recarregado");
    let (_, _, b) = req(&d, "GET", "/api/config", &th, "");
    let v = j(&b);
    assert_eq!(
        v.path(&["alerts", "cpu_pct"]).and_then(|x| x.as_f64()),
        Some(70.0)
    );
    assert_eq!(
        v.path(&["alerts", "cpu_samples"]).and_then(|x| x.as_f64()),
        Some(5.0)
    );
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[test]
fn long_history_is_reloaded_from_disk() {
    let env = Env::new("long");
    std::fs::create_dir_all(env.state()).unwrap();
    let base = (now_ms() / 60_000 - 10) * 60_000;
    let mut body = String::new();
    for i in 0..3 {
        body.push_str(&format!("{} 10.0 20.0 0 1 2 3 4 -\n", base + i * 60_000));
    }
    body.push_str("linha inválida\n");
    write_file(&env.state().join("history.log"), &body);
    let d = start(&env, &[]);
    let tok = env.token();
    let (st, _, b) = req(
        &d,
        "GET",
        "/api/history/long?range=1h",
        &[("X-Pulse-Token", &tok)],
        "",
    );
    assert_eq!(st, 200);
    let v = j(&b);
    assert!(v.get("count").and_then(|x| x.as_f64()).unwrap() >= 3.0);
    assert_eq!(
        req(
            &d,
            "GET",
            "/api/history/long?range=99y",
            &[("X-Pulse-Token", &tok)],
            ""
        )
        .0,
        400
    );
}

fn write_file(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

#[test]
fn config_file_is_applied_and_cli_wins() {
    let env = Env::new("cfgfile");
    write_file(
        &env.config().join("config.toml"),
        "interval_ms = 400\n[history]\nretention_days = 2\n[alerts]\ndisk_warn = 80\nchave_ruim = 1\n",
    );
    let d = start(&env, &["--interval-ms", "300"]);
    let tok = env.token();
    let (_, _, b) = req(&d, "GET", "/api/config", &[("X-Pulse-Token", &tok)], "");
    let v = j(&b);
    assert_eq!(
        v.get("interval_ms").and_then(|x| x.as_f64()),
        Some(300.0),
        "flag vence o arquivo"
    );
    assert_eq!(v.get("retention_days").and_then(|x| x.as_f64()), Some(2.0));
    assert_eq!(
        v.path(&["alerts", "disk_warn"]).and_then(|x| x.as_f64()),
        Some(80.0)
    );
    assert_eq!(
        v.get("warnings")
            .and_then(|x| x.as_array())
            .map(|a| a.len()),
        Some(1)
    );
}

#[test]
fn rotate_token_is_picked_up_without_restart() {
    let env = Env::new("rotate");
    let mut d = start(&env, &[]);
    let old = env.token();
    let out = env.cmd().arg("--rotate-token").output().unwrap();
    assert!(out.status.success());
    let new = env.token();
    assert_ne!(old, new);
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains(&new),
        "não imprime o token"
    );
    let ok = wait_for(
        || {
            try_req(
                d.port,
                "GET",
                "/api/snapshot",
                &[("X-Pulse-Token", &new)],
                "",
            )
            .map(|r| r.0)
            .ok()
                == Some(200)
        },
        8,
    );
    assert!(ok, "daemon adotou o token novo");
    assert_eq!(
        req(&d, "GET", "/api/snapshot", &[("X-Pulse-Token", &old)], "").0,
        401
    );
    // o stderr do daemon nunca contém tokens
    let _ = d.child.kill();
    let mut err = String::new();
    d.child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(!err.contains(&old) && !err.contains(&new));
}

#[test]
fn brute_force_is_blocked() {
    let env = Env::new("brute");
    let d = start(&env, &[]);
    let tok = env.token();
    for _ in 0..10 {
        let st = req(&d, "GET", "/api/snapshot", &[("X-Pulse-Token", "0000")], "").0;
        assert_eq!(st, 401);
    }
    let (st, head, _) = req(&d, "GET", "/api/snapshot", &[("X-Pulse-Token", &tok)], "");
    assert_eq!(st, 429, "bloqueado mesmo com o token certo");
    assert!(head.contains("Retry-After"));
    // a UI estática continua acessível para mostrar a mensagem
    assert_eq!(req(&d, "GET", "/", &[], "").0, 200);
    let audit = std::fs::read_to_string(env.state().join("audit.log")).unwrap();
    assert!(audit.contains("auth.block"));
}

#[test]
fn cli_once_help_and_errors() {
    let env = Env::new("cli");
    let out = env
        .cmd()
        .args(["--once", "--interval-ms", "250"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = j(String::from_utf8_lossy(&out.stdout).trim());
    assert!(v.path(&["mem", "total"]).and_then(|x| x.as_f64()).unwrap() > 0.0);

    let out = env.cmd().arg("--print-config").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("[alerts]"));

    let out = env.cmd().arg("--nao-existe").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = env.cmd().args(["--bind", "0.0.0.0"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "rede exige --allow-remote");
}

#[test]
fn phase2_routes() {
    let env = Env::new("fase2");
    let big = env.root.join("home/dados");
    std::fs::create_dir_all(big.join("sub")).unwrap();
    std::fs::write(big.join("sub/arquivo"), vec![7u8; 128 * 1024]).unwrap();
    let d = start(&env, &[]);
    let tok = env.token();
    let h = [("X-Pulse-Token", tok.as_str())];

    // snapshot traz GPU, energia e "quem está na swap"
    let (_, _, b) = req(&d, "GET", "/api/snapshot", &h, "");
    let v = j(&b);
    assert!(v.get("gpus").and_then(|x| x.as_array()).is_some());
    assert!(v.get("power").is_some());
    assert!(v
        .path(&["procs", "top_swap"])
        .and_then(|x| x.as_array())
        .is_some());

    // processos expõem E/S (o próprio daemon é legível)
    let (_, _, b) = req(&d, "GET", "/api/processes", &h, "");
    let own = j(&b)
        .get("list")
        .and_then(|l| l.as_array())
        .and_then(|l| {
            l.iter()
                .find(|p| p.get("pid").and_then(|x| x.as_f64()) == Some(d.child.id() as f64))
                .cloned()
        })
        .expect("daemon na lista");
    assert!(own.get("io_rd").and_then(|x| x.as_f64()).is_some());

    // cgroups e SMART respondem JSON mesmo sem suporte/permissão
    let (st, _, b) = req(&d, "GET", "/api/cgroups", &h, "");
    assert_eq!(st, 200);
    assert!(j(&b).get("available").is_some());
    let (st, _, b) = req(&d, "GET", "/api/disk/smart", &h, "");
    assert_eq!(st, 200);
    assert!(j(&b).get("available").is_some());

    // logs de contêiner validam o identificador
    assert_eq!(
        req(&d, "GET", "/api/containers/logs?id=--help", &h, "").0,
        400
    );

    // du em segundo plano: pending → resultado → cache
    let path = format!("/api/disk/usage?path={}", big.to_string_lossy());
    let (_, _, b) = req(&d, "GET", &path, &h, "");
    assert!(b.contains("\"pending\":true"));
    let done = wait_for(
        || {
            try_req(d.port, "GET", &path, &h, "")
                .map(|r| r.2.contains("\"entries\""))
                .unwrap_or(false)
        },
        10,
    );
    assert!(done, "varredura terminou");
    let (_, _, b) = req(&d, "GET", &path, &h, "");
    let v = j(&b);
    assert!(v.get("total").and_then(|x| x.as_f64()).unwrap() >= 128.0 * 1024.0);
    assert!(v.get("cached_s").is_some(), "segunda leitura vem do cache");

    // faixas longas usam a camada de 15 min
    let (_, _, b) = req(&d, "GET", "/api/history/long?range=90d", &h, "");
    assert_eq!(
        j(&b).get("bucket_ms").and_then(|x| x.as_f64()),
        Some(900_000.0)
    );
    let (_, _, b) = req(&d, "GET", "/api/history/long?range=7d", &h, "");
    assert_eq!(
        j(&b).get("bucket_ms").and_then(|x| x.as_f64()),
        Some(60_000.0)
    );
}

#[test]
fn phase3_agents_prefs_and_hooks() {
    let env = Env::new("fase3");
    // transcrição de exemplo no HOME temporário
    let proj = env.root.join("home/.claude/projects/-tmp-x");
    std::fs::create_dir_all(&proj).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let iso = |s: u64| {
        let out = Command::new("date")
            .args(["-u", "-d", &format!("@{}", s), "+%Y-%m-%dT%H:%M:%SZ"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let lines = [
        format!(
            r#"{{"type":"user","sessionId":"s1","cwd":"/tmp/x","timestamp":"{}","message":{{"content":"SEGREDO"}}}}"#,
            iso(now - 60)
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"m1","model":"claude-teste-1","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"ls --token=abc"}}}}],"usage":{{"input_tokens":1000000,"output_tokens":1000000}}}}}}"#,
            iso(now - 50)
        ),
        format!(
            r#"{{"type":"user","timestamp":"{}","message":{{"content":[{{"type":"tool_result","tool_use_id":"t1"}}]}}}}"#,
            iso(now - 48)
        ),
    ];
    std::fs::write(proj.join("s1.jsonl"), lines.join("\n")).unwrap();
    let d = start(&env, &[]);
    let tok = env.token();
    let h = [("X-Pulse-Token", tok.as_str())];

    // índice de uso e sessão com latência de ferramenta
    let ok = wait_for(
        || {
            try_req(d.port, "GET", "/api/agents", &h, "")
                .map(|r| r.2.contains("claude-teste-1") && r.2.contains("\"rows\":[{"))
                .unwrap_or(false)
        },
        20,
    );
    assert!(ok, "sessão e índice de uso aparecem");
    let (_, _, b) = req(&d, "GET", "/api/agents", &h, "");
    assert!(
        !b.contains("SEGREDO") && !b.contains("abc"),
        "sem conteúdo nem detalhes por padrão"
    );
    let v = j(&b);
    let s = &v.get("sessions").and_then(|x| x.as_array()).unwrap()[0];
    let bash = &s.get("tools").and_then(|x| x.as_array()).unwrap()[0];
    assert_eq!(bash.get("avg_ms").and_then(|x| x.as_f64()), Some(2000.0));

    // preferências: preços e modo detalhado gravados no config.toml
    let body = r#"{"show_tool_details":true,"notify_attention":false,"prices":{"claude-teste":[3,15,0.3,3.75]}}"#;
    assert_eq!(req(&d, "POST", "/api/config/agents", &h, body).0, 200);
    assert_eq!(
        req(
            &d,
            "POST",
            "/api/config/agents",
            &h,
            r#"{"prices":{"a b":[1,1,1,1]}}"#
        )
        .0,
        400
    );
    assert_eq!(
        req(
            &d,
            "POST",
            "/api/config/agents",
            &h,
            r#"{"notify_attention":"sim"}"#
        )
        .0,
        400
    );
    let cfg = std::fs::read_to_string(env.config().join("config.toml")).unwrap();
    assert!(
        cfg.contains("\"claude-teste\" = \"3, 15, 0.3, 3.75\"")
            && cfg.contains("show_tool_details = true")
    );

    // hooks: subagentes/falhas e detalhe mascarado (o aviso de atenção está desligado acima)
    let hh = |st: &'static str| {
        [
            ("X-Pulse-Token", tok.as_str()),
            ("X-Pulse-State", st),
            ("X-Pulse-Agent", "claude"),
        ]
    };
    for (ev, st) in [
        ("SubagentStart", "executando"),
        ("PostToolUseFailure", "executando"),
        ("PermissionRequest", "atencao"),
    ] {
        let b = format!(
            r#"{{"session_id":"hk","hook_event_name":"{}","tool_name":"Bash","tool_input":{{"command":"rm x --password p4ss"}}}}"#,
            ev
        );
        assert_eq!(req(&d, "POST", "/api/agents/hook", &hh(st), &b).0, 200);
    }
    let (_, _, b) = req(&d, "GET", "/api/agents", &h, "");
    assert!(b.contains("\"subagents\":1") && b.contains("\"failures\":1"));
    assert!(b.contains("rm x --password ***") && !b.contains("p4ss"));
}

#[test]
fn phase4_metrics_pwa_and_notify_channels() {
    let env = Env::new("fase4");
    let d = start(&env, &[]);
    let tok = env.token();

    // /metrics: Bearer (Prometheus), formato de texto, exige token
    assert_eq!(req(&d, "GET", "/metrics", &[], "").0, 401);
    let bearer = format!("Bearer {}", tok);
    let (st, head, body) = req(&d, "GET", "/metrics", &[("Authorization", &bearer)], "");
    assert_eq!(st, 200);
    assert!(head.contains("text/plain; version=0.0.4"));
    assert!(body.contains("# TYPE pulse_cpu_usage_percent gauge") && body.contains("pulse_up 1"));
    assert_eq!(
        req(&d, "GET", "/metrics", &[("Authorization", "Bearer 00")], "").0,
        401
    );

    // PWA: manifesto, service worker e ícones são públicos
    let (st, _, m) = req(&d, "GET", "/manifest.webmanifest", &[], "");
    assert_eq!(st, 200);
    assert!(j(&m).get("icons").is_some());
    assert_eq!(req(&d, "GET", "/sw.js", &[], "").0, 200);
    assert_eq!(req(&d, "GET", "/icon-192.png", &[], "").0, 200);

    // canais: um receptor HTTP local recebe ntfy e webhook
    let rx = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = rx.local_addr().unwrap().port();
    let got = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let g2 = got.clone();
    std::thread::spawn(move || {
        for s in rx.incoming().flatten() {
            let mut s = s;
            s.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut buf = vec![0u8; 65536];
            let mut data = Vec::new();
            loop {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        data.extend_from_slice(&buf[..n]);
                        let t = String::from_utf8_lossy(&data);
                        if let Some(i) = t.find("\r\n\r\n") {
                            let len = t[..i]
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if data.len() >= i + 4 + len {
                                break;
                            }
                        }
                    }
                }
            }
            g2.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&data).into_owned());
            let _ =
                s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    let h = [("X-Pulse-Token", tok.as_str())];
    let cfg = format!(
        r#"{{"ntfy_url":"http://127.0.0.1:{p}/topico","ntfy_token":"tk_SEGREDO","webhook_url":"http://127.0.0.1:{p}/hook"}}"#,
        p = port
    );
    let (st, _, b) = req(&d, "POST", "/api/config/notify", &h, &cfg);
    assert_eq!(st, 200, "{}", b);
    assert!(!b.contains("tk_SEGREDO"), "segredo não volta na resposta");
    assert_eq!(
        req(
            &d,
            "POST",
            "/api/config/notify",
            &h,
            r#"{"webhook_url":"file:///etc/passwd"}"#
        )
        .0,
        400
    );
    let (_, _, b) = req(&d, "GET", "/api/config/notify", &h, "");
    assert!(!b.contains("tk_SEGREDO") && !b.contains("/topico"));
    let (st, _, b) = req(&d, "POST", "/api/notify/test", &h, "{}");
    assert_eq!(st, 200);
    assert_eq!(
        j(&b)
            .get("results")
            .and_then(|r| r.as_array())
            .map(|a| a.len()),
        Some(2),
        "{}",
        b
    );
    assert!(
        wait_for(|| got.lock().unwrap().len() >= 2, 10),
        "dois envios recebidos"
    );
    let all = got.lock().unwrap().join("\n---\n");
    assert!(all.contains("POST /topico") && all.contains("Authorization: Bearer tk_SEGREDO"));
    assert!(
        all.contains("Title: =?UTF-8?B?"),
        "título com acento em RFC 2047"
    );
    assert!(all.contains("POST /hook") && all.contains("\"source\":\"pulse\""));
    let cfgfile = std::fs::read_to_string(env.config().join("config.toml")).unwrap();
    assert!(cfgfile.contains("[notify]") && cfgfile.contains("tk_SEGREDO"));
    let audit = std::fs::read_to_string(env.state().join("audit.log")).unwrap();
    assert!(!audit.contains("tk_SEGREDO"), "auditoria sem segredos");
}
