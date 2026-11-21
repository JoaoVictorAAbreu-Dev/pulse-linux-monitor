//! Argumentos de linha de comando.

use std::net::IpAddr;

pub const HELP: &str = "Pulse — monitor do Ubuntu com painel de agentes de IA

Uso: pulse [opções]

Opções:
  --port N            porta HTTP (padrão 7878)
  --bind IP           endereço de escuta (padrão 127.0.0.1)
  --allow-remote      permite --bind fora do loopback (expõe o painel na rede)
  --interval-ms N     intervalo do coletor em ms (padrão 1000, mínimo 250)
  --notify            envia alertas novos ao desktop via notify-send
  --no-agents         desativa detecção de agentes, sessões e hooks
  --print-token       imprime a URL de acesso com o token e sai
  --once              coleta duas amostras, imprime o snapshot JSON e sai
  -h, --help          mostra esta ajuda

Arquivos:
  ~/.config/pulse/token            token de acesso (modo 0600)
  ~/.config/pulse/agents.conf      regras extras de detecção de agentes
  ~/.local/state/pulse/history.log histórico longo (médias de 60 s, 7 dias)
  ~/.local/state/pulse/audit.log   auditoria de ações sensíveis
";

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub port: u16,
    pub bind: IpAddr,
    pub allow_remote: bool,
    pub interval_ms: u64,
    pub notify: bool,
    pub agents: bool,
    pub print_token: bool,
    pub once: bool,
    pub help: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: 7878,
            bind: IpAddr::from([127, 0, 0, 1]),
            allow_remote: false,
            interval_ms: 1000,
            notify: false,
            agents: true,
            print_token: false,
            once: false,
            help: false,
        }
    }
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Config, String> {
    let mut c = Config::default();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline.clone().or_else(|| it.next()).ok_or_else(|| format!("{} precisa de um valor", name))
        };
        match flag.as_str() {
            "--port" => c.port = value("--port")?.parse().map_err(|_| "porta inválida".to_string())?,
            "--bind" => c.bind = value("--bind")?.parse().map_err(|_| "endereço inválido em --bind".to_string())?,
            "--interval-ms" => {
                c.interval_ms = value("--interval-ms")?.parse().map_err(|_| "intervalo inválido".to_string())?;
            }
            "--allow-remote" => c.allow_remote = true,
            "--notify" => c.notify = true,
            "--no-agents" => c.agents = false,
            "--print-token" => c.print_token = true,
            "--once" => c.once = true,
            "-h" | "--help" => c.help = true,
            other => return Err(format!("opção desconhecida: {}", other)),
        }
    }
    if c.interval_ms < 250 {
        return Err("--interval-ms mínimo é 250".into());
    }
    if c.port == 0 {
        return Err("porta inválida".into());
    }
    if !c.bind.is_loopback() && !c.allow_remote {
        return Err(format!(
            "--bind {} está fora do loopback; use --allow-remote se realmente quiser expor o painel na rede",
            c.bind
        ));
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: &[&str]) -> Result<Config, String> {
        parse(v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults() {
        let c = p(&[]).unwrap();
        assert_eq!(c.port, 7878);
        assert!(c.bind.is_loopback());
        assert!(c.agents);
    }

    #[test]
    fn flags() {
        let c = p(&["--port", "9000", "--interval-ms=500", "--notify", "--no-agents", "--once"]).unwrap();
        assert_eq!((c.port, c.interval_ms, c.notify, c.agents, c.once), (9000, 500, true, false, true));
    }

    #[test]
    fn remote_requires_flag() {
        assert!(p(&["--bind", "0.0.0.0"]).is_err());
        assert!(p(&["--bind", "0.0.0.0", "--allow-remote"]).is_ok());
        assert!(p(&["--bind", "::1"]).is_ok());
    }

    #[test]
    fn rejects_bad_values() {
        assert!(p(&["--interval-ms", "100"]).is_err());
        assert!(p(&["--port"]).is_err());
        assert!(p(&["--port", "x"]).is_err());
        assert!(p(&["--wat"]).is_err());
    }
}
