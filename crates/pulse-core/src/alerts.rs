//! Regras de alerta avaliadas a cada amostra.

use crate::json::{self, Obj};
use std::collections::{BTreeMap, HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Critical,
}

impl Severity {
    pub fn name(&self) -> &'static str {
        match self {
            Severity::Warning => "aviso",
            Severity::Critical => "critico",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rules {
    pub cpu_pct: f64,
    pub cpu_samples: u32,
    pub mem_warn: f64,
    pub mem_crit: f64,
    pub swap_warn: f64,
    pub disk_warn: f64,
    pub disk_crit: f64,
    pub temp_crit: f64,
    pub proc_pct: f64,
    pub proc_samples: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            cpu_pct: 90.0,
            cpu_samples: 30,
            mem_warn: 90.0,
            mem_crit: 96.0,
            swap_warn: 60.0,
            disk_warn: 90.0,
            disk_crit: 97.0,
            temp_crit: 88.0,
            proc_pct: 95.0,
            proc_samples: 300,
        }
    }
}

impl Rules {
    pub fn to_json(&self) -> String {
        let r = |id: &str, title: &str, cond: &str, sev: &str| {
            Obj::new().s("id", id).s("title", title).s("condition", cond).s("severity", sev).finish()
        };
        json::arr(vec![
            r("cpu", "CPU alta", &format!("CPU > {}% por {} amostras seguidas", self.cpu_pct, self.cpu_samples), "aviso"),
            r("mem", "Memória alta", &format!("memória > {}%", self.mem_warn), "aviso"),
            r("mem", "Memória crítica", &format!("memória > {}%", self.mem_crit), "critico"),
            r("swap", "Swap alta", &format!("swap > {}%", self.swap_warn), "aviso"),
            r("disk", "Disco quase cheio", &format!("uso > {}%", self.disk_warn), "aviso"),
            r("disk", "Disco cheio", &format!("uso > {}%", self.disk_crit), "critico"),
            r("temp", "Temperatura alta", &format!("sensor > {} °C", self.temp_crit), "critico"),
            r(
                "proc",
                "Processo preso",
                &format!("processo > {}% CPU por {} amostras", self.proc_pct, self.proc_samples),
                "aviso",
            ),
        ])
    }
}

#[derive(Debug, Clone, Default)]
pub struct Input {
    pub ts: u64,
    pub cpu: f64,
    pub mem_pct: f64,
    pub swap_pct: f64,
    pub swap_total: u64,
    pub disks: Vec<(String, f64)>,
    pub temp_max: Option<f64>,
    /// (pid, início em ticks, nome, % CPU)
    pub procs: Vec<(i32, u64, String, f64)>,
}

#[derive(Debug, Clone)]
pub struct Alert {
    pub key: String,
    pub rule: &'static str,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub since: u64,
}

impl Alert {
    pub fn to_json(&self) -> String {
        Obj::new()
            .s("key", &self.key)
            .s("rule", self.rule)
            .s("severity", self.severity.name())
            .s("title", &self.title)
            .s("detail", &self.detail)
            .u("since", self.since)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub ts: u64,
    pub kind: &'static str, // "disparou" | "resolvido" | "agravou"
    pub alert: Alert,
}

pub struct Engine {
    pub rules: Rules,
    cpu_streak: u32,
    proc_streak: HashMap<(i32, u64), u32>,
    pub active: BTreeMap<String, Alert>,
    pub history: VecDeque<Event>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(Rules::default())
    }
}

impl Engine {
    pub fn new(rules: Rules) -> Self {
        Engine { rules, cpu_streak: 0, proc_streak: HashMap::new(), active: BTreeMap::new(), history: VecDeque::new() }
    }

    /// Avalia uma amostra; devolve alertas novos ou agravados (para notificar).
    pub fn evaluate(&mut self, i: &Input) -> Vec<Alert> {
        let r = self.rules.clone();
        let mut cur: Vec<(String, &'static str, Severity, String, String)> = Vec::new();

        self.cpu_streak = if i.cpu > r.cpu_pct { self.cpu_streak + 1 } else { 0 };
        if self.cpu_streak >= r.cpu_samples {
            cur.push(("cpu".into(), "cpu", Severity::Warning, "CPU alta".into(), format!("{:.0}% sustentado", i.cpu)));
        }
        if i.mem_pct > r.mem_crit {
            cur.push(("mem".into(), "mem", Severity::Critical, "Memória crítica".into(), format!("{:.0}% em uso", i.mem_pct)));
        } else if i.mem_pct > r.mem_warn {
            cur.push(("mem".into(), "mem", Severity::Warning, "Memória alta".into(), format!("{:.0}% em uso", i.mem_pct)));
        }
        if i.swap_total > 0 && i.swap_pct > r.swap_warn {
            cur.push(("swap".into(), "swap", Severity::Warning, "Swap alta".into(), format!("{:.0}% em uso", i.swap_pct)));
        }
        for (mount, pct) in &i.disks {
            let key = format!("disk:{}", mount);
            if *pct > r.disk_crit {
                cur.push((key, "disk", Severity::Critical, "Disco cheio".into(), format!("{} {:.0}%", mount, pct)));
            } else if *pct > r.disk_warn {
                cur.push((key, "disk", Severity::Warning, "Disco quase cheio".into(), format!("{} {:.0}%", mount, pct)));
            }
        }
        if let Some(t) = i.temp_max {
            if t > r.temp_crit {
                cur.push(("temp".into(), "temp", Severity::Critical, "Temperatura alta".into(), format!("{:.0} °C", t)));
            }
        }
        let mut next = HashMap::new();
        for (pid, start, name, cpu) in &i.procs {
            if *cpu > r.proc_pct {
                let n = self.proc_streak.get(&(*pid, *start)).copied().unwrap_or(0) + 1;
                next.insert((*pid, *start), n);
                if n >= r.proc_samples {
                    cur.push((
                        format!("proc:{}:{}", pid, start),
                        "proc",
                        Severity::Warning,
                        "Processo preso".into(),
                        format!("{} (PID {}) em {:.0}% de CPU", name, pid, cpu),
                    ));
                }
            }
        }
        self.proc_streak = next;

        let mut fired = Vec::new();
        let keys: std::collections::HashSet<String> = cur.iter().map(|c| c.0.clone()).collect();
        let resolved: Vec<String> = self.active.keys().filter(|k| !keys.contains(*k)).cloned().collect();
        for k in resolved {
            if let Some(a) = self.active.remove(&k) {
                self.push_event(Event { ts: i.ts, kind: "resolvido", alert: a });
            }
        }
        for (key, rule, sev, title, detail) in cur {
            match self.active.get_mut(&key) {
                Some(a) => {
                    let escalated = sev > a.severity;
                    a.severity = sev;
                    a.title = title;
                    a.detail = detail;
                    if escalated {
                        let a = a.clone();
                        self.push_event(Event { ts: i.ts, kind: "agravou", alert: a.clone() });
                        fired.push(a);
                    }
                }
                None => {
                    let a = Alert { key: key.clone(), rule, severity: sev, title, detail, since: i.ts };
                    self.active.insert(key, a.clone());
                    self.push_event(Event { ts: i.ts, kind: "disparou", alert: a.clone() });
                    fired.push(a);
                }
            }
        }
        fired
    }

    fn push_event(&mut self, e: Event) {
        if self.history.len() >= 300 {
            self.history.pop_front();
        }
        self.history.push_back(e);
    }

    pub fn active_json(&self) -> String {
        let mut v: Vec<&Alert> = self.active.values().collect();
        v.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.since.cmp(&b.since)));
        json::arr(v.into_iter().map(|a| a.to_json()))
    }

    pub fn history_json(&self) -> String {
        json::arr(self.history.iter().rev().map(|e| {
            Obj::new().u("ts", e.ts).s("kind", e.kind).raw("alert", &e.alert.to_json()).finish()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Input {
        Input { ts: 1, cpu: 10.0, mem_pct: 40.0, swap_pct: 0.0, swap_total: 0, ..Default::default() }
    }

    #[test]
    fn cpu_needs_streak() {
        let mut e = Engine::default();
        let mut i = base();
        i.cpu = 95.0;
        for n in 0..29 {
            assert!(e.evaluate(&i).is_empty(), "amostra {}", n);
        }
        assert_eq!(e.evaluate(&i).len(), 1);
        assert!(e.active.contains_key("cpu"));
        i.cpu = 10.0;
        e.evaluate(&i);
        assert!(e.active.is_empty());
        assert_eq!(e.history.back().unwrap().kind, "resolvido");
    }

    #[test]
    fn memory_escalates() {
        let mut e = Engine::default();
        let mut i = base();
        i.mem_pct = 92.0;
        assert_eq!(e.evaluate(&i)[0].severity, Severity::Warning);
        i.mem_pct = 97.0;
        let f = e.evaluate(&i);
        assert_eq!(f[0].severity, Severity::Critical);
        assert_eq!(e.active.len(), 1);
    }

    #[test]
    fn disk_swap_temp() {
        let mut e = Engine::default();
        let mut i = base();
        i.disks = vec![("/".into(), 91.0), ("/data".into(), 98.0), ("/ok".into(), 10.0)];
        i.swap_total = 100;
        i.swap_pct = 70.0;
        i.temp_max = Some(90.0);
        e.evaluate(&i);
        assert_eq!(e.active.len(), 4);
        assert_eq!(e.active["disk:/data"].severity, Severity::Critical);
        // swap desativada não alerta
        let mut e2 = Engine::default();
        let mut j = base();
        j.swap_pct = 99.0;
        e2.evaluate(&j);
        assert!(e2.active.is_empty());
    }

    #[test]
    fn stuck_process() {
        let mut e = Engine::default();
        let mut i = base();
        i.procs = vec![(42, 7, "loop".into(), 99.0)];
        for _ in 0..299 {
            e.evaluate(&i);
        }
        assert!(e.active.is_empty());
        e.evaluate(&i);
        assert!(e.active.contains_key("proc:42:7"));
        // PID reutilizado (outro start) zera a contagem
        i.procs = vec![(42, 8, "outro".into(), 99.0)];
        e.evaluate(&i);
        assert!(e.active.is_empty());
    }
}
