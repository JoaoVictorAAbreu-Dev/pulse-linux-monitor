//! Outras máquinas com Pulse, vistas a partir desta ("Pulse central").
//!
//! Cada máquina é alcançada por um túnel `ssh -L` supervisionado pelo daemon
//! (reconecta sozinho) ou por uma URL direta (ex.: um túnel que você já
//! mantém). O acesso é somente leitura: o daemon repassa apenas requisições
//! GET para `/api/...` da outra máquina, inclusive o fluxo SSE, sempre com o
//! token dela — nunca o cookie ou o token desta máquina.

use pulse_core::json::{self, Obj};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RemoteCfg {
    pub name: String,
    /// `usuario@host` para o túnel SSH.
    pub ssh: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_key: Option<String>,
    /// URL direta (`http://127.0.0.1:17878`), alternativa ao SSH.
    pub url: Option<String>,
    /// Porta do Pulse na outra máquina (padrão 7878).
    pub remote_port: u16,
    pub token: String,
}

pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 40
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn valid_ssh_target(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 200
        && !t.starts_with('-')
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || "@.-_:[]".contains(c))
}

/// (host, porta) de uma URL http://host:porta.
pub fn parse_http_url(u: &str) -> Option<(String, u16)> {
    let rest = u.strip_prefix("http://")?;
    let hostport = rest.split('/').next()?;
    let (h, p) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || hostport.starts_with('[') => {
            (h.to_string(), p.parse().ok()?)
        }
        _ => (hostport.to_string(), 80),
    };
    if h.is_empty() || h.contains('@') {
        return None;
    }
    Some((h.trim_matches(['[', ']']).to_string(), p))
}

#[derive(Debug, Default)]
pub struct Status {
    pub state: &'static str, // "conectando" | "ok" | "erro"
    pub error: Option<String>,
    pub hostname: String,
    pub version: String,
    pub checked: Option<Instant>,
    pub local_port: Option<u16>,
}

pub struct Remote {
    pub cfg: RemoteCfg,
    pub status: Mutex<Status>,
    child: Mutex<Option<Child>>,
}

impl Remote {
    /// Endereço TCP a usar agora (túnel local ou URL direta).
    pub fn addr(&self) -> Option<(String, u16)> {
        if let Some(u) = &self.cfg.url {
            return parse_http_url(u);
        }
        let p = self.status.lock().ok()?.local_port?;
        Some(("127.0.0.1".into(), p))
    }
}

pub type Remotes = Arc<BTreeMap<String, Arc<Remote>>>;

pub fn build(cfgs: &BTreeMap<String, RemoteCfg>) -> Remotes {
    Arc::new(
        cfgs.iter()
            .map(|(k, c)| {
                (
                    k.clone(),
                    Arc::new(Remote {
                        cfg: c.clone(),
                        status: Mutex::new(Status {
                            state: "conectando",
                            ..Default::default()
                        }),
                        child: Mutex::new(None),
                    }),
                )
            })
            .collect(),
    )
}

fn free_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0")
        .ok()?
        .local_addr()
        .ok()
        .map(|a| a.port())
}

/// Argumentos do `ssh` para o túnel (testável).
pub fn ssh_args(c: &RemoteCfg, local: u16) -> Vec<String> {
    let mut a = vec![
        "-N".to_string(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-o".into(),
        "ServerAliveInterval=15".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
        "-L".into(),
        format!("127.0.0.1:{}:127.0.0.1:{}", local, c.remote_port),
    ];
    if let Some(k) = &c.ssh_key {
        a.push("-i".into());
        a.push(k.clone());
    }
    if let Some(p) = c.ssh_port {
        a.push("-p".into());
        a.push(p.to_string());
    }
    a.push("--".into());
    a.push(c.ssh.clone().unwrap_or_default());
    a
}

/// GET simples no Pulse remoto; devolve (status, corpo).
pub fn get(r: &Remote, path: &str, timeout: Duration) -> Result<(u16, String), String> {
    let (h, p) = r.addr().ok_or("túnel ainda não estabelecido")?;
    let mut s = TcpStream::connect((h.as_str(), p)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    write!(
        s,
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Pulse-Token: {}\r\nConnection: close\r\n\r\n",
        path, r.cfg.remote_port, r.cfg.token
    )
    .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let t = String::from_utf8_lossy(&raw);
    let (head, body) = t.split_once("\r\n\r\n").unwrap_or((&t, ""));
    let st = head
        .split_whitespace()
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    Ok((st, body.to_string()))
}

fn check(r: &Remote) {
    let res = get(r, "/api/snapshot", Duration::from_secs(5));
    let mut st = r.status.lock().unwrap_or_else(|e| e.into_inner());
    st.checked = Some(Instant::now());
    match res {
        Ok((200, body)) => {
            let v = json::parse(&body).ok();
            st.hostname = v
                .as_ref()
                .and_then(|v| v.path(&["host", "hostname"]))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            st.version = v
                .as_ref()
                .and_then(|v| v.path(&["pulse", "version"]))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            st.state = "ok";
            st.error = None;
        }
        Ok((401, _)) => {
            st.state = "erro";
            st.error = Some("token recusado pela outra máquina".into());
        }
        Ok((code, _)) => {
            st.state = "erro";
            st.error = Some(format!("resposta HTTP {}", code));
        }
        Err(e) => {
            // sem túnel ainda: continua "conectando"; com endereço e falha: "erro"
            st.state = if st.local_port.is_none() && r.cfg.url.is_none() {
                "conectando"
            } else {
                "erro"
            };
            st.error = Some(e);
        }
    }
}

/// Supervisiona túneis e verifica a saúde de cada máquina a cada 15 s.
pub fn spawn_supervisor(remotes: Remotes) {
    if remotes.is_empty() {
        return;
    }
    std::thread::Builder::new()
        .name("remotos".into())
        .spawn(move || {
            let mut backoff: BTreeMap<String, (Instant, u64)> = BTreeMap::new();
            loop {
                for (name, r) in remotes.iter() {
                    if r.cfg.ssh.is_some() {
                        let mut child = r.child.lock().unwrap_or_else(|e| e.into_inner());
                        let alive = child
                            .as_mut()
                            .map(|c| matches!(c.try_wait(), Ok(None)))
                            .unwrap_or(false);
                        if !alive {
                            let mut err = String::new();
                            if let Some(mut c) = child.take() {
                                if let Some(mut e) = c.stderr.take() {
                                    let _ = e.read_to_string(&mut err);
                                }
                                let _ = c.wait();
                            }
                            let (next, wait) =
                                backoff.get(name).copied().unwrap_or((Instant::now(), 0));
                            if Instant::now() >= next {
                                let port = free_port();
                                let spawned = port.and_then(|p| {
                                    Command::new("ssh")
                                        .args(ssh_args(&r.cfg, p))
                                        .stdin(Stdio::null())
                                        .stdout(Stdio::null())
                                        .stderr(Stdio::piped())
                                        .spawn()
                                        .ok()
                                });
                                let mut st = r.status.lock().unwrap_or_else(|e| e.into_inner());
                                st.local_port = if spawned.is_some() { port } else { None };
                                if !err.trim().is_empty() {
                                    st.state = "erro";
                                    st.error = Some(format!(
                                        "ssh: {}",
                                        err.lines().last().unwrap_or("").trim()
                                    ));
                                }
                                *child = spawned;
                                let w = (wait * 2).clamp(5, 300);
                                backoff.insert(
                                    name.clone(),
                                    (Instant::now() + Duration::from_secs(w), w),
                                );
                                drop(st);
                                drop(child);
                                // dá tempo ao túnel antes de verificar
                                std::thread::sleep(Duration::from_millis(1500));
                            }
                        } else if r.status.lock().map(|s| s.state == "ok").unwrap_or(false) {
                            backoff.remove(name);
                        }
                    }
                    check(r);
                }
                std::thread::sleep(Duration::from_secs(15));
            }
        })
        .expect("thread dos remotos");
}

pub fn list_json(remotes: &Remotes) -> String {
    json::arr(remotes.values().map(|r| {
        let st = r.status.lock().unwrap_or_else(|e| e.into_inner());
        Obj::new()
            .s("name", &r.cfg.name)
            .s("mode", if r.cfg.ssh.is_some() { "ssh" } else { "url" })
            .s(
                "target",
                r.cfg
                    .ssh
                    .as_deref()
                    .unwrap_or(r.cfg.url.as_deref().unwrap_or("")),
            )
            .s("state", st.state)
            .opt_s("error", st.error.as_deref())
            .s("hostname", &st.hostname)
            .s("version", &st.version)
            .finish()
    }))
}

/// Encaminha `GET /<caminho>` para a máquina remota e copia a resposta crua
/// (status, cabeçalhos e corpo — inclusive SSE) para o cliente.
pub fn proxy(r: &Remote, path_and_query: &str, client: &mut TcpStream) -> Result<(), String> {
    let (h, p) = r.addr().ok_or("túnel ainda não estabelecido")?;
    let mut up = TcpStream::connect((h.as_str(), p)).map_err(|e| e.to_string())?;
    up.set_write_timeout(Some(Duration::from_secs(10))).ok();
    write!(
        up,
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Pulse-Token: {}\r\nConnection: close\r\n\r\n",
        path_and_query, r.cfg.remote_port, r.cfg.token
    )
    .map_err(|e| e.to_string())?;
    std::io::copy(&mut up, client)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_targets_and_urls() {
        assert!(valid_name("servidor-1"));
        assert!(!valid_name("Servidor") && !valid_name("a b") && !valid_name(""));
        assert!(valid_ssh_target("ana@192.168.1.10"));
        assert!(!valid_ssh_target("-oProxyCommand=x") && !valid_ssh_target("a;b"));
        assert_eq!(
            parse_http_url("http://127.0.0.1:17878"),
            Some(("127.0.0.1".into(), 17878))
        );
        assert_eq!(
            parse_http_url("http://pc.local/"),
            Some(("pc.local".into(), 80))
        );
        assert_eq!(
            parse_http_url("http://[::1]:7878"),
            Some(("::1".into(), 7878))
        );
        assert_eq!(parse_http_url("https://x"), None);
        assert_eq!(parse_http_url("http://u:p@x:1"), None);
    }

    #[test]
    fn ssh_command_is_safe() {
        let c = RemoteCfg {
            name: "srv".into(),
            ssh: Some("ana@srv".into()),
            ssh_port: Some(2222),
            ssh_key: Some("/home/ana/.ssh/id_pulse".into()),
            remote_port: 7878,
            ..Default::default()
        };
        let a = ssh_args(&c, 40000);
        assert!(
            a.contains(&"BatchMode=yes".to_string())
                && a.contains(&"127.0.0.1:40000:127.0.0.1:7878".to_string())
        );
        assert_eq!(
            &a[a.len() - 2..],
            &["--".to_string(), "ana@srv".to_string()],
            "alvo depois de --"
        );
        assert!(a.windows(2).any(|w| w == ["-p", "2222"]));
    }
}
