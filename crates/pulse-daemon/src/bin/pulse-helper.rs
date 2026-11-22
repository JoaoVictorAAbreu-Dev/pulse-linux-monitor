//! pulse-helper — ações privilegiadas do Pulse, executadas via `pkexec`.
//!
//! Instalado em /usr/libexec/pulse-monitor/pulse-helper e autorizado pela
//! política polkit `io.github.pulse-monitor.*`. Aceita só comandos fixos com
//! argumentos validados; nunca executa nada vindo do usuário como código.
//!
//!   pulse-helper unit <start|stop|restart> <nome.service>
//!   pulse-helper netprobe <segundos 1..30>
//!
//! O `netprobe` roda um script bpftrace fixo por N segundos e imprime JSON com
//! bytes TCP/UDP enviados e recebidos por processo e a latência dos discos.

use std::process::{Command, Stdio};

const SYSTEMCTL: &str = "/usr/bin/systemctl";
const BPFTRACE: &[&str] = &["/usr/bin/bpftrace", "/usr/sbin/bpftrace"];

/// Script fixo (o único texto variável é o número de segundos, já validado).
pub fn netprobe_script(secs: u32) -> String {
    format!(
        r#"kprobe:tcp_sendmsg {{ @tx[pid, comm] = sum(arg2); }}
kprobe:udp_sendmsg {{ @tx[pid, comm] = sum(arg2); }}
kprobe:tcp_cleanup_rbuf /(int32)arg1 > 0/ {{ @rx[pid, comm] = sum((int32)arg1); }}
tracepoint:block:block_rq_issue {{ @start[args.dev, args.sector] = nsecs; }}
tracepoint:block:block_rq_complete /@start[args.dev, args.sector]/ {{
  $us = (nsecs - @start[args.dev, args.sector]) / 1000;
  @lat_count = count(); @lat_sum = sum($us); @lat_max = max($us);
  delete(@start[args.dev, args.sector]);
}}
interval:s:{secs} {{ clear(@start); exit(); }}
"#
    )
}

fn valid_unit(u: &str) -> bool {
    !u.is_empty()
        && u.len() <= 200
        && !u.starts_with('-')
        && u.ends_with(".service")
        && u.chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._:-\\".contains(c))
}

fn fail(msg: &str, code: i32) -> ! {
    eprintln!("pulse-helper: {}", msg);
    std::process::exit(code);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match a.as_slice() {
        ["unit", verb @ ("start" | "stop" | "restart"), unit] => {
            if !valid_unit(unit) {
                fail("nome de unidade inválido", 2);
            }
            let st = Command::new(SYSTEMCTL)
                .args([*verb, "--no-ask-password", "--", unit])
                .env_clear()
                .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
                .stdin(Stdio::null())
                .status()
                .unwrap_or_else(|e| fail(&format!("systemctl: {}", e), 1));
            std::process::exit(st.code().unwrap_or(1));
        }
        ["netprobe", secs] => {
            let secs: u32 = secs
                .parse()
                .ok()
                .filter(|s| (1..=30).contains(s))
                .unwrap_or_else(|| fail("segundos deve ser de 1 a 30", 2));
            let bin = BPFTRACE
                .iter()
                .find(|p| std::path::Path::new(p).exists())
                .unwrap_or_else(|| {
                    fail("bpftrace não está instalado (sudo apt install bpftrace)", 3)
                });
            let st = Command::new(bin)
                .args(["-f", "json", "-q", "-e", &netprobe_script(secs)])
                .env_clear()
                .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
                .env("BPFTRACE_MAX_MAP_KEYS", "8192")
                .stdin(Stdio::null())
                .status()
                .unwrap_or_else(|e| fail(&format!("bpftrace: {}", e), 1));
            std::process::exit(st.code().unwrap_or(1));
        }
        _ => fail(
            "uso: pulse-helper unit <start|stop|restart> <nome.service> | netprobe <1..30>",
            2,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_script() {
        assert!(valid_unit("cron.service"));
        assert!(!valid_unit("--now"));
        assert!(!valid_unit("x; rm -rf /.service"));
        let s = netprobe_script(5);
        assert!(s.contains("interval:s:5"));
        assert!(s.contains("@tx[pid, comm]") && s.contains("@lat_max"));
    }
}
