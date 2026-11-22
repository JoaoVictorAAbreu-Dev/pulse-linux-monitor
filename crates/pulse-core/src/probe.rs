//! Resultado da medição eBPF sob demanda (`pulse-helper netprobe`): bytes de
//! rede por processo e latência dos discos, a partir da saída `-f json` do
//! bpftrace (uma linha JSON por mapa).

use crate::json::{self, Obj, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetProc {
    pub pid: i64,
    pub comm: String,
    pub tx: u64,
    pub rx: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProbeResult {
    pub procs: Vec<NetProc>,
    pub disk_ops: u64,
    pub disk_avg_us: Option<f64>,
    pub disk_max_us: Option<u64>,
}

/// Chave "pid,comm" (o comm pode conter vírgulas: divide só na primeira).
fn split_key(k: &str) -> Option<(i64, String)> {
    let (p, c) = k.split_once(',')?;
    Some((p.trim().parse().ok()?, c.to_string()))
}

pub fn parse_bpftrace(src: &str) -> ProbeResult {
    let mut per: BTreeMap<(i64, String), NetProc> = BTreeMap::new();
    let mut r = ProbeResult::default();
    let mut sum = 0u64;
    for line in src.lines() {
        let Ok(v) = json::parse(line.trim()) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("map") {
            continue;
        }
        let Some(Value::Obj(maps)) = v.get("data") else {
            continue;
        };
        for (name, val) in maps {
            match (name.as_str(), val) {
                ("@tx" | "@rx", Value::Obj(entries)) => {
                    for (k, n) in entries {
                        let Some((pid, comm)) = split_key(k) else {
                            continue;
                        };
                        let bytes = n.as_f64().unwrap_or(0.0).max(0.0) as u64;
                        let e = per.entry((pid, comm.clone())).or_insert(NetProc {
                            pid,
                            comm,
                            ..Default::default()
                        });
                        if name == "@tx" {
                            e.tx += bytes;
                        } else {
                            e.rx += bytes;
                        }
                    }
                }
                ("@lat_count", n) => r.disk_ops = n.as_f64().unwrap_or(0.0) as u64,
                ("@lat_sum", n) => sum = n.as_f64().unwrap_or(0.0) as u64,
                ("@lat_max", n) => r.disk_max_us = n.as_f64().map(|x| x as u64),
                _ => {}
            }
        }
    }
    if r.disk_ops > 0 {
        r.disk_avg_us = Some(sum as f64 / r.disk_ops as f64);
    }
    r.procs = per.into_values().collect();
    r.procs.sort_by_key(|p| std::cmp::Reverse(p.tx + p.rx));
    r
}

pub fn to_json(r: &ProbeResult, secs: u32) -> String {
    Obj::new()
        .u("seconds", secs as u64)
        .raw(
            "procs",
            &json::arr(r.procs.iter().take(50).map(|p| {
                Obj::new()
                    .i("pid", p.pid)
                    .s("comm", &p.comm)
                    .u("tx", p.tx)
                    .u("rx", p.rx)
                    .n("tx_bps", p.tx as f64 / secs.max(1) as f64)
                    .n("rx_bps", p.rx as f64 / secs.max(1) as f64)
                    .finish()
            })),
        )
        .u("disk_ops", r.disk_ops)
        .opt_n("disk_avg_us", r.disk_avg_us)
        .opt_n("disk_max_us", r.disk_max_us.map(|x| x as f64))
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_bpftrace_output() {
        // trecho da saída real do bpftrace 0.20 neste projeto
        let src = r#"

{"type": "map", "data": {"@lat_count": 96}}
{"type": "map", "data": {"@lat_max": 89300}}
{"type": "map", "data": {"@lat_sum": 2158997}}
{"type": "map", "data": {"@rx": {"255769,http": 420, "256355,pulse-tray": 65848, "128017,Chrome_ChildIOT": 2928346, "9,a,b": 1}}}
{"type": "map", "data": {"@tx": {"256355,pulse-tray": 420, "128017,Chrome_ChildIOT": 2669, "255769,http": 65848}}}
lixo
"#;
        let r = parse_bpftrace(src);
        assert_eq!(r.disk_ops, 96);
        assert_eq!(r.disk_max_us, Some(89_300));
        assert!((r.disk_avg_us.unwrap() - 22_489.55).abs() < 0.01);
        assert_eq!(r.procs[0].comm, "Chrome_ChildIOT");
        assert_eq!((r.procs[0].tx, r.procs[0].rx), (2669, 2_928_346));
        assert!(r.procs.iter().any(|p| p.comm == "a,b"), "comm com vírgula");
        let j = to_json(&r, 10);
        assert!(json::parse(&j).is_ok());
        assert!(j.contains("\"rx_bps\":292834.6"));
        assert_eq!(parse_bpftrace("").disk_avg_us, None);
    }
}
