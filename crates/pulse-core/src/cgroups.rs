//! Consumo por unidade systemd e por contêiner a partir do cgroup v2
//! (`cpu.stat` → usage_usec, `memory.current`). Não precisa de root nem do Docker.

use crate::json::{self, Obj};
use crate::sys;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub struct UnitUsage {
    /// Nome da unidade (ex.: `cron.service`, `docker-<id>.scope`).
    pub unit: String,
    /// "system" ou "user".
    pub scope: &'static str,
    pub cpu_pct: Option<f64>,
    pub mem: u64,
    pub tasks: u64,
}

pub fn parse_usage_usec(cpu_stat: &str) -> Option<u64> {
    cpu_stat
        .lines()
        .find_map(|l| l.strip_prefix("usage_usec ")?.trim().parse().ok())
}

/// Valor de uma chave de `memory.stat`.
pub fn parse_stat_key(src: &str, key: &str) -> Option<u64> {
    src.lines().find_map(|l| {
        let (k, v) = l.split_once(' ')?;
        if k == key {
            v.trim().parse().ok()
        } else {
            None
        }
    })
}

/// ID do contêiner Docker de uma unidade `docker-<id>.scope`.
pub fn docker_id(unit: &str) -> Option<&str> {
    unit.strip_prefix("docker-")?.strip_suffix(".scope")
}

fn is_unit(name: &str) -> bool {
    name.ends_with(".service") || name.ends_with(".scope")
}

/// Diretórios de cgroup das unidades: filhos diretos de system.slice e as
/// unidades do gerenciador do usuário (app.slice, session.slice, background.slice).
fn unit_dirs(root: &Path, uid: u32) -> Vec<(PathBuf, &'static str)> {
    let mut v = Vec::new();
    let push_children =
        |dir: PathBuf, scope: &'static str, v: &mut Vec<(PathBuf, &'static str)>| {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().into_owned();
                    if is_unit(&n) && e.path().is_dir() {
                        v.push((e.path(), scope));
                    }
                }
            }
        };
    push_children(root.join("system.slice"), "system", &mut v);
    let user = root.join(format!("user.slice/user-{uid}.slice/user@{uid}.service"));
    for s in ["app.slice", "session.slice", "background.slice"] {
        push_children(user.join(s), "user", &mut v);
    }
    // apps gráficos ficam em sub-slices (app-gnome-*.slice/…)
    if let Ok(rd) = std::fs::read_dir(user.join("app.slice")) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().ends_with(".slice") {
                push_children(e.path(), "user", &mut v);
            }
        }
    }
    v
}

pub struct CgroupSampler {
    root: PathBuf,
    prev: HashMap<PathBuf, u64>,
    prev_at: Option<Instant>,
}

impl Default for CgroupSampler {
    fn default() -> Self {
        Self::new(Path::new("/sys/fs/cgroup"))
    }
}

impl CgroupSampler {
    pub fn new(root: &Path) -> Self {
        CgroupSampler {
            root: root.to_path_buf(),
            prev: HashMap::new(),
            prev_at: None,
        }
    }

    pub fn available(&self) -> bool {
        self.root.join("cgroup.controllers").exists()
    }

    /// Amostra todas as unidades. CPU% é a média desde a amostra anterior
    /// (None na primeira chamada).
    pub fn sample(&mut self) -> Vec<UnitUsage> {
        let now = Instant::now();
        let dt = self.prev_at.map(|t| now.duration_since(t).as_secs_f64());
        let mut next = HashMap::new();
        let mut out = Vec::new();
        for (dir, scope) in unit_dirs(&self.root, sys::uid()) {
            let usage = sys::read(&dir.join("cpu.stat").to_string_lossy())
                .and_then(|s| parse_usage_usec(&s));
            // Como o `docker stats`: memory.current menos o cache de arquivos inativo
            // (recuperável), senão leituras grandes de arquivos inflam o número.
            let current = sys::read_u64(&dir.join("memory.current").to_string_lossy()).unwrap_or(0);
            let inactive = sys::read(&dir.join("memory.stat").to_string_lossy())
                .and_then(|s| parse_stat_key(&s, "inactive_file"))
                .unwrap_or(0);
            let mem = current.saturating_sub(inactive);
            let tasks = sys::read_u64(&dir.join("pids.current").to_string_lossy()).unwrap_or(0);
            let cpu_pct = match (usage, self.prev.get(&dir), dt) {
                (Some(u), Some(p), Some(dt)) if dt > 0.0 && u >= *p => {
                    Some((u - p) as f64 / 1e6 / dt * 100.0)
                }
                _ => None,
            };
            if let Some(u) = usage {
                next.insert(dir.clone(), u);
            }
            let unit = dir
                .file_name()
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(UnitUsage {
                unit,
                scope,
                cpu_pct,
                mem,
                tasks,
            });
        }
        self.prev = next;
        self.prev_at = Some(now);
        out
    }
}

pub fn usage_json(v: &[UnitUsage]) -> String {
    json::arr(v.iter().map(|u| {
        Obj::new()
            .s("unit", &u.unit)
            .s("scope", u.scope)
            .opt_n("cpu", u.cpu_pct)
            .u("mem", u.mem)
            .u("tasks", u.tasks)
            .opt_s("container", docker_id(&u.unit))
            .finish()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpu_stat_and_docker_ids() {
        assert_eq!(
            parse_usage_usec("usage_usec 9542397\nuser_usec 1\n"),
            Some(9_542_397)
        );
        assert_eq!(parse_usage_usec("user_usec 1\n"), None);
        assert_eq!(
            parse_stat_key("anon 10\ninactive_file 7\n", "inactive_file"),
            Some(7)
        );
        assert_eq!(parse_stat_key("anon 10\n", "inactive_file"), None);
        assert_eq!(docker_id("docker-abc123.scope"), Some("abc123"));
        assert_eq!(docker_id("cron.service"), None);
    }

    #[test]
    fn samples_a_fake_cgroup_tree() {
        let root = std::env::temp_dir().join(format!("pulse-cg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let uid = sys::uid();
        let sysu = root.join("system.slice/cron.service");
        let useru = root.join(format!(
            "user.slice/user-{uid}.slice/user@{uid}.service/app.slice/app-x.slice/pulse.service"
        ));
        for d in [&sysu, &useru] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("cpu.stat"), "usage_usec 1000000\n").unwrap();
            std::fs::write(d.join("memory.current"), "5096\n").unwrap();
            std::fs::write(d.join("memory.stat"), "anon 4000\ninactive_file 1000\n").unwrap();
        }
        std::fs::write(root.join("cgroup.controllers"), "cpu memory\n").unwrap();
        let mut s = CgroupSampler::new(&root);
        assert!(s.available());
        let first = s.sample();
        assert_eq!(first.len(), 2);
        assert!(first.iter().all(|u| u.cpu_pct.is_none()));
        std::fs::write(sysu.join("cpu.stat"), "usage_usec 3000000\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = s.sample();
        let cron = second.iter().find(|u| u.unit == "cron.service").unwrap();
        assert_eq!(cron.scope, "system");
        assert!(cron.cpu_pct.unwrap() > 0.0);
        assert_eq!(cron.mem, 4096);
        assert!(second
            .iter()
            .any(|u| u.unit == "pulse.service" && u.scope == "user"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
