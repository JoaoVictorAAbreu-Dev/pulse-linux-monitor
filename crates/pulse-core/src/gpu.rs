//! GPUs via /sys/class/drm (amdgpu, i915/xe) e `nvidia-smi` quando existir.

use crate::json::{self, Obj};
use crate::sys;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gpu {
    pub card: String,
    pub vendor: String,
    pub name: String,
    pub driver: String,
    pub busy_pct: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub gtt_used: Option<u64>,
    pub gtt_total: Option<u64>,
    pub temp_c: Option<f64>,
    pub power_w: Option<f64>,
    pub freq_mhz: Option<f64>,
}

impl Gpu {
    pub fn to_json(&self) -> String {
        let n = |v: Option<u64>| v.map(|x| x as f64);
        Obj::new()
            .s("card", &self.card)
            .s("vendor", &self.vendor)
            .s("name", &self.name)
            .s("driver", &self.driver)
            .opt_n("busy_pct", self.busy_pct)
            .opt_n("vram_used", n(self.vram_used))
            .opt_n("vram_total", n(self.vram_total))
            .opt_n("gtt_used", n(self.gtt_used))
            .opt_n("gtt_total", n(self.gtt_total))
            .opt_n("temp_c", self.temp_c)
            .opt_n("power_w", self.power_w)
            .opt_n("freq_mhz", self.freq_mhz)
            .finish()
    }
}

pub fn vendor_name(id: &str) -> &'static str {
    match id.to_lowercase().trim_start_matches("0x") {
        "1002" => "AMD",
        "8086" => "Intel",
        "10de" => "NVIDIA",
        "1af4" => "virtio",
        "15ad" => "VMware",
        _ => "",
    }
}

/// Procura o nome do dispositivo PCI em `pci.ids` (formato do hwdata).
pub fn parse_pci_ids(src: &str, vendor: &str, device: &str) -> Option<String> {
    let vendor = vendor.to_lowercase();
    let device = device.to_lowercase();
    let vendor = vendor.trim_start_matches("0x");
    let device = device.trim_start_matches("0x");
    let mut in_vendor = false;
    for line in src.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            if in_vendor {
                return None; // passou do bloco do fabricante
            }
            in_vendor = line.len() > 4 && line[..4].eq_ignore_ascii_case(vendor);
            continue;
        }
        if in_vendor && !line.starts_with("\t\t") {
            let l = line.trim_start_matches('\t');
            if l.len() > 6 && l[..4].eq_ignore_ascii_case(device) {
                return Some(l[4..].trim().to_string());
            }
        }
    }
    None
}

fn pci_name(vendor: &str, device: &str) -> Option<String> {
    for p in ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"] {
        if let Some(src) = sys::read(p) {
            return parse_pci_ids(&src, vendor, device);
        }
    }
    None
}

/// Placas fixas (cardN) descobertas uma vez; os valores são lidos a cada amostra.
pub struct GpuReader {
    cards: Vec<(String, Gpu)>,
    nvidia_smi: bool,
}

impl Default for GpuReader {
    fn default() -> Self {
        Self::new()
    }
}

fn hwmon_dir(dev: &Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(dev.join("hwmon"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .next()
}

impl GpuReader {
    pub fn new() -> Self {
        let mut cards = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/sys/class/drm") {
            let mut names: Vec<String> = rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit()))
                .collect();
            names.sort();
            for n in names {
                let dev = format!("/sys/class/drm/{}/device", n);
                let vendor = sys::read_trim(&format!("{}/vendor", dev)).unwrap_or_default();
                let device = sys::read_trim(&format!("{}/device", dev)).unwrap_or_default();
                if vendor.is_empty() {
                    continue;
                }
                let driver = std::fs::read_link(format!("{}/driver", dev))
                    .ok()
                    .and_then(|p| p.file_name().map(|x| x.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                let name =
                    pci_name(&vendor, &device).unwrap_or_else(|| format!("{}:{}", vendor, device));
                cards.push((
                    dev,
                    Gpu {
                        card: n,
                        vendor: vendor_name(&vendor).to_string(),
                        name,
                        driver,
                        ..Default::default()
                    },
                ));
            }
        }
        let nvidia_smi = cards.iter().any(|(_, g)| g.vendor == "NVIDIA")
            && sys::run_cmd("nvidia-smi", &["-L"], 3_000)
                .map(|r| r.0)
                .unwrap_or(false);
        GpuReader { cards, nvidia_smi }
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub fn sample(&self) -> Vec<Gpu> {
        let mut out = Vec::new();
        for (dev, base) in &self.cards {
            let mut g = base.clone();
            let u = |f: &str| sys::read_u64(&format!("{}/{}", dev, f));
            g.busy_pct = u("gpu_busy_percent").map(|x| x as f64);
            g.vram_used = u("mem_info_vram_used");
            g.vram_total = u("mem_info_vram_total");
            g.gtt_used = u("mem_info_gtt_used");
            g.gtt_total = u("mem_info_gtt_total");
            if let Some(h) = hwmon_dir(Path::new(dev)) {
                let hv = |f: &str| sys::read_u64(&h.join(f).to_string_lossy());
                g.temp_c = hv("temp1_input").map(|x| x as f64 / 1000.0);
                g.power_w = hv("power1_average")
                    .or_else(|| hv("power1_input"))
                    .map(|x| x as f64 / 1e6);
                g.freq_mhz = hv("freq1_input").map(|x| x as f64 / 1e6);
            }
            if g.freq_mhz.is_none() {
                // Intel i915/xe expõem a frequência atual no diretório da placa.
                let card_dir = Path::new(dev).parent().map(|p| p.to_path_buf());
                if let Some(c) = card_dir {
                    g.freq_mhz = sys::read_u64(&c.join("gt_act_freq_mhz").to_string_lossy())
                        .or_else(|| sys::read_u64(&c.join("gt_cur_freq_mhz").to_string_lossy()))
                        .map(|x| x as f64);
                }
            }
            out.push(g);
        }
        if self.nvidia_smi {
            merge_nvidia(&mut out);
        }
        out
    }
}

/// `nvidia-smi --query-gpu=... --format=csv,noheader,nounits`
pub fn parse_nvidia_smi(src: &str) -> Vec<Gpu> {
    src.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(|x| x.trim()).collect();
            if f.len() < 6 {
                return None;
            }
            let n = |s: &str| s.parse::<f64>().ok();
            Some(Gpu {
                vendor: "NVIDIA".into(),
                name: f[0].to_string(),
                busy_pct: n(f[1]),
                vram_used: n(f[2]).map(|m| (m * 1_048_576.0) as u64),
                vram_total: n(f[3]).map(|m| (m * 1_048_576.0) as u64),
                temp_c: n(f[4]),
                power_w: n(f[5]),
                freq_mhz: f.get(6).and_then(|x| n(x)),
                driver: "nvidia".into(),
                ..Default::default()
            })
        })
        .collect()
}

fn merge_nvidia(out: &mut [Gpu]) {
    let Ok((true, txt, _)) = sys::run_cmd(
        "nvidia-smi",
        &[
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,clocks.sm",
            "--format=csv,noheader,nounits",
        ],
        3_000,
    ) else {
        return;
    };
    let mut nv = parse_nvidia_smi(&txt).into_iter();
    for g in out.iter_mut().filter(|g| g.vendor == "NVIDIA") {
        if let Some(n) = nv.next() {
            let card = std::mem::take(&mut g.card);
            *g = Gpu { card, ..n };
        }
    }
}

pub fn list_json(g: &[Gpu]) -> String {
    json::arr(g.iter().map(|x| x.to_json()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pci_ids_lookup() {
        let src = "# comentário\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t15d8  Picasso/Raven 2 [Radeon Vega Series]\n\t\t1002 15d8  subdispositivo\n\t15dd  Raven Ridge\n10de  NVIDIA Corporation\n\t15d8  outro\n";
        assert_eq!(
            parse_pci_ids(src, "0x1002", "0x15d8").as_deref(),
            Some("Picasso/Raven 2 [Radeon Vega Series]")
        );
        assert_eq!(
            parse_pci_ids(src, "0x10de", "0x15d8").as_deref(),
            Some("outro")
        );
        assert_eq!(parse_pci_ids(src, "0x1002", "0xffff"), None);
        assert_eq!(vendor_name("0x1002"), "AMD");
    }

    #[test]
    fn nvidia_csv() {
        let g =
            parse_nvidia_smi("NVIDIA GeForce RTX 3060, 37, 1024, 12288, 55, 42.10, 1530\nlixo\n");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].busy_pct, Some(37.0));
        assert_eq!(g[0].vram_total, Some(12288 * 1_048_576));
        assert_eq!(g[0].power_w, Some(42.1));
        assert_eq!(g[0].freq_mhz, Some(1530.0));
    }

    #[test]
    fn reader_runs_on_this_machine() {
        let r = GpuReader::new();
        for g in r.sample() {
            assert!(!g.card.is_empty());
            if let (Some(u), Some(t)) = (g.vram_used, g.vram_total) {
                assert!(u <= t);
            }
        }
    }
}
