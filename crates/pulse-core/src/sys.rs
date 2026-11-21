//! Utilidades de sistema: leitura de arquivos, FFI mínima com a libc
//! (statvfs, kill, setpriority, sysconf) e caminhos do usuário.

use std::collections::HashMap;
use std::ffi::CString;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[repr(C)]
#[derive(Default)]
struct StatVfs {
    f_bsize: u64,
    f_frsize: u64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_favail: u64,
    f_fsid: u64,
    f_flag: u64,
    f_namemax: u64,
    _spare: [i32; 6],
}

extern "C" {
    fn statvfs(path: *const std::os::raw::c_char, buf: *mut StatVfs) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn setpriority(which: i32, who: u32, prio: i32) -> i32;
    fn sysconf(name: i32) -> i64;
    fn getuid() -> u32;
    fn getpid() -> i32;
}

const SC_CLK_TCK: i32 = 2;
const SC_PAGESIZE: i32 = 30;

pub const SIGKILL: i32 = 9;
pub const SIGTERM: i32 = 15;
pub const SIGCONT: i32 = 18;
pub const SIGSTOP: i32 = 19;

#[derive(Debug, Clone, Copy, Default)]
pub struct FsStat {
    pub total: u64,
    pub free: u64,
    pub avail: u64,
    pub files: u64,
    pub files_free: u64,
}

pub fn fs_stat(path: &str) -> Option<FsStat> {
    let c = CString::new(path).ok()?;
    let mut s = StatVfs::default();
    // SAFETY: `c` é uma string C válida e `s` tem o layout de `struct statvfs`
    // da glibc em plataformas de 64 bits.
    let rc = unsafe { statvfs(c.as_ptr(), &mut s) };
    if rc != 0 {
        return None;
    }
    let fr = if s.f_frsize > 0 {
        s.f_frsize
    } else {
        s.f_bsize
    };
    Some(FsStat {
        total: s.f_blocks * fr,
        free: s.f_bfree * fr,
        avail: s.f_bavail * fr,
        files: s.f_files,
        files_free: s.f_ffree,
    })
}

/// Envia um sinal. Retorna o erro do SO em caso de falha.
pub fn send_signal(pid: i32, sig: i32) -> Result<(), String> {
    if pid <= 1 {
        return Err("PID inválido".into());
    }
    // SAFETY: chamada simples de sistema sem ponteiros.
    let rc = unsafe { kill(pid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

pub fn renice(pid: i32, nice: i32) -> Result<(), String> {
    if pid <= 1 {
        return Err("PID inválido".into());
    }
    // SAFETY: PRIO_PROCESS = 0; sem ponteiros.
    let rc = unsafe { setpriority(0, pid as u32, nice.clamp(-20, 19)) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

pub fn clock_ticks() -> f64 {
    // SAFETY: sysconf é segura para constantes conhecidas.
    let v = unsafe { sysconf(SC_CLK_TCK) };
    if v > 0 {
        v as f64
    } else {
        100.0
    }
}

pub fn page_size() -> u64 {
    // SAFETY: idem.
    let v = unsafe { sysconf(SC_PAGESIZE) };
    if v > 0 {
        v as u64
    } else {
        4096
    }
}

pub fn uid() -> u32 {
    // SAFETY: getuid nunca falha.
    unsafe { getuid() }
}

pub fn self_pid() -> i32 {
    // SAFETY: getpid nunca falha.
    unsafe { getpid() }
}

pub fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

pub fn read_trim(path: &str) -> Option<String> {
    read(path).map(|s| s.trim().to_string())
}

pub fn read_u64(path: &str) -> Option<u64> {
    read_trim(path)?.parse().ok()
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// `$XDG_CONFIG_HOME/pulse` ou `~/.config/pulse`.
pub fn config_dir() -> PathBuf {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(p) if !p.is_empty() => PathBuf::from(p).join("pulse"),
        _ => home().join(".config/pulse"),
    }
}

/// `$XDG_STATE_HOME/pulse` ou `~/.local/state/pulse`.
pub fn state_dir() -> PathBuf {
    match std::env::var_os("XDG_STATE_HOME") {
        Some(p) if !p.is_empty() => PathBuf::from(p).join("pulse"),
        _ => home().join(".local/state/pulse"),
    }
}

/// Mapa uid → nome lido de /etc/passwd.
pub fn users() -> HashMap<u32, String> {
    parse_passwd(&read("/etc/passwd").unwrap_or_default())
}

pub fn parse_passwd(src: &str) -> HashMap<u32, String> {
    let mut m = HashMap::new();
    for line in src.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() > 2 {
            if let Ok(uid) = f[2].parse() {
                m.insert(uid, f[0].to_string());
            }
        }
    }
    m
}

/// Formata um instante (ms desde a época) como ISO 8601 UTC.
pub fn iso_utc(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // algoritmo de Howard Hinnant (civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Executa um comando com tempo limite, devolvendo (status ok, stdout, stderr).
pub fn run_cmd(
    prog: &str,
    args: &[&str],
    timeout_ms: u64,
) -> Result<(bool, String, String), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C.UTF-8")
        .env("SYSTEMD_COLORS", "0")
        .env("SYSTEMD_PAGER", "")
        .spawn()
        .map_err(|e| format!("falha ao executar {}: {}", prog, e))?;
    // Lê as saídas em threads para não travar com buffers cheios.
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let t_out = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = so.read_to_end(&mut v);
        v
    });
    let t_err = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = se.read_to_end(&mut v);
        v
    });
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if start.elapsed().as_millis() as u64 > timeout_ms {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} excedeu o tempo limite", prog));
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let out = String::from_utf8_lossy(&t_out.join().unwrap_or_default()).into_owned();
    let err = String::from_utf8_lossy(&t_err.join().unwrap_or_default()).into_owned();
    Ok((status.success(), out, err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwd() {
        let m = parse_passwd(
            "root:x:0:0:root:/root:/bin/bash\nbad\nana:x:1000:1000::/home/ana:/bin/sh\n",
        );
        assert_eq!(m.get(&0).unwrap(), "root");
        assert_eq!(m.get(&1000).unwrap(), "ana");
    }

    #[test]
    fn iso() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_700_000_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn statvfs_root() {
        let s = fs_stat("/").expect("statvfs /");
        assert!(s.total > 0 && s.avail <= s.total);
    }

    #[test]
    fn run_cmd_timeout() {
        assert!(run_cmd("sleep", &["5"], 100).is_err());
        let (ok, out, _) = run_cmd("echo", &["oi"], 2000).unwrap();
        assert!(ok && out.trim() == "oi");
    }
}
