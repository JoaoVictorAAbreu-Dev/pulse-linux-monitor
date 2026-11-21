//! Roteamento, autenticação por requisição e handlers da API.

use crate::auth;
use crate::http::{Request, Response};
use crate::state::{procs_json, Shared};
use pulse_agents::hooks;
use pulse_core::json::{self, Obj, Value};
use pulse_core::services::{self, LogQuery, Scope};
use pulse_core::{disk, procs, schedule, sys};
use std::sync::Arc;

pub const INDEX_HTML: &str = include_str!("ui/index.html");
pub const APP_CSS: &str = include_str!("ui/app.css");
pub const APP_JS: &str = include_str!("ui/app.js");
pub const FAVICON: &str = include_str!("ui/favicon.svg");

#[derive(Debug, PartialEq)]
pub enum Auth {
    None,
    Cookie,
    Header,
}

pub fn authenticate(sh: &Shared, req: &Request) -> Auth {
    if let Some(h) = req.header("x-pulse-token") {
        if auth::ct_eq(h.trim().as_bytes(), sh.token.as_bytes()) {
            return Auth::Header;
        }
    }
    if let Some(c) = auth::cookie_value(req.header("cookie"), auth::COOKIE) {
        if auth::ct_eq(c.as_bytes(), sh.token.as_bytes()) {
            return Auth::Cookie;
        }
    }
    Auth::None
}

/// Resultado do roteamento: resposta comum ou fluxo SSE.
pub enum Outcome {
    Resp(Response),
    Stream,
}

fn body_json(req: &Request) -> Result<Value, Response> {
    let s = std::str::from_utf8(&req.body).map_err(|_| Response::error(400, "corpo não é UTF-8"))?;
    if s.trim().is_empty() {
        return Ok(Value::Obj(vec![]));
    }
    json::parse(s).map_err(|e| Response::error(400, &format!("JSON inválido: {}", e)))
}

fn str_field<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

pub fn handle(sh: &Arc<Shared>, req: &Request, peer: &str) -> Outcome {
    // 1) Host e Origin (DNS rebinding e CSRF) valem para tudo.
    if !auth::host_ok(req.header("host"), &sh.allowed_hosts) {
        return Outcome::Resp(Response::error(421, "Host não permitido"));
    }
    if !auth::origin_ok(req.header("origin"), &sh.allowed_hosts) {
        return Outcome::Resp(Response::error(403, "Origin não permitida"));
    }
    let a = authenticate(sh, req);
    let m = req.method.as_str();
    let p = req.path.as_str();

    // 2) Recursos públicos: a UI estática e a saúde mínima.
    match (m, p) {
        ("GET", "/") | ("GET", "/index.html") => {
            return Outcome::Resp(Response::new(200, "text/html; charset=utf-8", INDEX_HTML))
        }
        ("GET", "/app.css") => return Outcome::Resp(Response::new(200, "text/css; charset=utf-8", APP_CSS)),
        ("GET", "/app.js") => {
            return Outcome::Resp(Response::new(200, "text/javascript; charset=utf-8", APP_JS))
        }
        ("GET", "/favicon.svg") => return Outcome::Resp(Response::new(200, "image/svg+xml", FAVICON)),
        ("GET", "/api/health") => {
            let st = sh.lock();
            let body = Obj::new()
                .b("ok", true)
                .s("version", env!("CARGO_PKG_VERSION"))
                .b("authenticated", a != Auth::None)
                .u("seq", st.seq)
                .n("uptime_s", st.started.elapsed().as_secs_f64())
                .u("history_points", st.ring.len() as u64)
                .u("long_points", st.long.len() as u64)
                .opt_s("long_error", st.long.last_error.as_deref())
                .finish();
            return Outcome::Resp(Response::ok_json(body));
        }
        ("POST", "/api/login") => {
            return Outcome::Resp(if a == Auth::Header {
                Response::ok_json("{\"ok\":true}").with_header("Set-Cookie", &auth::set_cookie(&sh.token))
            } else {
                Response::error(401, "token inválido")
            });
        }
        ("POST", "/api/logout") => {
            return Outcome::Resp(Response::ok_json("{\"ok\":true}").with_header("Set-Cookie", &auth::clear_cookie()))
        }
        _ => {}
    }

    // 3) Demais rotas: GET aceita cookie ou cabeçalho; POST exige o cabeçalho.
    match m {
        "GET" if a == Auth::None => return Outcome::Resp(Response::error(401, "autenticação necessária")),
        "POST" if a != Auth::Header => {
            return Outcome::Resp(Response::error(401, "POST exige o cabeçalho X-Pulse-Token"))
        }
        "GET" | "POST" => {}
        _ => return Outcome::Resp(Response::error(405, "método não permitido")),
    }

    let r = match (m, p) {
        ("GET", "/api/stream") => return Outcome::Stream,
        ("GET", "/api/snapshot") => Response::ok_json(sh.lock().snapshot.as_bytes().to_vec()),
        ("GET", "/api/history") => Response::ok_json(sh.lock().ring.to_json()),
        ("GET", "/api/history/long") => {
            let range_ms: u64 = match req.q("range").unwrap_or("24h") {
                "1h" => 3600_000,
                "6h" => 6 * 3600_000,
                "24h" => 24 * 3600_000,
                "7d" => 7 * 24 * 3600_000,
                _ => return Outcome::Resp(Response::error(400, "range deve ser 1h, 6h, 24h ou 7d")),
            };
            let since = sys::now_ms().saturating_sub(range_ms);
            Response::ok_json(sh.lock().long.to_json(since))
        }
        ("GET", "/api/processes") => {
            let list = sh.lock().procs.clone();
            Response::ok_json(procs_json(&list))
        }
        ("GET", "/api/alerts") => {
            let st = sh.lock();
            Response::ok_json(
                Obj::new()
                    .raw("active", &st.alerts.active_json())
                    .raw("history", &st.alerts.history_json())
                    .raw("rules", &st.alerts.rules.to_json())
                    .b("notify", sh.cfg.notify)
                    .finish(),
            )
        }
        ("GET", "/api/services") => {
            let scope = Scope::parse(req.q("scope").unwrap_or("user")).unwrap_or(Scope::User);
            match services::list(scope) {
                Ok(u) => Response::ok_json(
                    Obj::new().s("scope", scope.name()).raw("units", &services::list_json(&u)).finish(),
                ),
                Err(e) => Response::error(502, &e),
            }
        }
        ("POST", "/api/services/action") => match body_json(req) {
            Err(r) => r,
            Ok(v) => {
                let scope = str_field(&v, "scope").and_then(Scope::parse).unwrap_or(Scope::User);
                let unit = str_field(&v, "unit").unwrap_or("");
                let verb = str_field(&v, "action").unwrap_or("");
                let res = services::action(scope, verb, unit);
                let target = format!("{}:{}", scope.name(), unit);
                auth::audit(&format!("service.{}", verb), &target, res.as_ref().map(|_| "ok").unwrap_or_else(|e| e), peer);
                match res {
                    Ok(_) => Response::ok_json("{\"ok\":true}"),
                    Err(e) => Response::error(502, &e),
                }
            }
        },
        ("GET", "/api/timers") => {
            let scope = Scope::parse(req.q("scope").unwrap_or("user")).unwrap_or(Scope::User);
            match schedule::timers(scope) {
                Ok(t) => Response::ok_json(Obj::new().s("scope", scope.name()).raw("timers", &schedule::timers_json(&t)).finish()),
                Err(e) => Response::error(502, &e),
            }
        }
        ("GET", "/api/cron") => Response::ok_json(schedule::cron_json()),
        ("GET", "/api/containers") => Response::ok_json(schedule::containers_json(req.q("stats") == Some("1"))),
        ("POST", "/api/containers/action") => match body_json(req) {
            Err(r) => r,
            Ok(v) => {
                let id = str_field(&v, "id").unwrap_or("");
                let verb = str_field(&v, "action").unwrap_or("");
                let res = schedule::container_action(verb, id);
                auth::audit(&format!("container.{}", verb), id, res.as_ref().map(|_| "ok").unwrap_or_else(|e| e), peer);
                match res {
                    Ok(_) => Response::ok_json("{\"ok\":true}"),
                    Err(e) => Response::error(502, &e),
                }
            }
        },
        ("GET", "/api/logs") => {
            let q = LogQuery {
                scope: Scope::parse(req.q("scope").unwrap_or("system")).unwrap_or(Scope::System),
                unit: req.q("unit").map(|s| s.to_string()),
                priority: req.q("priority").and_then(|s| s.parse().ok()),
                lines: req.q("lines").and_then(|s| s.parse().ok()).unwrap_or(200),
                grep: req.q("q").map(|s| s.to_string()),
                after_cursor: req.q("cursor").map(|s| s.to_string()),
            };
            match services::query_logs(&q) {
                Ok(j) => Response::ok_json(j),
                Err(e) => Response::error(400, &e),
            }
        }
        ("GET", "/api/disk/usage") => {
            let home = sys::home().to_string_lossy().into_owned();
            let path = req.q("path").unwrap_or(&home);
            match disk::du(path, 400_000, 4_000) {
                Ok(r) => Response::ok_json(r.to_json()),
                Err(e) => Response::error(400, &e),
            }
        }
        ("GET", "/api/disk/cleanup") => Response::ok_json(disk::cleanup_candidates()),
        ("GET", "/api/agents") => agents_json(sh),
        ("GET", "/api/agents/hooks") => hooks_info(),
        ("POST", "/api/agents/hooks/install") => {
            let settings = sys::home().join(".claude/settings.json");
            let script = hook_script_path();
            let res = hooks::install_claude(&settings, &script);
            auth::audit(
                "hooks.install",
                &settings.to_string_lossy(),
                res.as_ref().map(|_| "ok").unwrap_or_else(|e| e),
                peer,
            );
            match res {
                Ok((added, backup)) => Response::ok_json(
                    Obj::new()
                        .b("ok", true)
                        .raw("added", &json::arr(added.iter().map(|s| json::quote(s))))
                        .opt_s("backup", backup.as_deref())
                        .s("script", &script)
                        .b("script_exists", std::path::Path::new(&script).exists())
                        .finish(),
                ),
                Err(e) => Response::error(409, &e),
            }
        }
        ("POST", "/api/agents/hook") => {
            let agent = req.header("x-pulse-agent").unwrap_or("claude").to_string();
            let state = req.header("x-pulse-state").unwrap_or("").to_string();
            let payload = String::from_utf8_lossy(&req.body).into_owned();
            let mut st = sh.lock();
            match st.hooks.ingest(sys::now_ms(), &agent, &state, &payload) {
                Ok(_) => {
                    drop(st);
                    sh.tick.notify_all();
                    Response::ok_json("{\"ok\":true}")
                }
                Err(e) => Response::error(400, &e),
            }
        }
        ("POST", p) if p.starts_with("/api/processes/") => process_action(sh, req, p, peer),
        _ => Response::error(404, "rota não encontrada"),
    };
    Outcome::Resp(r)
}

/// Caminho do script de hook: ao lado do binário ou em ~/.local/bin.
pub fn hook_script_path() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("pulse-hook");
            if p.exists() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    sys::home().join(".local/bin/pulse-hook").to_string_lossy().into_owned()
}

fn hooks_info() -> Response {
    let settings = sys::home().join(".claude/settings.json");
    let script = hook_script_path();
    let connected = hooks::claude_status(&settings);
    Response::ok_json(
        Obj::new()
            .s("settings", &settings.to_string_lossy())
            .s("script", &script)
            .b("script_exists", std::path::Path::new(&script).exists())
            .raw("connected", &json::arr(connected.iter().map(|s| json::quote(s))))
            .u("total_events", hooks::CLAUDE_EVENTS.len() as u64)
            .s("snippet", &hooks::claude_snippet(&script))
            .s("codex_snippet", &hooks::codex_snippet(&script))
            .b("codex_connected", hooks::codex_status(&sys::home().join(".codex/config.toml")))
            .finish(),
    )
}

fn agents_json(sh: &Shared) -> Response {
    if !sh.cfg.agents {
        return Response::ok_json("{\"enabled\":false,\"processes\":[],\"sessions\":[],\"hooks\":{\"sessions\":[],\"events\":[]}}");
    }
    let st = sh.lock();
    Response::ok_json(
        Obj::new()
            .b("enabled", true)
            .raw("processes", &st.agents_json)
            .raw("sessions", &st.sessions_json)
            .raw("hooks", &st.hooks.to_json())
            .finish(),
    )
}

fn process_action(_sh: &Arc<Shared>, req: &Request, p: &str, peer: &str) -> Response {
    // /api/processes/{pid}/{acao}
    let parts: Vec<&str> = p.trim_start_matches("/api/processes/").split('/').collect();
    if parts.len() != 2 {
        return Response::error(404, "rota não encontrada");
    }
    let Ok(pid) = parts[0].parse::<i32>() else { return Response::error(400, "PID inválido") };
    let action = parts[1];
    if pid <= 1 {
        return Response::error(403, "PID protegido");
    }
    if pid == sys::self_pid() {
        return Response::error(403, "o Pulse não age sobre si mesmo");
    }
    let Some(start) = procs::start_ticks(pid) else { return Response::error(404, "processo não encontrado") };
    let name = sys::read_trim(&format!("/proc/{}/comm", pid)).unwrap_or_default();
    let target = format!("pid={} nome={}", pid, name);
    let res: Result<String, String> = match action {
        "terminate" => sys::send_signal(pid, sys::SIGTERM).map(|_| {
            // Escala para SIGKILL se ainda estiver vivo após 5 s (mesmo processo).
            let peer = peer.to_string();
            let target = target.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                let alive = procs::start_ticks(pid) == Some(start)
                    && sys::read(&format!("/proc/{}/stat", pid))
                        .and_then(|s| procs::parse_pid_stat(&s))
                        .map(|f| f.state != 'Z')
                        .unwrap_or(false);
                if alive {
                    let r = sys::send_signal(pid, sys::SIGKILL);
                    auth::audit("process.kill", &target, r.as_ref().map(|_| "ok").unwrap_or_else(|e| e), &peer);
                }
            });
            "SIGTERM enviado; SIGKILL em 5 s se não encerrar".into()
        }),
        "kill" => sys::send_signal(pid, sys::SIGKILL).map(|_| "SIGKILL enviado".into()),
        "pause" => sys::send_signal(pid, sys::SIGSTOP).map(|_| "processo pausado".into()),
        "resume" => sys::send_signal(pid, sys::SIGCONT).map(|_| "processo retomado".into()),
        "renice" => {
            let nice = match body_json(req) {
                Ok(v) => v.get("nice").and_then(|x| x.as_f64()),
                Err(r) => return r,
            };
            match nice {
                Some(n) if (-20.0..=19.0).contains(&n) => {
                    sys::renice(pid, n as i32).map(|_| format!("prioridade ajustada para {}", n as i32))
                }
                _ => Err("nice deve estar entre -20 e 19".into()),
            }
        }
        _ => return Response::error(404, "ação desconhecida"),
    };
    auth::audit(&format!("process.{}", action), &target, res.as_ref().map(|s| s.as_str()).unwrap_or_else(|e| e), peer);
    match res {
        Ok(msg) => Response::ok_json(Obj::new().b("ok", true).s("message", &msg).finish()),
        Err(e) => Response::error(if e.contains("ermission") || e.contains("permitida") { 403 } else { 400 }, &e),
    }
}
