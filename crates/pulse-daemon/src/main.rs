//! Pulse — daemon HTTP com coletor, API, SSE e UI embutida.

mod api;
mod auth;
mod config;
mod http;
mod metrics;
mod notify;
mod remotes;
mod settings;
mod state;

use http::{ParseError, Response};
use state::Shared;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const MAX_CONNECTIONS: usize = 64;
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn fail(msg: &str, code: i32) -> ! {
    eprintln!("pulse: {}", msg);
    std::process::exit(code);
}

fn main() {
    pulse_core::sys::tune_allocator();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // 1) arquivo de configuração (padrões < arquivo < flags)
    let cfg_path = config::config_arg(&args).unwrap_or_else(settings::path);
    let (file, mut warnings) = match std::fs::read_to_string(&cfg_path) {
        Ok(src) => settings::parse(&src),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(e) => (
            Default::default(),
            vec![format!(
                "não foi possível ler {}: {}",
                cfg_path.display(),
                e
            )],
        ),
    };
    let base = config::Config::from_file(&file).unwrap_or_else(|e| fail(&e, 2));
    let cfg = match config::parse_with(base, args) {
        Ok(c) => c,
        Err(e) => fail(&format!("{}\n\nUse --help para ver as opções.", e), 2),
    };
    let rules = match settings::apply_alerts(&Default::default(), &file.alerts) {
        Ok(r) => r,
        Err(e) => {
            warnings.push(format!("[alerts] ignorado: {}", e));
            Default::default()
        }
    };
    if cfg.print_config {
        print!("{}", settings::EXAMPLE);
        return;
    }
    if cfg.rotate_token {
        match auth::rotate_token(&auth::token_path()) {
            Ok(_) => {
                auth::audit("token.rotate", "pulse --rotate-token", "ok", "local");
                println!(
                    "Token novo gravado em {}. O daemon em execução passa a aceitá-lo em até 3 s;\n\
                     sessões abertas no navegador precisam entrar de novo (pulse --print-token).",
                    auth::token_path().display()
                );
                return;
            }
            Err(e) => fail(&format!("não foi possível trocar o token: {}", e), 1),
        }
    }
    if cfg.help {
        print!("{}", config::HELP);
        return;
    }
    if cfg.once {
        println!("{}", state::once(&cfg));
        return;
    }
    let token = match auth::load_or_create_token(&auth::token_path()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("pulse: não foi possível preparar o token: {}", e);
            std::process::exit(1);
        }
    };
    let host = if cfg.bind.is_loopback() {
        "127.0.0.1".to_string()
    } else {
        cfg.bind.to_string()
    };
    if cfg.print_token {
        println!("http://{}:{}/#token={}", host, cfg.port, token);
        return;
    }
    let addr = SocketAddr::new(cfg.bind, cfg.port);
    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("pulse: não foi possível escutar em {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    let allowed = auth::allowed_hosts(cfg.port, cfg.bind, cfg.allow_remote);
    for w in &warnings {
        eprintln!("pulse: config {}: {}", cfg_path.display(), w);
    }
    let prefs = state::AgentPrefs {
        notify_attention: file.notify_attention.unwrap_or(true),
        show_tool_details: file.show_tool_details.unwrap_or(false),
        prices: file.prices.clone(),
    };
    let shared = Shared::new(
        cfg.clone(),
        token,
        allowed,
        rules,
        cfg_path,
        warnings,
        prefs,
    );
    let mut channels = file.channels.clone();
    channels.alerts = file.notify_alerts.unwrap_or(true);
    channels.agents = file.notify_agents.unwrap_or(false);
    if let Ok(mut c) = shared.channels.write() {
        *c = channels;
    }
    let remotes = remotes::build(&file.remotes);
    remotes::spawn_supervisor(remotes.clone());
    if let Ok(mut r) = shared.remotes.write() {
        *r = remotes;
    }
    // O token nunca vai para o log: só o caminho do arquivo.
    eprintln!(
        "pulse: ouvindo em http://{} (intervalo {} ms)",
        addr, cfg.interval_ms
    );
    eprintln!(
        "pulse: token em {} — use `pulse --print-token` para obter a URL de acesso",
        auth::token_path().display()
    );
    if cfg.allow_remote && !cfg.bind.is_loopback() {
        eprintln!("pulse: ATENÇÃO: painel acessível pela rede em {}", addr);
    }
    if let Some(e) = &shared.lock().long.last_error {
        eprintln!("pulse: histórico longo: {}", e);
    }
    state::spawn_collector(shared.clone());
    if cfg.agents {
        state::spawn_usage_indexer(shared.clone());
    }

    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        if ACTIVE.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            let mut s = stream;
            let _ = http::write_response(&mut s, &Response::error(503, "conexões demais"));
            continue;
        }
        ACTIVE.fetch_add(1, Ordering::SeqCst);
        let sh = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("http".into())
            .spawn(move || {
                serve(sh, stream);
                ACTIVE.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            ACTIVE.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn serve(sh: Arc<Shared>, mut stream: TcpStream) {
    let peer = stream
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "?".into());
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let req = match http::read_request(&mut stream) {
        Ok(r) => r,
        Err(ParseError::Closed) => return,
        Err(ParseError::TooLarge) => {
            let _ = http::write_response(
                &mut stream,
                &Response::error(413, "requisição grande demais"),
            );
            return;
        }
        Err(ParseError::Bad(m)) => {
            let _ = http::write_response(&mut stream, &Response::error(400, m));
            return;
        }
    };
    match api::handle(&sh, &req, &peer) {
        api::Outcome::Resp(r) => {
            let _ = http::write_response(&mut stream, &r);
        }
        api::Outcome::Stream => sse(&sh, stream),
        api::Outcome::Proxy(remote, path) => {
            if let Err(e) = remotes::proxy(&remote, &path, &mut stream) {
                let _ = http::write_response(
                    &mut stream,
                    &Response::error(502, &format!("máquina {}: {}", remote.cfg.name, e)),
                );
            }
        }
    }
}

/// Server-Sent Events: envia o snapshot a cada amostra e eventos de hooks.
fn sse(sh: &Shared, mut s: TcpStream) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-store\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\n{}\r\n",
        http::SECURITY_HEADERS
    );
    if s.write_all(head.as_bytes()).is_err() {
        return;
    }
    let mut last_seq = 0u64;
    let mut last_hooks = { sh.lock().hooks.received };
    let _ = s.write_all(b"retry: 3000\n\n");
    loop {
        let (snap, seq, hook_ev) = {
            let mut st = sh.lock();
            while st.seq == last_seq && st.hooks.received == last_hooks {
                let (g, to) = sh
                    .tick
                    .wait_timeout(st, Duration::from_secs(15))
                    .unwrap_or_else(|e| e.into_inner());
                st = g;
                if to.timed_out() {
                    break;
                }
            }
            let hook_ev = if st.hooks.received != last_hooks {
                last_hooks = st.hooks.received;
                st.hooks.events.back().map(|e| e.to_json())
            } else {
                None
            };
            let snap = if st.seq != last_seq {
                Some(st.snapshot.clone())
            } else {
                None
            };
            (snap, st.seq, hook_ev)
        };
        let mut out = String::new();
        if let Some(h) = hook_ev {
            out.push_str("event: hook\ndata: ");
            out.push_str(&h);
            out.push_str("\n\n");
        }
        match snap {
            Some(j) => {
                last_seq = seq;
                out.push_str("event: snapshot\ndata: ");
                out.push_str(&j);
                out.push_str("\n\n");
            }
            None if out.is_empty() => out.push_str(": ping\n\n"),
            None => {}
        }
        if s.write_all(out.as_bytes()).and_then(|_| s.flush()).is_err() {
            return;
        }
    }
}
