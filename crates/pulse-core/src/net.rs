//! Rede: /proc/net/dev, /sys/class/net, /proc/net/{tcp,tcp6,udp,udp6}.

use crate::json::{self, Obj};
use crate::sys;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IfCounters {
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errs: u64,
    pub rx_drop: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errs: u64,
    pub tx_drop: u64,
}

pub fn parse_net_dev(src: &str) -> Vec<(String, IfCounters)> {
    let mut v = Vec::new();
    for line in src.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else { continue };
        let n: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if n.len() < 16 {
            continue;
        }
        v.push((
            name.trim().to_string(),
            IfCounters {
                rx_bytes: n[0],
                rx_packets: n[1],
                rx_errs: n[2],
                rx_drop: n[3],
                tx_bytes: n[8],
                tx_packets: n[9],
                tx_errs: n[10],
                tx_drop: n[11],
            },
        ));
    }
    v
}

pub fn read_ifaces() -> Vec<(String, IfCounters)> {
    parse_net_dev(&sys::read("/proc/net/dev").unwrap_or_default())
}

#[derive(Debug, Clone, Default)]
pub struct IfInfo {
    pub name: String,
    pub state: String,
    pub speed_mbps: Option<i64>,
    pub mac: String,
    pub mtu: u64,
    pub c: IfCounters,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub virtual_if: bool,
}

impl IfInfo {
    pub fn to_json(&self) -> String {
        Obj::new()
            .s("name", &self.name)
            .s("state", &self.state)
            .opt_n("speed_mbps", self.speed_mbps.map(|x| x as f64))
            .s("mac", &self.mac)
            .u("mtu", self.mtu)
            .b("virtual", self.virtual_if)
            .u("rx_bytes", self.c.rx_bytes)
            .u("tx_bytes", self.c.tx_bytes)
            .u("rx_errs", self.c.rx_errs + self.c.rx_drop)
            .u("tx_errs", self.c.tx_errs + self.c.tx_drop)
            .n("rx_bps", self.rx_bps)
            .n("tx_bps", self.tx_bps)
            .finish()
    }
}

pub fn iface_info(prev: &[(String, IfCounters)], cur: &[(String, IfCounters)], dt_s: f64) -> Vec<IfInfo> {
    let pm: HashMap<&str, &IfCounters> = prev.iter().map(|(n, c)| (n.as_str(), c)).collect();
    cur.iter()
        .map(|(name, c)| {
            let p = pm.get(name.as_str()).copied().copied().unwrap_or(*c);
            let base = format!("/sys/class/net/{}", name);
            let speed = sys::read_trim(&format!("{}/speed", base)).and_then(|s| s.parse::<i64>().ok()).filter(|s| *s > 0);
            let virtual_if = std::fs::read_link(&base)
                .map(|t| t.to_string_lossy().contains("/virtual/"))
                .unwrap_or(name == "lo");
            IfInfo {
                name: name.clone(),
                state: sys::read_trim(&format!("{}/operstate", base)).unwrap_or_else(|| "unknown".into()),
                speed_mbps: speed,
                mac: sys::read_trim(&format!("{}/address", base)).unwrap_or_default(),
                mtu: sys::read_u64(&format!("{}/mtu", base)).unwrap_or(0),
                c: *c,
                rx_bps: c.rx_bytes.saturating_sub(p.rx_bytes) as f64 / dt_s.max(0.001),
                tx_bps: c.tx_bytes.saturating_sub(p.tx_bytes) as f64 / dt_s.max(0.001),
                virtual_if,
            }
        })
        .collect()
}

pub const TCP_STATES: [&str; 12] = [
    "?", "ESTABLISHED", "SYN_SENT", "SYN_RECV", "FIN_WAIT1", "FIN_WAIT2", "TIME_WAIT", "CLOSE", "CLOSE_WAIT",
    "LAST_ACK", "LISTEN", "CLOSING",
];

#[derive(Debug, Clone)]
pub struct Sock {
    pub proto: &'static str,
    pub local: String,
    pub port: u16,
    pub state: u8,
    pub inode: u64,
    pub loopback: bool,
}

fn parse_addr(hex: &str, v6: bool) -> Option<(String, u16, bool)> {
    let (a, p) = hex.split_once(':')?;
    let port = u16::from_str_radix(p, 16).ok()?;
    if !v6 {
        let n = u32::from_str_radix(a, 16).ok()?;
        let ip = Ipv4Addr::from(n.to_le_bytes());
        return Some((ip.to_string(), port, ip.is_loopback()));
    }
    if a.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for w in 0..4 {
        let word = u32::from_str_radix(&a[w * 8..w * 8 + 8], 16).ok()?;
        bytes[w * 4..w * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let ip = Ipv6Addr::from(bytes);
    let loop_ = ip.is_loopback() || ip.to_ipv4_mapped().map(|x| x.is_loopback()).unwrap_or(false);
    Some((ip.to_string(), port, loop_))
}

pub fn parse_proc_net(src: &str, proto: &'static str) -> Vec<Sock> {
    let v6 = proto.ends_with('6');
    let mut v = Vec::new();
    for line in src.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 {
            continue;
        }
        let Some((local, port, loopback)) = parse_addr(f[1], v6) else { continue };
        let state = u8::from_str_radix(f[3], 16).unwrap_or(0);
        let inode = f[9].parse().unwrap_or(0);
        v.push(Sock { proto, local, port, state, inode, loopback });
    }
    v
}

pub fn read_sockets() -> Vec<Sock> {
    let mut v = Vec::new();
    for p in ["tcp", "tcp6", "udp", "udp6"] {
        let proto: &'static str = p;
        v.extend(parse_proc_net(&sys::read(&format!("/proc/net/{}", p)).unwrap_or_default(), proto));
    }
    v
}

/// Contagem de conexões TCP por estado.
pub fn tcp_state_counts(socks: &[Sock]) -> Vec<(&'static str, u64)> {
    let mut counts = [0u64; 12];
    for s in socks.iter().filter(|s| s.proto.starts_with("tcp")) {
        if (s.state as usize) < counts.len() {
            counts[s.state as usize] += 1;
        }
    }
    TCP_STATES.iter().enumerate().skip(1).map(|(i, n)| (*n, counts[i])).filter(|(_, c)| *c > 0).collect()
}

/// Mapa inode de socket → (pid, nome) varrendo /proc/*/fd (só processos legíveis).
pub fn socket_owners() -> HashMap<u64, (i32, String)> {
    let mut m = HashMap::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return m };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else { continue };
        let Ok(fds) = std::fs::read_dir(format!("/proc/{}/fd", pid)) else { continue };
        let mut name: Option<String> = None;
        for fd in fds.flatten() {
            let Ok(t) = std::fs::read_link(fd.path()) else { continue };
            let t = t.to_string_lossy();
            if let Some(n) = t.strip_prefix("socket:[").and_then(|x| x.strip_suffix(']')) {
                if let Ok(ino) = n.parse::<u64>() {
                    let nm = name.get_or_insert_with(|| sys::read_trim(&format!("/proc/{}/comm", pid)).unwrap_or_default());
                    m.insert(ino, (pid, nm.clone()));
                }
            }
        }
    }
    m
}

/// Portas em escuta (TCP LISTEN e UDP vinculadas) com processo dono.
pub fn listening_json(socks: &[Sock], owners: &HashMap<u64, (i32, String)>) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut items: Vec<&Sock> = socks
        .iter()
        .filter(|s| (s.proto.starts_with("tcp") && s.state == 10) || (s.proto.starts_with("udp") && s.state == 7))
        .filter(|s| seen.insert((s.proto, s.local.clone(), s.port)))
        .collect();
    items.sort_by_key(|s| (s.port, s.proto));
    json::arr(items.into_iter().map(|s| {
        let owner = owners.get(&s.inode);
        Obj::new()
            .s("proto", s.proto)
            .s("addr", &s.local)
            .u("port", s.port as u64)
            .opt_n("pid", owner.map(|o| o.0 as f64))
            .opt_s("process", owner.map(|o| o.1.as_str()))
            .s("scope", if s.loopback { "local" } else { "rede" })
            .finish()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_dev() {
        let src = "Inter-|   Receive\n face |bytes\n  eth0: 1000 10 1 0 0 0 0 0 2000 20 2 0 0 0 0 0\n    lo: 5 1 0 0 0 0 0 0 5 1 0 0 0 0 0 0\n";
        let v = parse_net_dev(src);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, "eth0");
        assert_eq!(v[0].1.tx_bytes, 2000);
        assert_eq!(v[0].1.rx_errs, 1);
    }

    #[test]
    fn tcp_v4_and_v6() {
        let v4 = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1EC6 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 12345 1\n   1: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 999 1\n   2: 0100007F:9C40 0100007F:1EC6 01 00000000:00000000 00:00000000 00000000  1000        0 1 1\n";
        let s = parse_proc_net(v4, "tcp");
        assert_eq!(s[0].local, "127.0.0.1");
        assert_eq!(s[0].port, 7878);
        assert!(s[0].loopback);
        assert_eq!(s[1].local, "0.0.0.0");
        assert!(!s[1].loopback);
        let counts = tcp_state_counts(&s);
        assert!(counts.contains(&("LISTEN", 2)));
        assert!(counts.contains(&("ESTABLISHED", 1)));
        let v6 = "  sl  local_address                         remote_address                        st\n   0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 777 1\n";
        let s6 = parse_proc_net(v6, "tcp6");
        assert_eq!(s6[0].local, "::1");
        assert!(s6[0].loopback);
        let mut owners = HashMap::new();
        owners.insert(12345, (42, "pulse".to_string()));
        let j = listening_json(&s, &owners);
        assert!(j.contains("\"process\":\"pulse\""));
        assert!(j.contains("\"scope\":\"rede\""));
    }
}
