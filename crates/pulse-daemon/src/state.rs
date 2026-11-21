//! Estado compartilhado (Mutex + Condvar) e a thread do coletor.

use crate::auth::FailTracker;
use crate::config::Config;
use pulse_agents::hooks::{HookEvent, HookStore};
use pulse_agents::{detect, sessions};
use pulse_core::alerts::{self, Engine, Rules};
use pulse_core::collector::{Collector, Tick};
use pulse_core::eventlog::EventLog;
use pulse_core::history::Ring;
use pulse_core::json::Obj;
use pulse_core::longterm::{LongStore, BUCKET_MS};
use pulse_core::procs::{self, Proc};
use pulse_core::sys;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

pub const HISTORY_CAP: usize = 900;

pub struct State {
    pub seq: u64,
    pub snapshot: Arc<String>,
    pub ring: Ring,
    pub long: LongStore,
    pub alerts: Engine,
    pub procs: Arc<Vec<Proc>>,
    pub agents_json: Arc<String>,
    pub sessions_json: Arc<String>,
    /// (sessões nas últimas 24 h, tokens dessas sessões)
    pub sessions_summary: (usize, u64),
    pub hooks: HookStore,
    pub hook_log: EventLog,
    pub alert_log: EventLog,
    pub started: Instant,
}

pub struct Shared {
    pub state: Mutex<State>,
    pub tick: Condvar,
    pub cfg: Config,
    token: RwLock<String>,
    token_mtime: Mutex<Option<std::time::SystemTime>>,
    pub fails: FailTracker,
    pub allowed_hosts: Vec<String>,
    pub config_path: PathBuf,
    pub config_warnings: Vec<String>,
}

pub const EVENT_CAP: usize = 300;

impl Shared {
    pub fn new(
        cfg: Config,
        token: String,
        allowed_hosts: Vec<String>,
        rules: Rules,
        config_path: PathBuf,
        config_warnings: Vec<String>,
    ) -> Arc<Self> {
        let dir = sys::state_dir();
        let days = cfg.retention_days.max(1);
        let long = LongStore::open_with(
            dir.join("history.log"),
            (days * 86_400_000 / BUCKET_MS) as usize,
            days * 86_400_000,
            sys::now_ms(),
        );
        let mut alerts = Engine::new(rules);
        let alert_log = EventLog::open(dir.join("alerts.log"), EVENT_CAP, |l| {
            alerts::Event::from_json(l).is_some()
        });
        alerts.restore_history(alert_log.lines());
        let mut hooks = HookStore::default();
        let hook_log = EventLog::open(dir.join("hooks.log"), EVENT_CAP, |l| {
            HookEvent::from_json(l).is_some()
        });
        hooks.restore(hook_log.lines());
        Arc::new(Shared {
            state: Mutex::new(State {
                seq: 0,
                snapshot: Arc::new("{}".into()),
                ring: Ring::new(HISTORY_CAP),
                long,
                alerts,
                procs: Arc::new(Vec::new()),
                agents_json: Arc::new("[]".into()),
                sessions_json: Arc::new("[]".into()),
                sessions_summary: (0, 0),
                hooks,
                hook_log,
                alert_log,
                started: Instant::now(),
            }),
            tick: Condvar::new(),
            cfg,
            token_mtime: Mutex::new(token_mtime()),
            token: RwLock::new(token),
            fails: FailTracker::default(),
            allowed_hosts,
            config_path,
            config_warnings,
        })
    }

    pub fn token(&self) -> String {
        self.token.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Recarrega o token se o arquivo mudou (ex.: `pulse --rotate-token`).
    pub fn reload_token_if_changed(&self) {
        let now = token_mtime();
        let mut last = self.token_mtime.lock().unwrap_or_else(|e| e.into_inner());
        if now.is_some() && now != *last {
            if let Ok(t) = crate::auth::load_or_create_token(&crate::auth::token_path()) {
                *self.token.write().unwrap_or_else(|e| e.into_inner()) = t;
                crate::auth::audit("token.reload", "arquivo de token alterado", "ok", "local");
            }
            *last = now;
        }
    }

    /// Trava o estado mesmo se outra thread entrou em pânico segurando-o.
    pub fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn token_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(crate::auth::token_path())
        .and_then(|m| m.modified())
        .ok()
}

fn agent_rules() -> Vec<detect::AgentRule> {
    let mut rules = detect::default_rules();
    if let Some(conf) = sys::read(&sys::config_dir().join("agents.conf").to_string_lossy()) {
        let extra = detect::parse_conf(&conf);
        // regras do usuário têm prioridade
        rules.splice(0..0, extra);
    }
    rules
}

/// Monta o JSON do snapshot a partir de uma amostra.
pub fn build_snapshot(
    t: &Tick,
    alerts_json: &str,
    agents_summary: &str,
    seq: u64,
    interval_ms: u64,
) -> String {
    let mut o = Obj::new()
        .u("ts", t.ts)
        .u("seq", seq)
        .u("interval_ms", interval_ms);
    for (k, v) in &t.sections {
        o = o.raw(k, v);
    }
    o.raw(
        "pulse",
        &Obj::new()
            .n("cpu", t.self_cpu)
            .u("rss", t.self_rss)
            .s("version", env!("CARGO_PKG_VERSION"))
            .finish(),
    )
    .raw("alerts", alerts_json)
    .raw("agents", agents_summary)
    .finish()
}

pub fn alert_input(t: &Tick) -> alerts::Input {
    alerts::Input {
        ts: t.ts,
        cpu: t.point.cpu,
        mem_pct: t.mem.used_pct(),
        swap_pct: t.mem.swap_pct(),
        swap_total: t.mem.swap_total,
        disks: t
            .fs
            .iter()
            .map(|f| (f.mount.mount.clone(), f.pct))
            .collect(),
        temp_max: if t.point.temp.is_finite() {
            Some(t.point.temp)
        } else {
            None
        },
        procs: t
            .procs
            .iter()
            .filter(|p| p.cpu > 50.0)
            .map(|p| (p.pid, p.start_ticks, p.name.clone(), p.cpu))
            .collect(),
    }
}

fn notify(a: &alerts::Alert) {
    let urgency = if a.severity == alerts::Severity::Critical {
        "critical"
    } else {
        "normal"
    };
    let _ = std::process::Command::new("notify-send")
        .args([
            "-a",
            "Pulse",
            "-u",
            urgency,
            &format!("Pulse: {}", a.title),
            &a.detail,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut c| {
            std::thread::spawn(move || {
                let _ = c.wait();
            })
        });
}

pub fn spawn_collector(shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("coletor".into())
        .spawn(move || {
            let cfg = shared.cfg.clone();
            let mut col = Collector::new();
            let rules = agent_rules();
            let mut scanner = sessions::Scanner::new(&sys::home());
            let mut last_scan: Option<Instant> = None;
            let interval = Duration::from_millis(cfg.interval_ms);
            let mut last_token_check = Instant::now();
            loop {
                let started = Instant::now();
                if last_token_check.elapsed() >= Duration::from_secs(3) {
                    last_token_check = Instant::now();
                    shared.reload_token_if_changed();
                }
                let t = col.sample();

                // Agentes (fora da trava).
                let (agents_json, agents_summary, sessions_json) = if cfg.agents {
                    let found = detect::detect(&rules, &t.procs);
                    let mcp: usize = found.iter().map(|a| a.mcp.len()).sum();
                    let sessions_json = if last_scan
                        .map(|l| l.elapsed().as_secs() >= 10)
                        .unwrap_or(true)
                    {
                        last_scan = Some(Instant::now());
                        let list = scanner.scan();
                        Some((sessions::list_json(&list), list))
                    } else {
                        None
                    };
                    (
                        Some(detect::to_json(&found)),
                        Some((found.len(), mcp)),
                        sessions_json,
                    )
                } else {
                    (None, None, None)
                };

                let mut st = shared.lock();
                let fired = st.alerts.evaluate(&alert_input(&t));
                for line in st.alerts.drain_pending() {
                    st.alert_log.append(&line);
                }
                if let Some(j) = agents_json {
                    st.agents_json = Arc::new(j);
                }
                if let Some((j, list)) = sessions_json {
                    st.sessions_json = Arc::new(j);
                    let day_ago = t.ts.saturating_sub(24 * 3600 * 1000);
                    let recent: Vec<&sessions::Session> =
                        list.iter().filter(|s| s.updated >= day_ago).collect();
                    st.sessions_summary =
                        (recent.len(), recent.iter().map(|s| s.tokens_total()).sum());
                }
                let (recent, tokens) = st.sessions_summary;
                let summary = match agents_summary {
                    Some((n, mcp)) => Obj::new()
                        .b("enabled", true)
                        .u("processes", n as u64)
                        .u("mcp", mcp as u64)
                        .u("sessions_24h", recent as u64)
                        .u("tokens_24h", tokens)
                        .u("hook_sessions", st.hooks.sessions.len() as u64)
                        .finish(),
                    None => Obj::new().b("enabled", false).finish(),
                };
                st.seq += 1;
                let seq = st.seq;
                let snap =
                    build_snapshot(&t, &st.alerts.active_json(), &summary, seq, cfg.interval_ms);
                st.snapshot = Arc::new(snap);
                st.ring.push(t.point);
                st.long.add(t.point);
                st.procs = Arc::new(t.procs);
                drop(st);
                shared.tick.notify_all();

                if cfg.notify {
                    for a in &fired {
                        notify(a);
                    }
                }
                let el = started.elapsed();
                if el < interval {
                    std::thread::sleep(interval - el);
                }
            }
        })
        .expect("thread do coletor");
}

/// Usado por `--once`: duas amostras e o snapshot no stdout.
pub fn once(cfg: &Config) -> String {
    let mut col = Collector::new();
    std::thread::sleep(Duration::from_millis(cfg.interval_ms.min(1000)));
    let t = col.sample();
    let mut eng = Engine::default();
    eng.evaluate(&alert_input(&t));
    let summary = if cfg.agents {
        let found = detect::detect(&agent_rules(), &t.procs);
        let mcp: usize = found.iter().map(|a| a.mcp.len()).sum();
        Obj::new()
            .b("enabled", true)
            .u("processes", found.len() as u64)
            .u("mcp", mcp as u64)
            .raw("list", &detect::to_json(&found))
            .finish()
    } else {
        Obj::new().b("enabled", false).finish()
    };
    build_snapshot(&t, &eng.active_json(), &summary, 1, cfg.interval_ms)
}

pub fn procs_json(list: &[Proc]) -> String {
    let s = procs::summarize(list);
    Obj::new()
        .raw("summary", &procs::summary_json(&s))
        .raw("list", &procs::list_json(list))
        .finish()
}
