//! Processos: /proc/[pid]/{stat,cmdline} e uso de CPU entre amostras.

use crate::json::{self, Obj};
use crate::sys;
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;

#[derive(Debug, Clone, Default)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub name: String,
    pub cmd: String,
    pub state: char,
    pub uid: u32,
    pub user: String,
    pub cpu: f64,
    pub rss: u64,
    pub threads: u64,
    pub nice: i64,
    pub start_ticks: u64,
    pub ticks: u64,
}

impl Proc {
    pub fn to_json(&self) -> String {
        Obj::new()
            .i("pid", self.pid as i64)
            .i("ppid", self.ppid as i64)
            .s("name", &self.name)
            .s("cmd", &self.cmd)
            .s("state", &self.state.to_string())
            .s("user", &self.user)
            .n("cpu", self.cpu)
            .u("rss", self.rss)
            .u("threads", self.threads)
            .i("nice", self.nice)
            .finish()
    }
}

/// Campos de /proc/[pid]/stat (o nome pode conter espaços e parênteses).
pub struct StatFields {
    pub name: String,
    pub state: char,
    pub ppid: i32,
    pub ticks: u64,
    pub nice: i64,
    pub threads: u64,
    pub start_ticks: u64,
    pub rss_pages: u64,
}

pub fn parse_pid_stat(src: &str) -> Option<StatFields> {
    let open = src.find('(')?;
    let close = src.rfind(')')?;
    let name = src.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = src.get(close + 2..)?.split_whitespace().collect();
    if rest.len() < 22 {
        return None;
    }
    let n = |i: usize| rest[i].parse::<u64>().unwrap_or(0);
    Some(StatFields {
        name,
        state: rest[0].chars().next().unwrap_or('?'),
        ppid: rest[1].parse().unwrap_or(0),
        ticks: n(11) + n(12),
        nice: rest[16].parse().unwrap_or(0),
        threads: n(17),
        start_ticks: n(19),
        rss_pages: n(21),
    })
}

pub fn cmdline(pid: i32) -> String {
    match std::fs::read(format!("/proc/{}/cmdline", pid)) {
        Ok(b) => {
            let s: String = String::from_utf8_lossy(&b).replace('\0', " ");
            let s = s.trim().to_string();
            if s.len() > 600 {
                format!(
                    "{}…",
                    &s[..s.char_indices().nth(600).map(|x| x.0).unwrap_or(s.len())]
                )
            } else {
                s
            }
        }
        Err(_) => String::new(),
    }
}

pub fn cwd(pid: i32) -> Option<String> {
    std::fs::read_link(format!("/proc/{}/cwd", pid))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn exe(pid: i32) -> Option<String> {
    std::fs::read_link(format!("/proc/{}/exe", pid))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Lê starttime de um PID (para evitar agir sobre PID reutilizado).
pub fn start_ticks(pid: i32) -> Option<u64> {
    parse_pid_stat(&sys::read(&format!("/proc/{}/stat", pid))?).map(|s| s.start_ticks)
}

pub struct Sampler {
    prev: HashMap<i32, (u64, u64)>,
    prev_at: std::time::Instant,
    users: HashMap<u32, String>,
    users_at: std::time::Instant,
    clk: f64,
    page: u64,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        Sampler {
            prev: HashMap::new(),
            prev_at: std::time::Instant::now(),
            users: sys::users(),
            users_at: std::time::Instant::now(),
            clk: sys::clock_ticks(),
            page: sys::page_size(),
        }
    }

    pub fn sample(&mut self) -> Vec<Proc> {
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.prev_at).as_secs_f64().max(0.001);
        if self.users_at.elapsed().as_secs() > 60 {
            self.users = sys::users();
            self.users_at = now;
        }
        let mut out = Vec::new();
        let mut next = HashMap::new();
        let Ok(rd) = std::fs::read_dir("/proc") else {
            return out;
        };
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            let Some(st) =
                sys::read(&format!("/proc/{}/stat", pid)).and_then(|s| parse_pid_stat(&s))
            else {
                continue;
            };
            let uid = e.metadata().map(|m| m.uid()).unwrap_or(0);
            let cpu = match self.prev.get(&pid) {
                Some((t, start)) if *start == st.start_ticks => {
                    (st.ticks.saturating_sub(*t) as f64 / self.clk) / dt * 100.0
                }
                _ => 0.0,
            };
            next.insert(pid, (st.ticks, st.start_ticks));
            let cmd = cmdline(pid);
            out.push(Proc {
                pid,
                ppid: st.ppid,
                cmd: if cmd.is_empty() {
                    format!("[{}]", st.name)
                } else {
                    cmd
                },
                name: st.name,
                state: st.state,
                uid,
                user: self
                    .users
                    .get(&uid)
                    .cloned()
                    .unwrap_or_else(|| uid.to_string()),
                cpu,
                rss: st.rss_pages * self.page,
                threads: st.threads,
                nice: st.nice,
                start_ticks: st.start_ticks,
                ticks: st.ticks,
            });
        }
        self.prev = next;
        self.prev_at = now;
        out
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Summary {
    pub total: u64,
    pub running: u64,
    pub sleeping: u64,
    pub zombie: u64,
    pub stopped: u64,
    pub threads: u64,
}

pub fn summarize(list: &[Proc]) -> Summary {
    let mut s = Summary {
        total: list.len() as u64,
        ..Default::default()
    };
    for p in list {
        s.threads += p.threads;
        match p.state {
            'R' => s.running += 1,
            'S' | 'D' | 'I' => s.sleeping += 1,
            'Z' => s.zombie += 1,
            'T' | 't' => s.stopped += 1,
            _ => {}
        }
    }
    s
}

pub fn summary_json(s: &Summary) -> String {
    Obj::new()
        .u("total", s.total)
        .u("running", s.running)
        .u("sleeping", s.sleeping)
        .u("zombie", s.zombie)
        .u("stopped", s.stopped)
        .u("threads", s.threads)
        .finish()
}

pub fn list_json(list: &[Proc]) -> String {
    json::arr(list.iter().map(|p| p.to_json()))
}

/// Maiores consumidores ordenados por CPU e depois memória.
pub fn top(list: &[Proc], n: usize) -> Vec<Proc> {
    let mut v: Vec<Proc> = list.iter().filter(|p| p.rss > 0).cloned().collect();
    v.sort_by(|a, b| {
        b.cpu
            .partial_cmp(&a.cpu)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.rss.cmp(&a.rss))
    });
    v.truncate(n);
    v
}

pub fn top_mem(list: &[Proc], n: usize) -> Vec<Proc> {
    let mut v: Vec<Proc> = list.to_vec();
    v.sort_by_key(|p| std::cmp::Reverse(p.rss));
    v.truncate(n);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_with_weird_name() {
        let src = "1234 (my (weird) proc) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 5 7 0 99999 1000000 300 18446744073709551615";
        let f = parse_pid_stat(src).unwrap();
        assert_eq!(f.name, "my (weird) proc");
        assert_eq!(f.state, 'S');
        assert_eq!(f.ppid, 1);
        assert_eq!(f.ticks, 300);
        assert_eq!(f.nice, 5);
        assert_eq!(f.threads, 7);
        assert_eq!(f.start_ticks, 99999);
        assert_eq!(f.rss_pages, 300);
    }

    #[test]
    fn sampler_sees_self() {
        let mut s = Sampler::new();
        let list = s.sample();
        let me = sys::self_pid();
        assert!(list.iter().any(|p| p.pid == me));
        let sum = summarize(&list);
        assert!(sum.total > 0);
    }
}
