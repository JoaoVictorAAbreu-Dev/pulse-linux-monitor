//! CPU: /proc/stat, /proc/loadavg, PSI e cpufreq.

use crate::sys;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Times {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl Times {
    pub fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq + self.steal
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stat {
    pub total: Times,
    pub cores: Vec<Times>,
    pub ctxt: u64,
    pub intr: u64,
    pub procs_running: u64,
    pub procs_blocked: u64,
}

pub fn parse_stat(src: &str) -> Stat {
    let mut st = Stat::default();
    for line in src.lines() {
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        let nums: Vec<u64> = it.filter_map(|x| x.parse().ok()).collect();
        let times = || Times {
            user: *nums.first().unwrap_or(&0),
            nice: *nums.get(1).unwrap_or(&0),
            system: *nums.get(2).unwrap_or(&0),
            idle: *nums.get(3).unwrap_or(&0),
            iowait: *nums.get(4).unwrap_or(&0),
            irq: *nums.get(5).unwrap_or(&0),
            softirq: *nums.get(6).unwrap_or(&0),
            steal: *nums.get(7).unwrap_or(&0),
        };
        match key {
            "cpu" => st.total = times(),
            k if k.starts_with("cpu") => st.cores.push(times()),
            "ctxt" => st.ctxt = *nums.first().unwrap_or(&0),
            "intr" => st.intr = *nums.first().unwrap_or(&0),
            "procs_running" => st.procs_running = *nums.first().unwrap_or(&0),
            "procs_blocked" => st.procs_blocked = *nums.first().unwrap_or(&0),
            _ => {}
        }
    }
    st
}

/// Percentuais entre duas leituras.
#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub total: f64,
    pub user: f64,
    pub system: f64,
    pub iowait: f64,
    pub irq: f64,
    pub steal: f64,
    pub idle: f64,
}

pub fn usage(a: &Times, b: &Times) -> Usage {
    let dt = b.total().saturating_sub(a.total()) as f64;
    if dt <= 0.0 {
        return Usage::default();
    }
    let p = |x: u64, y: u64| (y.saturating_sub(x) as f64) * 100.0 / dt;
    let idle = p(a.idle, b.idle);
    let iowait = p(a.iowait, b.iowait);
    Usage {
        total: (100.0 - idle - iowait).clamp(0.0, 100.0),
        user: p(a.user + a.nice, b.user + b.nice),
        system: p(a.system, b.system),
        iowait,
        irq: p(a.irq + a.softirq, b.irq + b.softirq),
        steal: p(a.steal, b.steal),
        idle,
    }
}

pub fn read_stat() -> Stat {
    parse_stat(&sys::read("/proc/stat").unwrap_or_default())
}

pub fn loadavg() -> [f64; 3] {
    let s = sys::read("/proc/loadavg").unwrap_or_default();
    let v: Vec<f64> = s.split_whitespace().take(3).filter_map(|x| x.parse().ok()).collect();
    [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0)]
}

/// Pressure Stall Information: (some avg10, full avg10).
#[derive(Debug, Clone, Copy, Default)]
pub struct Psi {
    pub some10: f64,
    pub some60: f64,
    pub full10: Option<f64>,
    pub available: bool,
}

pub fn parse_psi(src: &str) -> Psi {
    let mut p = Psi { available: !src.is_empty(), ..Default::default() };
    for line in src.lines() {
        let mut parts = line.split_whitespace();
        let kind = parts.next().unwrap_or("");
        let mut a10 = None;
        let mut a60 = None;
        for kv in parts {
            if let Some(v) = kv.strip_prefix("avg10=") {
                a10 = v.parse().ok();
            } else if let Some(v) = kv.strip_prefix("avg60=") {
                a60 = v.parse().ok();
            }
        }
        match kind {
            "some" => {
                p.some10 = a10.unwrap_or(0.0);
                p.some60 = a60.unwrap_or(0.0);
            }
            "full" => p.full10 = a10,
            _ => {}
        }
    }
    p
}

pub fn psi(resource: &str) -> Psi {
    parse_psi(&sys::read(&format!("/proc/pressure/{}", resource)).unwrap_or_default())
}

pub fn core_mhz(core: usize) -> Option<f64> {
    sys::read_u64(&format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", core)).map(|khz| khz as f64 / 1000.0)
}

pub fn governor() -> Option<String> {
    sys::read_trim("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
}

pub fn model_name() -> String {
    let s = sys::read("/proc/cpuinfo").unwrap_or_default();
    s.lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|x| x.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT_A: &str = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 50 0 25 400 25 0 0 0\ncpu1 50 0 25 400 25 0 0 0\nintr 999 1 2\nctxt 5000\nprocs_running 3\nprocs_blocked 1\n";
    const STAT_B: &str = "cpu  200 0 100 1600 100 0 0 0 0 0\ncpu0 100 0 50 800 50 0 0 0\ncpu1 100 0 50 800 50 0 0 0\nctxt 6000\n";

    #[test]
    fn parses_and_computes_usage() {
        let a = parse_stat(STAT_A);
        let b = parse_stat(STAT_B);
        assert_eq!(a.cores.len(), 2);
        assert_eq!(a.ctxt, 5000);
        assert_eq!(a.procs_running, 3);
        let u = usage(&a.total, &b.total);
        // delta: user 100, sys 50, idle 800, iowait 50 → total 1000
        assert!((u.total - 15.0).abs() < 1e-9);
        assert!((u.user - 10.0).abs() < 1e-9);
        assert!((u.iowait - 5.0).abs() < 1e-9);
    }

    #[test]
    fn usage_no_delta_is_zero() {
        let a = parse_stat(STAT_A);
        assert_eq!(usage(&a.total, &a.total).total, 0.0);
    }

    #[test]
    fn psi_parse() {
        let p = parse_psi("some avg10=1.50 avg60=0.80 avg300=0.10 total=123\nfull avg10=0.25 avg60=0.00 avg300=0.00 total=5\n");
        assert!(p.available);
        assert_eq!(p.some10, 1.5);
        assert_eq!(p.full10, Some(0.25));
        assert!(!parse_psi("").available);
    }
}
