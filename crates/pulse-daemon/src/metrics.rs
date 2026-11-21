//! Exposição no formato de texto do Prometheus (`GET /metrics`).
//! Gerado a partir do snapshot já serializado e do estado dos agentes.

use pulse_core::json::{self, Value};
use std::fmt::Write as _;

/// Escapa um valor de rótulo (\\, " e quebra de linha).
pub fn esc_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

pub struct Out {
    buf: String,
    declared: std::collections::HashSet<String>,
}

impl Default for Out {
    fn default() -> Self {
        Self::new()
    }
}

impl Out {
    pub fn new() -> Self {
        Out {
            buf: String::new(),
            declared: Default::default(),
        }
    }
    fn declare(&mut self, name: &str, kind: &str, help: &str) {
        if self.declared.insert(name.to_string()) {
            let _ = writeln!(self.buf, "# HELP {} {}", name, help);
            let _ = writeln!(self.buf, "# TYPE {} {}", name, kind);
        }
    }
    pub fn gauge(&mut self, name: &str, help: &str, labels: &[(&str, &str)], v: Option<f64>) {
        self.sample(name, "gauge", help, labels, v)
    }
    pub fn counter(&mut self, name: &str, help: &str, labels: &[(&str, &str)], v: Option<f64>) {
        self.sample(name, "counter", help, labels, v)
    }
    fn sample(
        &mut self,
        name: &str,
        kind: &str,
        help: &str,
        labels: &[(&str, &str)],
        v: Option<f64>,
    ) {
        let Some(v) = v.filter(|x| x.is_finite()) else {
            return;
        };
        self.declare(name, kind, help);
        self.buf.push_str(name);
        if !labels.is_empty() {
            self.buf.push('{');
            for (i, (k, val)) in labels.iter().enumerate() {
                if i > 0 {
                    self.buf.push(',');
                }
                let _ = write!(self.buf, "{}=\"{}\"", k, esc_label(val));
            }
            self.buf.push('}');
        }
        let _ = writeln!(self.buf, " {}", fmt(v));
    }
    pub fn finish(self) -> String {
        self.buf
    }
}

fn fmt(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

fn n(v: &Value, path: &[&str]) -> Option<f64> {
    v.path(path).and_then(|x| x.as_f64())
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

fn arr<'a>(v: &'a Value, path: &[&str]) -> &'a [Value] {
    v.path(path)
        .and_then(|x| x.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

/// Monta a exposição a partir do snapshot, dos agentes e do índice de uso.
pub fn render(snapshot: &str, agents: &str, usage: &str) -> String {
    let mut o = Out::new();
    let Ok(sn) = json::parse(snapshot) else {
        return o.finish();
    };
    let host = s(sn.get("host").unwrap_or(&Value::Null), "hostname").to_string();
    o.gauge(
        "pulse_up",
        "1 se o coletor do Pulse produziu um snapshot",
        &[],
        Some(1.0),
    );
    o.gauge(
        "pulse_uptime_seconds",
        "tempo ligado do sistema",
        &[],
        n(&sn, &["host", "uptime_s"]),
    );
    o.gauge(
        "pulse_self_cpu_percent",
        "CPU usada pelo próprio Pulse",
        &[],
        n(&sn, &["pulse", "cpu"]),
    );
    o.gauge(
        "pulse_self_rss_bytes",
        "memória residente do Pulse",
        &[],
        n(&sn, &["pulse", "rss"]),
    );
    let _ = host;

    // CPU
    o.gauge(
        "pulse_cpu_usage_percent",
        "uso total da CPU",
        &[],
        n(&sn, &["cpu", "total"]),
    );
    for (mode, k) in [
        ("user", "user"),
        ("system", "system"),
        ("iowait", "iowait"),
        ("irq", "irq"),
        ("steal", "steal"),
    ] {
        o.gauge(
            "pulse_cpu_mode_percent",
            "uso da CPU por modo",
            &[("mode", mode)],
            n(&sn, &["cpu", k]),
        );
    }
    for c in arr(&sn, &["cpu", "cores"]) {
        let id = n(c, &["id"])
            .map(|x| (x as i64).to_string())
            .unwrap_or_default();
        o.gauge(
            "pulse_cpu_core_usage_percent",
            "uso por núcleo",
            &[("core", &id)],
            n(c, &["pct"]),
        );
        o.gauge(
            "pulse_cpu_core_frequency_mhz",
            "frequência por núcleo",
            &[("core", &id)],
            n(c, &["mhz"]),
        );
    }
    for (i, w) in ["1", "5", "15"].iter().enumerate() {
        let v = arr(&sn, &["cpu", "load"]).get(i).and_then(|x| x.as_f64());
        o.gauge("pulse_load_average", "carga média", &[("window", w)], v);
    }
    o.gauge(
        "pulse_context_switches_per_second",
        "trocas de contexto",
        &[],
        n(&sn, &["cpu", "ctxt_ps"]),
    );
    for (res, path) in [
        ("cpu", ["cpu", "psi"]),
        ("memory", ["mem", "psi"]),
        ("io", ["disks", "psi"]),
    ] {
        let p = sn.path(&path).cloned().unwrap_or(Value::Null);
        o.gauge(
            "pulse_pressure_some_avg10_percent",
            "PSI some (média de 10 s)",
            &[("resource", res)],
            n(&p, &["some10"]),
        );
    }

    // Memória
    for (k, name) in [
        ("total", "total"),
        ("used", "used"),
        ("available", "available"),
        ("cache", "cache"),
        ("apps", "apps"),
        ("shmem", "shmem"),
        ("dirty", "dirty"),
    ] {
        o.gauge(
            "pulse_memory_bytes",
            "memória por categoria",
            &[("kind", name)],
            n(&sn, &["mem", k]),
        );
    }
    o.gauge(
        "pulse_swap_total_bytes",
        "swap total",
        &[],
        n(&sn, &["mem", "swap_total"]),
    );
    o.gauge(
        "pulse_swap_used_bytes",
        "swap em uso",
        &[],
        n(&sn, &["mem", "swap_used"]),
    );
    o.counter(
        "pulse_oom_kills_total",
        "processos mortos por falta de memória desde o boot",
        &[],
        n(&sn, &["mem", "oom_kills"]),
    );

    // Discos
    for f in arr(&sn, &["disks", "fs"]) {
        let l = [
            ("mount", s(f, "mount")),
            ("device", s(f, "device")),
            ("fstype", s(f, "fstype")),
        ];
        o.gauge(
            "pulse_filesystem_size_bytes",
            "tamanho do sistema de arquivos",
            &l,
            n(f, &["total"]),
        );
        o.gauge(
            "pulse_filesystem_used_bytes",
            "espaço usado",
            &l,
            n(f, &["used"]),
        );
        o.gauge(
            "pulse_filesystem_avail_bytes",
            "espaço disponível ao usuário",
            &l,
            n(f, &["avail"]),
        );
        o.gauge(
            "pulse_filesystem_inodes_used_percent",
            "inodes em uso",
            &l,
            n(f, &["inodes_pct"]),
        );
    }
    for d in arr(&sn, &["disks", "devices"]) {
        let l = [("device", s(d, "name"))];
        o.gauge(
            "pulse_disk_read_bytes_per_second",
            "leitura do dispositivo",
            &l,
            n(d, &["read_bps"]),
        );
        o.gauge(
            "pulse_disk_write_bytes_per_second",
            "escrita do dispositivo",
            &l,
            n(d, &["write_bps"]),
        );
        o.gauge(
            "pulse_disk_busy_percent",
            "ocupação do dispositivo",
            &l,
            n(d, &["busy_pct"]),
        );
    }

    // Rede
    for i in arr(&sn, &["net", "ifaces"]) {
        let l = [("iface", s(i, "name"))];
        o.counter(
            "pulse_network_receive_bytes_total",
            "bytes recebidos",
            &l,
            n(i, &["rx_bytes"]),
        );
        o.counter(
            "pulse_network_transmit_bytes_total",
            "bytes enviados",
            &l,
            n(i, &["tx_bytes"]),
        );
        o.gauge(
            "pulse_network_receive_bytes_per_second",
            "taxa de recepção",
            &l,
            n(i, &["rx_bps"]),
        );
        o.gauge(
            "pulse_network_transmit_bytes_per_second",
            "taxa de envio",
            &l,
            n(i, &["tx_bps"]),
        );
        o.gauge(
            "pulse_network_up",
            "1 se a interface está ativa",
            &l,
            Some(if s(i, "state") == "up" { 1.0 } else { 0.0 }),
        );
    }
    if let Some(Value::Obj(states)) = sn.path(&["net", "tcp"]) {
        for (st, v) in states {
            o.gauge(
                "pulse_tcp_connections",
                "conexões TCP por estado",
                &[("state", st)],
                v.as_f64(),
            );
        }
    }

    // Sensores e GPU
    for t in arr(&sn, &["temps"]) {
        o.gauge(
            "pulse_temperature_celsius",
            "temperatura do sensor",
            &[("chip", s(t, "chip")), ("sensor", s(t, "label"))],
            n(t, &["c"]),
        );
    }
    for g in arr(&sn, &["gpus"]) {
        let l = [
            ("card", s(g, "card")),
            ("vendor", s(g, "vendor")),
            ("name", s(g, "name")),
        ];
        o.gauge(
            "pulse_gpu_busy_percent",
            "uso da GPU",
            &l,
            n(g, &["busy_pct"]),
        );
        o.gauge(
            "pulse_gpu_vram_used_bytes",
            "VRAM em uso",
            &l,
            n(g, &["vram_used"]),
        );
        o.gauge(
            "pulse_gpu_vram_total_bytes",
            "VRAM total",
            &l,
            n(g, &["vram_total"]),
        );
        o.gauge(
            "pulse_gpu_temperature_celsius",
            "temperatura da GPU",
            &l,
            n(g, &["temp_c"]),
        );
        o.gauge(
            "pulse_gpu_power_watts",
            "potência da GPU",
            &l,
            n(g, &["power_w"]),
        );
    }

    // Processos e alertas
    for (k, st) in [
        ("running", "running"),
        ("sleeping", "sleeping"),
        ("zombie", "zombie"),
        ("stopped", "stopped"),
    ] {
        o.gauge(
            "pulse_processes",
            "processos por estado",
            &[("state", st)],
            n(&sn, &["procs", "summary", k]),
        );
    }
    o.gauge(
        "pulse_threads",
        "threads no sistema",
        &[],
        n(&sn, &["procs", "summary", "threads"]),
    );
    let alerts = arr(&sn, &["alerts"]);
    for sev in ["aviso", "critico"] {
        let c = alerts.iter().filter(|a| s(a, "severity") == sev).count() as f64;
        o.gauge(
            "pulse_alerts_active",
            "alertas ativos por severidade",
            &[("severity", sev)],
            Some(c),
        );
    }

    // Agentes
    if let Ok(ag) = json::parse(agents) {
        let mut per: std::collections::BTreeMap<String, (f64, f64, f64)> = Default::default();
        for p in arr(&ag, &[]) {
            let e = per.entry(s(p, "agent").to_string()).or_default();
            e.0 += 1.0;
            e.1 += n(p, &["tree_cpu"]).unwrap_or(0.0);
            e.2 += n(p, &["tree_rss"]).unwrap_or(0.0);
        }
        for (a, (c, cpu, rss)) in per {
            o.gauge(
                "pulse_agent_processes",
                "processos de agentes de IA",
                &[("agent", &a)],
                Some(c),
            );
            o.gauge(
                "pulse_agent_cpu_percent",
                "CPU da árvore de processos do agente",
                &[("agent", &a)],
                Some(cpu),
            );
            o.gauge(
                "pulse_agent_rss_bytes",
                "RSS somado da árvore do agente",
                &[("agent", &a)],
                Some(rss),
            );
        }
    }
    if let Ok(u) = json::parse(usage) {
        let today = s(&u, "today").to_string();
        for r in arr(&u, &["rows"]) {
            if s(r, "day") != today {
                continue;
            }
            for kind in ["input", "output", "cache_read", "cache_write"] {
                o.gauge(
                    "pulse_agent_tokens_today",
                    "tokens usados hoje (fuso local) por agente, modelo e tipo",
                    &[
                        ("agent", s(r, "agent")),
                        ("model", s(r, "model")),
                        ("kind", kind),
                    ],
                    n(r, &[kind]),
                );
            }
        }
    }
    o.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposition_format() {
        let snap = r#"{"host":{"hostname":"h","uptime_s":10},"pulse":{"cpu":1.5,"rss":100},
          "cpu":{"total":12.5,"user":10,"system":2,"iowait":0,"irq":0,"steal":0,"load":[1,0.5,0.25],"ctxt_ps":100,"psi":null,"cores":[{"id":0,"pct":20,"mhz":null}]},
          "mem":{"total":100,"used":40,"swap_total":0,"swap_used":0,"oom_kills":2,"psi":{"some10":0.5}},
          "disks":{"fs":[{"mount":"/media/My \"Disk\"","device":"/dev/sdb1","fstype":"vfat","total":10,"used":5,"avail":5,"inodes_pct":1}],"devices":[]},
          "net":{"ifaces":[{"name":"eth0","state":"up","rx_bytes":5,"tx_bytes":6,"rx_bps":1,"tx_bps":2}],"tcp":{"ESTABLISHED":3}},
          "temps":[],"gpus":[],"procs":{"summary":{"running":1,"threads":9}},"alerts":[{"severity":"critico"}]}"#;
        let agents = r#"[{"agent":"Codex","tree_cpu":3,"tree_rss":10},{"agent":"Codex","tree_cpu":1,"tree_rss":5}]"#;
        let usage = r#"{"today":"2026-01-01","rows":[{"day":"2026-01-01","agent":"Codex","model":"gpt-5","input":7,"output":1,"cache_read":0,"cache_write":0},{"day":"2025-12-31","agent":"Codex","model":"gpt-5","input":99,"output":0,"cache_read":0,"cache_write":0}]}"#;
        let m = render(snap, agents, usage);
        assert!(m.contains("# TYPE pulse_cpu_usage_percent gauge\npulse_cpu_usage_percent 12.5\n"));
        assert_eq!(
            m.matches("# HELP pulse_cpu_mode_percent").count(),
            1,
            "HELP/TYPE uma vez só"
        );
        assert!(m.contains(r#"pulse_filesystem_size_bytes{mount="/media/My \"Disk\"",device="/dev/sdb1",fstype="vfat"} 10"#));
        assert!(m.contains(r#"pulse_load_average{window="15"} 0.25"#));
        assert!(m.contains(r#"pulse_tcp_connections{state="ESTABLISHED"} 3"#));
        assert!(m.contains(r#"pulse_alerts_active{severity="critico"} 1"#));
        assert!(m.contains(r#"pulse_agent_processes{agent="Codex"} 2"#));
        assert!(
            m.contains(r#"pulse_agent_tokens_today{agent="Codex",model="gpt-5",kind="input"} 7"#)
        );
        assert!(!m.contains(" 99\n"), "só o dia de hoje");
        assert!(
            !m.contains("pulse_cpu_core_frequency_mhz"),
            "valores nulos são omitidos"
        );
        assert!(m.contains("# TYPE pulse_oom_kills_total counter"));
        assert!(render("lixo", "", "").is_empty());
    }
}
