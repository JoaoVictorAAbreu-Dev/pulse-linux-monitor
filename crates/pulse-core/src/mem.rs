//! Memória: /proc/meminfo, /proc/vmstat e swappiness.

use crate::sys;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Default)]
pub struct Mem {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub buffers: u64,
    pub cached: u64,
    pub sreclaimable: u64,
    pub shmem: u64,
    pub dirty: u64,
    pub writeback: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub swap_cached: u64,
}

impl Mem {
    /// Memória em uso (total − disponível).
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
    /// Cache recuperável (page cache + buffers + slab recuperável − shmem).
    pub fn cache(&self) -> u64 {
        (self.cached + self.buffers + self.sreclaimable).saturating_sub(self.shmem)
    }
    /// Memória de aplicativos (total − livre − cache).
    pub fn apps(&self) -> u64 {
        self.total.saturating_sub(self.free).saturating_sub(self.cache())
    }
    pub fn swap_used(&self) -> u64 {
        self.swap_total.saturating_sub(self.swap_free)
    }
    pub fn used_pct(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.used() as f64 * 100.0 / self.total as f64 }
    }
    pub fn swap_pct(&self) -> f64 {
        if self.swap_total == 0 { 0.0 } else { self.swap_used() as f64 * 100.0 / self.swap_total as f64 }
    }
}

pub fn parse_meminfo(src: &str) -> Mem {
    let mut m: HashMap<&str, u64> = HashMap::new();
    for line in src.lines() {
        let mut it = line.split_whitespace();
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            let mult = if it.next() == Some("kB") { 1024 } else { 1 };
            if let Ok(n) = v.parse::<u64>() {
                m.insert(k.trim_end_matches(':'), n * mult);
            }
        }
    }
    let g = |k: &str| *m.get(k).unwrap_or(&0);
    let free = g("MemFree");
    Mem {
        total: g("MemTotal"),
        free,
        available: m.get("MemAvailable").copied().unwrap_or(free),
        buffers: g("Buffers"),
        cached: g("Cached"),
        sreclaimable: g("SReclaimable"),
        shmem: g("Shmem"),
        dirty: g("Dirty"),
        writeback: g("Writeback"),
        swap_total: g("SwapTotal"),
        swap_free: g("SwapFree"),
        swap_cached: g("SwapCached"),
    }
}

pub fn read() -> Mem {
    parse_meminfo(&sys::read("/proc/meminfo").unwrap_or_default())
}

/// Valor de uma chave de /proc/vmstat.
pub fn parse_vmstat(src: &str, key: &str) -> Option<u64> {
    src.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        if it.next() == Some(key) { it.next()?.parse().ok() } else { None }
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VmStat {
    pub oom_kill: u64,
    pub pswpin: u64,
    pub pswpout: u64,
    pub pgmajfault: u64,
}

pub fn vmstat() -> VmStat {
    let s = sys::read("/proc/vmstat").unwrap_or_default();
    VmStat {
        oom_kill: parse_vmstat(&s, "oom_kill").unwrap_or(0),
        pswpin: parse_vmstat(&s, "pswpin").unwrap_or(0),
        pswpout: parse_vmstat(&s, "pswpout").unwrap_or(0),
        pgmajfault: parse_vmstat(&s, "pgmajfault").unwrap_or(0),
    }
}

pub fn swappiness() -> Option<u64> {
    sys::read_u64("/proc/sys/vm/swappiness")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo() {
        let m = parse_meminfo(
            "MemTotal: 1000 kB\nMemFree: 200 kB\nMemAvailable: 600 kB\nBuffers: 50 kB\nCached: 300 kB\nSReclaimable: 20 kB\nShmem: 70 kB\nSwapTotal: 400 kB\nSwapFree: 100 kB\nDirty: 4 kB\n",
        );
        assert_eq!(m.total, 1000 * 1024);
        assert_eq!(m.used(), 400 * 1024);
        assert_eq!(m.cache(), 300 * 1024);
        assert_eq!(m.apps(), 500 * 1024);
        assert!((m.swap_pct() - 75.0).abs() < 1e-9);
        assert!((m.used_pct() - 40.0).abs() < 1e-9);
    }

    #[test]
    fn vmstat_key() {
        assert_eq!(parse_vmstat("nr_free 1\noom_kill 3\n", "oom_kill"), Some(3));
        assert_eq!(parse_vmstat("nr_free 1\n", "oom_kill"), None);
    }

    #[test]
    fn no_swap() {
        let m = parse_meminfo("MemTotal: 10 kB\nMemFree: 10 kB\n");
        assert_eq!(m.swap_pct(), 0.0);
        assert_eq!(m.available, m.free);
    }
}
