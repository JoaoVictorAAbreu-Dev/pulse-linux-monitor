//! Energia: baterias e fonte (/sys/class/power_supply) e consumo do pacote
//! da CPU via RAPL (/sys/class/powercap), quando legível.

use crate::json::{self, Obj};
use crate::sys;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Battery {
    pub name: String,
    pub capacity_pct: Option<f64>,
    pub status: String,
    pub power_w: Option<f64>,
    /// Saúde: capacidade atual ÷ capacidade de fábrica.
    pub health_pct: Option<f64>,
    /// Horas restantes (descarregando) ou até carregar.
    pub hours_left: Option<f64>,
}

/// Lê uma bateria a partir de um mapa de atributos (nome → conteúdo).
pub fn parse_battery(name: &str, get: impl Fn(&str) -> Option<String>) -> Battery {
    let n = |k: &str| get(k).and_then(|v| v.trim().parse::<f64>().ok());
    let status = get("status")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    // energy_* em µWh; charge_* em µAh (× tensão para Wh)
    let volt = n("voltage_now").map(|v| v / 1e6);
    let (now, full, design) = if n("energy_full").is_some() {
        (n("energy_now"), n("energy_full"), n("energy_full_design"))
    } else {
        let to_e = |x: Option<f64>| match (x, volt) {
            (Some(c), Some(v)) => Some(c * v),
            _ => None,
        };
        (
            to_e(n("charge_now")),
            to_e(n("charge_full")),
            to_e(n("charge_full_design")),
        )
    };
    let power_w = n("power_now")
        .map(|p| p / 1e6)
        .or_else(|| match (n("current_now"), volt) {
            (Some(c), Some(v)) => Some(c / 1e6 * v),
            _ => None,
        })
        .filter(|p| *p > 0.0);
    let health_pct = match (full, design) {
        (Some(f), Some(d)) if d > 0.0 => Some((f / d * 100.0).min(150.0)),
        _ => None,
    };
    let hours_left = match (now, full, power_w) {
        (Some(e), _, Some(p)) if status == "Discharging" => Some(e / 1e6 / p),
        (Some(e), Some(f), Some(p)) if status == "Charging" => Some((f - e).max(0.0) / 1e6 / p),
        _ => None,
    };
    Battery {
        name: name.to_string(),
        capacity_pct: n("capacity"),
        status,
        power_w,
        health_pct,
        hours_left,
    }
}

pub struct PowerReader {
    rapl: Option<(String, u64, u64, Instant)>, // (arquivo, max_energy_range, última leitura, quando)
    pub rapl_note: Option<String>,
}

impl Default for PowerReader {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerReader {
    pub fn new() -> Self {
        let base = "/sys/class/powercap/intel-rapl:0";
        let file = format!("{}/energy_uj", base);
        if !Path::new(base).exists() {
            return PowerReader {
                rapl: None,
                rapl_note: None,
            };
        }
        match std::fs::read_to_string(&file) {
            Ok(s) => {
                let e = s.trim().parse().unwrap_or(0);
                let max =
                    sys::read_u64(&format!("{}/max_energy_range_uj", base)).unwrap_or(u64::MAX);
                PowerReader {
                    rapl: Some((file, max, e, Instant::now())),
                    rapl_note: None,
                }
            }
            Err(e) => PowerReader {
                rapl: None,
                rapl_note: Some(if e.kind() == std::io::ErrorKind::PermissionDenied {
                    "consumo da CPU (RAPL) só é legível pelo root neste kernel".into()
                } else {
                    format!("RAPL indisponível: {}", e)
                }),
            },
        }
    }

    /// Watts do pacote da CPU desde a última chamada.
    pub fn cpu_watts(&mut self) -> Option<f64> {
        let (file, max, last, at) = self.rapl.as_mut()?;
        let now = sys::read_u64(file)?;
        let dt = at.elapsed().as_secs_f64();
        let delta = if now >= *last {
            now - *last
        } else {
            (*max - *last) + now
        };
        *last = now;
        *at = Instant::now();
        if dt <= 0.0 {
            return None;
        }
        Some(delta as f64 / 1e6 / dt)
    }

    pub fn batteries() -> (Vec<Battery>, Option<bool>) {
        let mut bats = Vec::new();
        let mut ac = None;
        if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                let get = |k: &str| std::fs::read_to_string(p.join(k)).ok();
                match get("type").as_deref().map(|s| s.trim()) {
                    Some("Battery")
                        if get("scope").map(|s| s.trim() != "Device").unwrap_or(true) =>
                    {
                        bats.push(parse_battery(&name, get))
                    }
                    Some("Mains") => {
                        ac = Some(
                            get("online").map(|s| s.trim() == "1").unwrap_or(false)
                                || ac == Some(true),
                        )
                    }
                    _ => {}
                }
            }
        }
        bats.sort_by(|a, b| a.name.cmp(&b.name));
        (bats, ac)
    }
}

pub fn power_json(
    bats: &[Battery],
    ac: Option<bool>,
    cpu_w: Option<f64>,
    note: Option<&str>,
) -> String {
    Obj::new()
        .raw(
            "batteries",
            &json::arr(bats.iter().map(|b| {
                Obj::new()
                    .s("name", &b.name)
                    .opt_n("capacity_pct", b.capacity_pct)
                    .s("status", &b.status)
                    .opt_n("power_w", b.power_w)
                    .opt_n("health_pct", b.health_pct)
                    .opt_n("hours_left", b.hours_left)
                    .finish()
            })),
        )
        .raw(
            "ac_online",
            &ac.map(|b| b.to_string()).unwrap_or_else(|| "null".into()),
        )
        .opt_n("cpu_w", cpu_w)
        .opt_s("note", note)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn bat(m: &[(&str, &str)]) -> Battery {
        let h: HashMap<String, String> = m
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        parse_battery("BAT0", |k| h.get(k).cloned())
    }

    #[test]
    fn energy_based_battery() {
        let b = bat(&[
            ("capacity", "50"),
            ("status", "Discharging"),
            ("energy_now", "25000000"),
            ("energy_full", "50000000"),
            ("energy_full_design", "62500000"),
            ("power_now", "10000000"),
        ]);
        assert_eq!(b.capacity_pct, Some(50.0));
        assert_eq!(b.power_w, Some(10.0));
        assert!((b.health_pct.unwrap() - 80.0).abs() < 1e-9);
        assert!((b.hours_left.unwrap() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn charge_based_battery() {
        let b = bat(&[
            ("status", "Charging"),
            ("charge_now", "1000000"),
            ("charge_full", "3000000"),
            ("charge_full_design", "3000000"),
            ("voltage_now", "12000000"),
            ("current_now", "2000000"),
        ]);
        assert_eq!(b.power_w, Some(24.0));
        assert_eq!(b.health_pct, Some(100.0));
        assert!(
            (b.hours_left.unwrap() - 1.0).abs() < 1e-9,
            "{:?}",
            b.hours_left
        );
    }

    #[test]
    fn rapl_reader_does_not_panic() {
        let mut r = PowerReader::new();
        let _ = r.cpu_watts();
        let (_b, _ac) = PowerReader::batteries();
    }
}
