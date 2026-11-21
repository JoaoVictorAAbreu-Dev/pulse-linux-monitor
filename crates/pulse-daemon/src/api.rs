//! Roteamento, autenticação por requisição e handlers da API.

use crate::auth;
use crate::http::{Request, Response};
use crate::settings;
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

/// Resultado da autenticação e se alguma credencial (errada) foi enviada.
pub fn authenticate(sh: &Shared, req: &Request) -> (Auth, bool) {
    let token = sh.token();
    let mut presented = false;
    if let Some(h) = req.header("x-pulse-token") {
        presented = true;
        if auth::ct_eq(h.trim().as_bytes(), token.as_bytes()) {
            return (Auth::Header, true);
        }
    }
    if let Some(c) = auth::cookie_value(req.header("cookie"), auth::COOKIE) {
        if !c.is_empty() {
            presented = true;
            if auth::ct_eq(c.as_bytes(), token.as_bytes()) {
                return (Auth::Cookie, true);
            }
        }
    }
    (Auth::None, presented)
}

/// Resultado do roteamento: resposta comum ou fluxo SSE.
pub enum Outcome {
    Resp(Response),
    Stream,
}

fn body_json(req: &Request) -> Result<Value, Response> {
    let s =
        std::str::from_utf8(&req.body).map_err(|_| Response::error(400, "corpo não é UTF-8"))?;
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
    let m = req.method.as_str();
    let p = req.path.as_str();
    let is_public = matches!(
        (m, p),
        ("GET", "/")
            | ("GET", "/index.html")
            | ("GET", "/app.css")
            | ("GET", "/app.js")
            | ("GET", "/favicon.svg")
            | ("POST", "/api/logout")
    );
    // Bloqueio por força bruta: IP com muitas falhas recentes não autentica.
    if !is_public {
        if let Some(secs) = sh.fails.blocked(peer) {
            return Outcome::Resp(
                Response::error(
                    429,
                    &format!(
                        "muitas tentativas com token inválido; tente de novo em {} s",
                        secs
                    ),
                )
                .with_header("Retry-After", &secs.to_string()),
            );
        }
    }
    let (a, presented) = authenticate(sh, req);
    if a != Auth::None {
        sh.fails.success(peer);
    } else if presented && !is_public {
        // Atraso progressivo (até 1,5 s) e bloqueio após MAX_FAILS.
        let n = sh.fails.count(peer);
        std::thread::sleep(std::time::Duration::from_millis((n as u64 * 150).min(1500)));
        if sh.fails.fail(peer) {
            auth::audit("auth.block", peer, "IP bloqueado por falhas de token", peer);
        }
    }

    // 2) Recursos públicos: a UI estática e a saúde mínima.
    match (m, p) {
        ("GET", "/") | ("GET", "/index.html") => {
            return Outcome::Resp(Response::new(200, "text/html; charset=utf-8", INDEX_HTML))
        }
        ("GET", "/app.css") => {
            return Outcome::Resp(Response::new(200, "text/css; charset=utf-8", APP_CSS))
        }
        ("GET", "/app.js") => {
            return Outcome::Resp(Response::new(200, "text/javascript; charset=utf-8", APP_JS))
        }
        ("GET", "/favicon.svg") => {
            return Outcome::Resp(Response::new(200, "image/svg+xml", FAVICON))
        }
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
                .opt_s(
                    "log_error",
                    st.hook_log
                        .last_error
                        .as_deref()
                        .or(st.alert_log.last_error.as_deref()),
                )
                .u("config_warnings", sh.config_warnings.len() as u64)
                .finish();
            return Outcome::Resp(Response::ok_json(body));
        }
        ("POST", "/api/login") => {
            return Outcome::Resp(if a == Auth::Header {
                Response::ok_json("{\"ok\":true}")
                    .with_header("Set-Cookie", &auth::set_cookie(&sh.token()))
            } else {
                Response::error(401, "token inválido")
            });
        }
        ("POST", "/api/logout") => {
            return Outcome::Resp(
                Response::ok_json("{\"ok\":true}").with_header("Set-Cookie", &auth::clear_cookie()),
            )
        }
        _ => {}
    }

    // 3) Demais rotas: GET aceita cookie ou cabeçalho; POST exige o cabeçalho.
    match m {
        "GET" if a == Auth::None => {
            return Outcome::Resp(Response::error(401, "autenticação necessária"))
        }
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
                "1h" => 3_600_000,
                "6h" => 6 * 3_600_000,
                "24h" => 24 * 3_600_000,
                "7d" => 7 * 24 * 3_600_000,
                "30d" => 30 * 24 * 3_600_000,
                "90d" => 90 * 24 * 3_600_000,
                _ => {
                    return Outcome::Resp(Response::error(
                        400,
                        "range deve ser 1h, 6h, 24h, 7d, 30d ou 90d",
                    ))
                }
            };
            let since = sys::now_ms().saturating_sub(range_ms);
            let st = sh.lock();
            // até 7 dias: médias de 1 min; acima: camada de 15 min (90 dias)
            Response::ok_json(if range_ms > 7 * 24 * 3_600_000 {
                st.long90.to_json(since)
            } else {
                st.long.to_json(since)
            })
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
                    .raw("values", &settings::rules_json(&st.alerts.rules))
                    .b("notify", sh.cfg.notify)
                    .finish(),
            )
        }
        ("GET", "/api/config") => {
            let st = sh.lock();
            Response::ok_json(
                Obj::new()
                    .s("path", &sh.config_path.to_string_lossy())
                    .b("exists", sh.config_path.exists())
                    .raw(
                        "warnings",
                        &json::arr(sh.config_warnings.iter().map(|w| json::quote(w))),
                    )
                    .raw("alerts", &settings::rules_json(&st.alerts.rules))
                    .u("port", sh.cfg.port as u64)
                    .u("interval_ms", sh.cfg.interval_ms)
                    .u("retention_days", sh.cfg.retention_days)
                    .b("notify", sh.cfg.notify)
                    .b("agents", sh.cfg.agents)
                    .finish(),
            )
        }
        ("POST", "/api/config/alerts") => match body_json(req) {
            Err(r) => r,
            Ok(v) => {
                let mut kv = std::collections::BTreeMap::new();
                if let Value::Obj(pairs) = &v {
                    for (k, val) in pairs {
                        match val.as_f64() {
                            Some(x) => {
                                kv.insert(k.clone(), x);
                            }
                            None => {
                                return Outcome::Resp(Response::error(
                                    400,
                                    &format!("{} precisa ser número", k),
                                ))
                            }
                        }
                    }
                }
                let current = sh.lock().alerts.rules.clone();
                match settings::apply_alerts(&current, &kv) {
                    Err(e) => Response::error(400, &e),
                    Ok(rules) => {
                        let src = std::fs::read_to_string(&sh.config_path).unwrap_or_default();
                        let body = settings::rewrite_alerts(&src, &rules);
                        let res = settings::write_atomic(&sh.config_path, &body);
                        auth::audit(
                            "config.alerts",
                            &sh.config_path.to_string_lossy(),
                            res.as_ref().map(|_| "ok").unwrap_or_else(|e| e),
                            peer,
                        );
                        match res {
                            Ok(()) => {
                                sh.lock().alerts.rules = rules.clone();
                                Response::ok_json(
                                    Obj::new()
                                        .b("ok", true)
                                        .raw("alerts", &settings::rules_json(&rules))
                                        .finish(),
                                )
                            }
                            Err(e) => Response::error(
                                500,
                                &format!("não foi possível gravar a configuração: {}", e),
                            ),
                        }
                    }
                }
            }
        },
        ("GET", "/api/services") => {
            let scope = Scope::parse(req.q("scope").unwrap_or("user")).unwrap_or(Scope::User);
            match services::list(scope) {
                Ok(u) => Response::ok_json(
                    Obj::new()
                        .s("scope", scope.name())
                        .raw("units", &services::list_json(&u))
                        .finish(),
                ),
                Err(e) => Response::error(502, &e),
            }
        }
        ("POST", "/api/services/action") => match body_json(req) {
            Err(r) => r,
            Ok(v) => {
                let scope = str_field(&v, "scope")
                    .and_then(Scope::parse)
                    .unwrap_or(Scope::User);
                let unit = str_field(&v, "unit").unwrap_or("");
                let verb = str_field(&v, "action").unwrap_or("");
                let res = services::action(scope, verb, unit);
                let target = format!("{}:{}", scope.name(), unit);
                auth::audit(
                    &format!("service.{}", verb),
                    &target,
                    res.as_ref().map(|_| "ok").unwrap_or_else(|e| e),
                    peer,
                );
                match res {
                    Ok(_) => Response::ok_json("{\"ok\":true}"),
                    Err(e) => Response::error(502, &e),
                }
            }
        },
        ("GET", "/api/timers") => {
            let scope = Scope::parse(req.q("scope").unwrap_or("user")).unwrap_or(Scope::User);
            match schedule::timers(scope) {
                Ok(t) => Response::ok_json(
                    Obj::new()
                        .s("scope", scope.name())
                        .raw("timers", &schedule::timers_json(&t))
                        .finish(),
                ),
                Err(e) => Response::error(502, &e),
            }
        }
        ("GET", "/api/cron") => Response::ok_json(schedule::cron_json()),
        ("GET", "/api/containers") => {
            Response::ok_json(schedule::containers_json(req.q("stats") == Some("1")))
        }
        ("POST", "/api/containers/action") => match body_json(req) {
            Err(r) => r,
            Ok(v) => {
                let id = str_field(&v, "id").unwrap_or("");
                let verb = str_field(&v, "action").unwrap_or("");
                let res = schedule::container_action(verb, id);
                auth::audit(
                    &format!("container.{}", verb),
                    id,
                    res.as_ref().map(|_| "ok").unwrap_or_else(|e| e),
                    peer,
                );
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
        ("GET", "/api/disk/usage") => du_route(sh, req),
        ("GET", "/api/disk/smart") => {
            let devs: Vec<String> = disk::read_devices().into_iter().map(|d| d.0).collect();
            Response::ok_json(pulse_core::smart::smart_json(&devs))
        }
        ("GET", "/api/cgroups") => {
            let mut cg = sh.cgroups.lock().unwrap_or_else(|e| e.into_inner());
            if !cg.available() {
                Response::ok_json("{\"available\":false,\"units\":[]}")
            } else {
                let u = cg.sample();
                Response::ok_json(
                    Obj::new()
                        .b("available", true)
                        .raw("units", &pulse_core::cgroups::usage_json(&u))
                        .finish(),
                )
            }
        }
        ("GET", "/api/containers/logs") => {
            let id = req.q("id").unwrap_or("");
            let lines = req
                .q("lines")
                .and_then(|x| x.parse::<u32>().ok())
                .unwrap_or(200)
                .clamp(10, 2000);
            match schedule::container_logs(id, lines) {
                Ok(t) => Response::ok_json(Obj::new().s("id", id).s("text", &t).finish()),
                Err(e) => Response::error(400, &e),
            }
        }
        ("GET", "/api/disk/cleanup") => Response::ok_json(disk::cleanup_candidates()),
        ("GET", "/api/agents") => agents_json(sh),
        ("GET", "/api/agents/hooks") => hooks_info(),
        ("POST", "/api/config/agents") => match body_json(req) {
            Err(r) => r,
            Ok(v) => config_agents(sh, &v, peer),
        },
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
            let prefs = sh.prefs.read().map(|p| p.clone()).unwrap_or_default();
            let mut st = sh.lock();
            match st.hooks.ingest(
                sys::now_ms(),
                &agent,
                &state,
                &payload,
                prefs.show_tool_details,
            ) {
                Ok(e) => {
                    st.hook_log.append(&e.to_json());
                    drop(st);
                    sh.tick.notify_all();
                    if e.state == "atencao" && prefs.notify_attention {
                        notify_attention(sh, &e);
                    }
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

/// Explorador de espaço: varredura em thread própria com cache de 5 min.
/// Responde na hora com o resultado em cache ou com `pending` + progresso.
fn du_route(sh: &Arc<Shared>, req: &Request) -> Response {
    use std::sync::atomic::{AtomicU64, Ordering};
    let home = sys::home().to_string_lossy().into_owned();
    let raw = req.q("path").filter(|p| !p.is_empty()).unwrap_or(&home);
    let key = match std::path::Path::new(raw).canonicalize() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(e) => return Response::error(400, &e.to_string()),
    };
    let refresh = req.q("refresh") == Some("1");
    let mut jobs = sh.du_jobs.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(j) = jobs.get(&key) {
        match &j.result {
            Some((at, res)) if !refresh && at.elapsed().as_secs() < 300 => {
                return match res {
                    Ok(json) => Response::ok_json(format!(
                        "{},\"cached_s\":{}}}",
                        &json[..json.len() - 1],
                        at.elapsed().as_secs()
                    )),
                    Err(e) => Response::error(400, e),
                };
            }
            None => {
                return Response::ok_json(
                    Obj::new()
                        .b("pending", true)
                        .s("path", &key)
                        .u("scanned", j.progress.load(Ordering::Relaxed))
                        .n("elapsed_s", j.started.elapsed().as_secs_f64())
                        .finish(),
                );
            }
            _ => {}
        }
    }
    // limpa resultados antigos e limita a quantidade de varreduras guardadas
    jobs.retain(|_, j| {
        j.result
            .as_ref()
            .map(|(at, _)| at.elapsed().as_secs() < 600)
            .unwrap_or(true)
    });
    if jobs.values().filter(|j| j.result.is_none()).count() >= 3 {
        return Response::error(429, "já há varreduras demais em andamento; aguarde");
    }
    let progress = Arc::new(AtomicU64::new(0));
    jobs.insert(
        key.clone(),
        crate::state::DuJob {
            started: std::time::Instant::now(),
            progress: progress.clone(),
            result: None,
        },
    );
    drop(jobs);
    let sh2 = sh.clone();
    let k2 = key.clone();
    std::thread::spawn(move || {
        // varredura pesada: não deve competir com o resto da máquina
        let _ = sys::lower_thread_priority(10);
        let res =
            disk::du_with_progress(&k2, 5_000_000, 180_000, Some(&progress)).map(|r| r.to_json());
        let mut jobs = sh2.du_jobs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(j) = jobs.get_mut(&k2) {
            j.result = Some((std::time::Instant::now(), res));
        }
    });
    Response::ok_json(
        Obj::new()
            .b("pending", true)
            .s("path", &key)
            .u("scanned", 0)
            .n("elapsed_s", 0.0)
            .finish(),
    )
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
    sys::home()
        .join(".local/bin/pulse-hook")
        .to_string_lossy()
        .into_owned()
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
            .raw(
                "connected",
                &json::arr(connected.iter().map(|s| json::quote(s))),
            )
            .u("total_events", hooks::CLAUDE_EVENTS.len() as u64)
            .s("snippet", &hooks::claude_snippet(&script))
            .s("codex_snippet", &hooks::codex_snippet(&script))
            .b(
                "codex_connected",
                hooks::codex_status(&sys::home().join(".codex/config.toml")),
            )
            .finish(),
    )
}

/// Grava [agents] e [prices] no config.toml e aplica na hora.
fn config_agents(sh: &Shared, v: &Value, peer: &str) -> Response {
    let mut prefs = sh.prefs.read().map(|p| p.clone()).unwrap_or_default();
    let mut agent_pairs: Vec<(String, String)> = Vec::new();
    for k in ["notify_attention", "show_tool_details"] {
        if let Some(x) = v.get(k) {
            let Some(b) = x.as_bool() else {
                return Response::error(400, &format!("{} precisa ser true ou false", k));
            };
            if k == "notify_attention" {
                prefs.notify_attention = b;
            } else {
                prefs.show_tool_details = b;
            }
            agent_pairs.push((k.to_string(), b.to_string()));
        }
    }
    let mut price_pairs: Option<Vec<(String, String)>> = None;
    if let Some(Value::Obj(items)) = v.get("prices") {
        let mut map = std::collections::BTreeMap::new();
        for (model, val) in items {
            if !settings::valid_model_key(model) {
                return Response::error(400, &format!("nome de modelo inválido: {}", model));
            }
            let nums: Option<Vec<f64>> = val
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_f64()).collect());
            let p = nums
                .filter(|n| n.len() == 4)
                .map(|n| {
                    n.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .and_then(|s| settings::parse_price(&s));
            let Some(p) = p else {
                return Response::error(
                    400,
                    &format!("preço de {} precisa ter 4 números ≥ 0", model),
                );
            };
            map.insert(model.clone(), p);
        }
        price_pairs = Some(
            map.iter()
                .map(|(k, p)| (json::quote(k), settings::fmt_price(p)))
                .collect(),
        );
        prefs.prices = map;
    }
    let mut src = std::fs::read_to_string(&sh.config_path).unwrap_or_default();
    if !agent_pairs.is_empty() {
        src = settings::rewrite_section(&src, "agents", &agent_pairs, false);
    }
    if let Some(pp) = &price_pairs {
        src = settings::rewrite_section(&src, "prices", pp, true);
    }
    let res = settings::write_atomic(&sh.config_path, &src);
    auth::audit(
        "config.agents",
        &sh.config_path.to_string_lossy(),
        res.as_ref().map(|_| "ok").unwrap_or_else(|e| e),
        peer,
    );
    match res {
        Ok(()) => {
            let j = prefs.to_json();
            if let Ok(mut p) = sh.prefs.write() {
                *p = prefs;
            }
            Response::ok_json(Obj::new().b("ok", true).raw("prefs", &j).finish())
        }
        Err(e) => Response::error(
            500,
            &format!("não foi possível gravar a configuração: {}", e),
        ),
    }
}

/// Avisa no desktop que um agente pede atenção (no máximo 1×/min por sessão).
fn notify_attention(sh: &Shared, e: &hooks::HookEvent) {
    {
        let mut sent = sh.attention_sent.lock().unwrap_or_else(|x| x.into_inner());
        if let Some(t) = sent.get(&e.session) {
            if t.elapsed().as_secs() < 60 {
                return;
            }
        }
        sent.insert(e.session.clone(), std::time::Instant::now());
        if sent.len() > 200 {
            sent.retain(|_, t| t.elapsed().as_secs() < 600);
        }
    }
    let agent = match e.agent.as_str() {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "openclaw" | "ponto" => "OpenClaw",
        other => other,
    };
    let project = e.cwd.rsplit('/').next().unwrap_or("");
    let mut body = String::new();
    if !project.is_empty() {
        body.push_str(project);
    }
    if !e.message.is_empty() {
        if !body.is_empty() {
            body.push_str(" · ");
        }
        body.push_str(&e.message);
    } else if !e.tool.is_empty() {
        if !body.is_empty() {
            body.push_str(" · ");
        }
        body.push_str(&format!("quer usar {}", e.tool));
    }
    crate::state::notify_text(&format!("{} está esperando você", agent), &body, "normal");
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
            .raw("usage", &st.usage_json)
            .raw(
                "prefs",
                &sh.prefs
                    .read()
                    .map(|p| p.to_json())
                    .unwrap_or_else(|_| "{}".into()),
            )
            .opt_s("openclaw_note", st.openclaw_note.as_deref())
            .finish(),
    )
}

fn process_action(_sh: &Arc<Shared>, req: &Request, p: &str, peer: &str) -> Response {
    // /api/processes/{pid}/{acao}
    let parts: Vec<&str> = p.trim_start_matches("/api/processes/").split('/').collect();
    if parts.len() != 2 {
        return Response::error(404, "rota não encontrada");
    }
    let Ok(pid) = parts[0].parse::<i32>() else {
        return Response::error(400, "PID inválido");
    };
    let action = parts[1];
    if pid <= 1 {
        return Response::error(403, "PID protegido");
    }
    if pid == sys::self_pid() {
        return Response::error(403, "o Pulse não age sobre si mesmo");
    }
    let Some(start) = procs::start_ticks(pid) else {
        return Response::error(404, "processo não encontrado");
    };
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
                    auth::audit(
                        "process.kill",
                        &target,
                        r.as_ref().map(|_| "ok").unwrap_or_else(|e| e),
                        &peer,
                    );
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
                Some(n) if (-20.0..=19.0).contains(&n) => sys::renice(pid, n as i32)
                    .map(|_| format!("prioridade ajustada para {}", n as i32)),
                _ => Err("nice deve estar entre -20 e 19".into()),
            }
        }
        _ => return Response::error(404, "ação desconhecida"),
    };
    auth::audit(
        &format!("process.{}", action),
        &target,
        res.as_ref().map(|s| s.as_str()).unwrap_or_else(|e| e),
        peer,
    );
    match res {
        Ok(msg) => Response::ok_json(Obj::new().b("ok", true).s("message", &msg).finish()),
        Err(e) => Response::error(
            if e.contains("ermission") || e.contains("permitida") {
                403
            } else {
                400
            },
            &e,
        ),
    }
}
