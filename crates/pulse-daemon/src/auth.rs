//! Token de acesso, comparação em tempo constante, validação de Host/Origin
//! e log de auditoria.

use pulse_core::sys;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const COOKIE: &str = "pulse_token";

/// Comparação em tempo constante (não para no primeiro byte diferente).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    // O comprimento do token não é segredo (sempre 64); o conteúdo é
    // comparado byte a byte sem desvio antecipado.
    let mut diff = 0u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        diff |= x ^ y;
    }
    diff == 0 && a.len() == b.len()
}

fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; bytes];
    fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{:02x}", b)).collect())
}

/// Lê o token ou cria um novo (diretório 0700, arquivo 0600).
pub fn load_or_create_token(path: &Path) -> Result<String, String> {
    if let Ok(s) = fs::read_to_string(path) {
        let t = s.trim().to_string();
        if t.len() >= 32 && t.chars().all(|c| c.is_ascii_hexdigit()) {
            // Garante permissões restritas mesmo se alguém as afrouxou.
            if let Ok(md) = fs::metadata(path) {
                if md.permissions().mode() & 0o077 != 0 {
                    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
                }
            }
            return Ok(t);
        }
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("criar {}: {}", dir.display(), e))?;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    let token = random_hex(32).map_err(|e| format!("gerar token: {}", e))?;
    let tmp = path.with_extension("tmp");
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("gravar token: {}", e))?;
    f.write_all(token.as_bytes()).and_then(|_| f.write_all(b"\n")).map_err(|e| e.to_string())?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(token)
}

pub fn token_path() -> PathBuf {
    sys::config_dir().join("token")
}

/// Hosts aceitos no cabeçalho `Host` (defesa contra DNS rebinding).
pub fn allowed_hosts(port: u16, bind: std::net::IpAddr, allow_remote: bool) -> Vec<String> {
    let mut v = vec![
        format!("127.0.0.1:{}", port),
        format!("localhost:{}", port),
        format!("[::1]:{}", port),
    ];
    if allow_remote && !bind.is_unspecified() && !bind.is_loopback() {
        v.push(match bind {
            std::net::IpAddr::V6(ip) => format!("[{}]:{}", ip, port),
            ip => format!("{}:{}", ip, port),
        });
    }
    if allow_remote {
        if let Some(h) = sys::read_trim("/proc/sys/kernel/hostname") {
            v.push(format!("{}:{}", h.to_lowercase(), port));
            v.push(format!("{}.local:{}", h.to_lowercase(), port));
        }
    }
    v
}

pub fn host_ok(host: Option<&str>, allowed: &[String]) -> bool {
    match host {
        Some(h) => {
            let h = h.trim().to_lowercase();
            allowed.iter().any(|a| *a == h)
        }
        None => false,
    }
}

/// `Origin`, quando presente, precisa ser `http://<host permitido>`.
pub fn origin_ok(origin: Option<&str>, allowed: &[String]) -> bool {
    match origin {
        None => true,
        Some(o) => match o.trim().to_lowercase().strip_prefix("http://") {
            Some(rest) => allowed.iter().any(|a| a == rest),
            None => false,
        },
    }
}

pub fn cookie_value<'a>(cookie_header: Option<&'a str>, name: &str) -> Option<&'a str> {
    cookie_header?.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        if k == name { Some(v) } else { None }
    })
}

pub fn set_cookie(token: &str) -> String {
    format!("{}={}; Path=/; HttpOnly; SameSite=Strict", COOKIE, token)
}

pub fn clear_cookie() -> String {
    format!("{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0", COOKIE)
}

/// Registra uma ação sensível. Nunca grava segredos.
pub fn audit(action: &str, target: &str, result: &str, peer: &str) {
    let dir = sys::state_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let line = format!(
        "{} peer={} action={} target={:?} result={:?}\n",
        sys::iso_utc(sys::now_ms()),
        peer,
        action,
        target,
        result
    );
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).mode(0o600).open(dir.join("audit.log")) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"a"));
        assert!(ct_eq(b"", b""));
        // comprimentos que diferem por múltiplos de 256 também falham
        assert!(!ct_eq(&[0u8; 256], &[]));
    }

    #[test]
    fn hosts_and_origins() {
        let a = allowed_hosts(7878, "127.0.0.1".parse().unwrap(), false);
        assert!(host_ok(Some("127.0.0.1:7878"), &a));
        assert!(host_ok(Some("LOCALHOST:7878"), &a));
        assert!(!host_ok(Some("evil.com:7878"), &a));
        assert!(!host_ok(Some("127.0.0.1:80"), &a));
        assert!(!host_ok(None, &a));
        assert!(origin_ok(None, &a));
        assert!(origin_ok(Some("http://localhost:7878"), &a));
        assert!(!origin_ok(Some("http://evil.com"), &a));
        assert!(!origin_ok(Some("null"), &a));
        assert!(!origin_ok(Some("https://localhost:7878"), &a));
    }

    #[test]
    fn cookies() {
        assert_eq!(cookie_value(Some("a=1; pulse_token=xyz; b=2"), COOKIE), Some("xyz"));
        assert_eq!(cookie_value(Some("a=1"), COOKIE), None);
        assert!(set_cookie("t").contains("HttpOnly; SameSite=Strict"));
    }

    #[test]
    fn token_file_is_private() {
        let dir = std::env::temp_dir().join(format!("pulse-token-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let p = dir.join("token");
        let t = load_or_create_token(&p).unwrap();
        assert_eq!(t.len(), 64);
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(load_or_create_token(&p).unwrap(), t);
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        load_or_create_token(&p).unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }
}
