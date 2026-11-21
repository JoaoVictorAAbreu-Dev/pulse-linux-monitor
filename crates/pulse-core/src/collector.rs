//! Junta todos os coletores numa amostra (`Tick`) com seções JSON prontas.

use crate::cpu::{self, Stat};
use crate::disk::{self, DevCounters, FsUsage};
use crate::history::Point;
use crate::json::{self, Obj};
use crate::mem::{self, Mem};
use crate::net::{self, IfCounters};
use crate::procs::{self, Proc, Sampler};
use crate::sensors::{self, Host, Temp};
use crate::sys;
use std::time::Instant;

pub struct Collector {
    prev_stat: Stat,
    prev_disk: Vec<(String, DevCounters)>,
    prev_net: Vec<(String, IfCounters)>,
    prev_at: Instant,
    prev_ctxt: u64,
    prev_vm: mem::VmStat,
    procs: Sampler,
    pub host: Host,
    ticks: u64,
    listening: String,
    tcp_states: String,
    gpus: crate::gpu::GpuReader,
    gpu_json: String,
    power: crate::power::PowerReader,
    power_json: String,
}

pub struct Tick {
    pub ts: u64,
    pub point: Point,
    pub procs: Vec<Proc>,
    pub fs: Vec<FsUsage>,
    pub temps: Vec<Temp>,
    pub mem: Mem,
    pub self_cpu: f64,
    pub self_rss: u64,
    /// Seções do snapshot já serializadas: (chave, JSON).
    pub sections: Vec<(&'static str, String)>,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        let stat = cpu::read_stat();
        let cores = stat.cores.len().max(1);
        let mut procs = Sampler::new();
        procs.sample();
        Collector {
            prev_ctxt: stat.ctxt,
            prev_stat: stat,
            prev_disk: disk::read_devices(),
            prev_net: net::read_ifaces(),
            prev_at: Instant::now(),
            prev_vm: mem::vmstat(),
            procs,
            host: sensors::host(cores),
            ticks: 0,
            listening: "[]".into(),
            tcp_states: "{}".into(),
            gpus: crate::gpu::GpuReader::new(),
            gpu_json: "[]".into(),
            power: crate::power::PowerReader::new(),
            power_json: "null".into(),
        }
    }

    pub fn host_json(&self) -> String {
        Obj::new()
            .s("hostname", &self.host.hostname)
            .s("os", &self.host.os)
            .s("kernel", &self.host.kernel)
            .s("user", &self.host.user)
            .s("cpu_model", &self.host.cpu_model)
            .u("cores", self.host.cores as u64)
            .n("uptime_s", sensors::uptime_s())
            .finish()
    }

    pub fn sample(&mut self) -> Tick {
        let now = Instant::now();
        let dt = now.duration_since(self.prev_at).as_secs_f64().max(0.001);
        let ts = sys::now_ms();

        // CPU
        let stat = cpu::read_stat();
        let u = cpu::usage(&self.prev_stat.total, &stat.total);
        let cores_json = json::arr(stat.cores.iter().enumerate().map(|(i, c)| {
            let pu = self
                .prev_stat
                .cores
                .get(i)
                .map(|p| cpu::usage(p, c).total)
                .unwrap_or(0.0);
            Obj::new()
                .u("id", i as u64)
                .n("pct", pu)
                .opt_n("mhz", cpu::core_mhz(i))
                .finish()
        }));
        let ctxt_ps = stat.ctxt.saturating_sub(self.prev_ctxt) as f64 / dt;
        let load = cpu::loadavg();
        let psi_c = cpu::psi("cpu");
        let cpu_json = Obj::new()
            .n("total", u.total)
            .n("user", u.user)
            .n("system", u.system)
            .n("iowait", u.iowait)
            .n("irq", u.irq)
            .n("steal", u.steal)
            .n("idle", u.idle)
            .raw("load", &json::arr_n(&load))
            .n("ctxt_ps", ctxt_ps)
            .u("procs_running", stat.procs_running)
            .u("procs_blocked", stat.procs_blocked)
            .raw("psi", &psi_json(&psi_c))
            .opt_s("governor", cpu::governor().as_deref())
            .s("model", &self.host.cpu_model)
            .raw("cores", &cores_json)
            .finish();
        self.prev_ctxt = stat.ctxt;
        self.prev_stat = stat;

        // Memória
        let m = mem::read();
        let vm = mem::vmstat();
        let psi_m = cpu::psi("memory");
        let mem_json = Obj::new()
            .u("total", m.total)
            .u("used", m.used())
            .u("available", m.available)
            .u("apps", m.apps())
            .u("cache", m.cache())
            .u("free", m.free)
            .u("buffers", m.buffers)
            .u("shmem", m.shmem)
            .u("dirty", m.dirty)
            .u("writeback", m.writeback)
            .n("pct", m.used_pct())
            .u("swap_total", m.swap_total)
            .u("swap_used", m.swap_used())
            .u("swap_cached", m.swap_cached)
            .n("swap_pct", m.swap_pct())
            .opt_n("swappiness", mem::swappiness().map(|x| x as f64))
            .u("oom_kills", vm.oom_kill)
            .n(
                "swap_in_ps",
                vm.pswpin.saturating_sub(self.prev_vm.pswpin) as f64 / dt,
            )
            .n(
                "swap_out_ps",
                vm.pswpout.saturating_sub(self.prev_vm.pswpout) as f64 / dt,
            )
            .n(
                "major_faults_ps",
                vm.pgmajfault.saturating_sub(self.prev_vm.pgmajfault) as f64 / dt,
            )
            .raw("psi", &psi_json(&psi_m))
            .finish();
        self.prev_vm = vm;

        // Discos
        let fs = disk::filesystems();
        let devs = disk::read_devices();
        let rates = disk::rates(&self.prev_disk, &devs, dt);
        self.prev_disk = devs;
        let (rd, wr) = rates
            .iter()
            .fold((0.0, 0.0), |a, r| (a.0 + r.read_bps, a.1 + r.write_bps));
        let psi_io = cpu::psi("io");
        let disks_json = Obj::new()
            .raw("fs", &json::arr(fs.iter().map(|f| f.to_json())))
            .raw("devices", &json::arr(rates.iter().map(|r| r.to_json())))
            .n("read_bps", rd)
            .n("write_bps", wr)
            .raw("psi", &psi_json(&psi_io))
            .finish();

        // Rede
        let ifs = net::read_ifaces();
        let info = net::iface_info(&self.prev_net, &ifs, dt);
        self.prev_net = ifs;
        let (rx, tx) = info
            .iter()
            .filter(|i| !i.virtual_if)
            .fold((0.0, 0.0), |a, i| (a.0 + i.rx_bps, a.1 + i.tx_bps));
        if self.ticks.is_multiple_of(5) {
            let socks = net::read_sockets();
            let owners = net::socket_owners();
            self.listening = net::listening_json(&socks, &owners);
            let counts = net::tcp_state_counts(&socks);
            let mut o = Obj::new();
            for (k, v) in counts {
                o = o.u(k, v);
            }
            self.tcp_states = o.finish();
        }
        let net_json = Obj::new()
            .n("rx_bps", rx)
            .n("tx_bps", tx)
            .raw("ifaces", &json::arr(info.iter().map(|i| i.to_json())))
            .raw("tcp", &self.tcp_states)
            .raw("listening", &self.listening)
            .finish();

        // GPU (a cada amostra; nvidia-smi é caro, então a cada 5)
        if !self.gpus.is_empty() && (self.ticks.is_multiple_of(5) || !self.gpus_has_nvidia()) {
            self.gpu_json = crate::gpu::list_json(&self.gpus.sample());
        }
        // Energia: bateria e RAPL a cada 5 amostras (variam devagar)
        if self.ticks.is_multiple_of(5) {
            let (bats, ac) = crate::power::PowerReader::batteries();
            let cpu_w = self.power.cpu_watts();
            self.power_json =
                crate::power::power_json(&bats, ac, cpu_w, self.power.rapl_note.as_deref());
        }

        // Temperaturas
        let temps = sensors::temps();
        let tmax = sensors::max_temp(&temps);

        // Processos
        let plist = self.procs.sample();
        let me = sys::self_pid();
        let (self_cpu, self_rss) = plist
            .iter()
            .find(|p| p.pid == me)
            .map(|p| (p.cpu, p.rss))
            .unwrap_or((0.0, 0));
        let summary = procs::summarize(&plist);
        let top = procs::top(&plist, 8);
        let procs_json = Obj::new()
            .raw("summary", &procs::summary_json(&summary))
            .raw("top", &procs::list_json(&top))
            .raw("top_mem", &procs::list_json(&procs::top_mem(&plist, 8)))
            .raw("top_swap", &procs::list_json(&procs::top_swap(&plist, 10)))
            .finish();

        let point = Point {
            ts,
            cpu: u.total,
            mem: m.used_pct(),
            swap: m.swap_pct(),
            rx,
            tx,
            rd,
            wr,
            temp: tmax.unwrap_or(f64::NAN),
        };

        self.prev_at = now;
        self.ticks += 1;
        Tick {
            ts,
            point,
            fs,
            mem: m,
            self_cpu,
            self_rss,
            sections: vec![
                ("host", self.host_json()),
                ("cpu", cpu_json),
                ("mem", mem_json),
                ("disks", disks_json),
                ("net", net_json),
                ("temps", sensors::temps_json(&temps)),
                ("procs", procs_json),
                ("gpus", self.gpu_json.clone()),
                ("power", self.power_json.clone()),
            ],
            temps,
            procs: plist,
        }
    }
}

impl Collector {
    fn gpus_has_nvidia(&self) -> bool {
        self.gpu_json.contains("\"NVIDIA\"")
    }
}

fn psi_json(p: &cpu::Psi) -> String {
    if !p.available {
        return "null".into();
    }
    Obj::new()
        .n("some10", p.some10)
        .n("some60", p.some60)
        .opt_n("full10", p.full10)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_samples_produce_valid_json() {
        let mut c = Collector::new();
        std::thread::sleep(std::time::Duration::from_millis(150));
        let t = c.sample();
        assert!(t.point.cpu >= 0.0 && t.point.cpu <= 100.0);
        assert!(t.mem.total > 0);
        for (k, v) in &t.sections {
            json::parse(v).unwrap_or_else(|e| panic!("seção {} inválida: {}", k, e));
        }
    }
}
