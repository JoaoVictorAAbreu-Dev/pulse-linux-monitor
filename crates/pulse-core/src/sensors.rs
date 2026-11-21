//! Temperaturas (hwmon e thermal zones) e dados do host.

use crate::json::{self, Obj};
use crate::sys;

#[derive(Debug, Clone)]
pub struct Temp {
    pub chip: String,
    pub label: String,
    pub celsius: f64,
    pub crit: Option<f64>,
}

pub fn temps() -> Vec<Temp> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/hwmon") {
        let mut dirs: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for d in dirs {
            let chip =
                sys::read_trim(&format!("{}/name", d.display())).unwrap_or_else(|| "hwmon".into());
            let Ok(files) = std::fs::read_dir(&d) else {
                continue;
            };
            let mut inputs: Vec<String> = files
                .flatten()
                .map(|f| f.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("temp") && n.ends_with("_input"))
                .collect();
            inputs.sort();
            for inp in inputs {
                let base = inp.trim_end_matches("_input");
                let Some(milli) = sys::read_trim(&format!("{}/{}", d.display(), inp))
                    .and_then(|s| s.parse::<f64>().ok())
                else {
                    continue;
                };
                let c = milli / 1000.0;
                if !(-40.0..=150.0).contains(&c) || c == 0.0 {
                    continue;
                }
                v.push(Temp {
                    label: sys::read_trim(&format!("{}/{}_label", d.display(), base))
                        .unwrap_or_else(|| base.to_string()),
                    chip: chip.clone(),
                    celsius: c,
                    crit: sys::read_trim(&format!("{}/{}_crit", d.display(), base))
                        .and_then(|s| s.parse::<f64>().ok())
                        .map(|x| x / 1000.0)
                        .filter(|x| *x > 0.0 && *x < 200.0),
                });
            }
        }
    }
    if v.is_empty() {
        // Alternativa: thermal zones.
        if let Ok(rd) = std::fs::read_dir("/sys/class/thermal") {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if !n.starts_with("thermal_zone") {
                    continue;
                }
                let p = e.path();
                if let Some(c) = sys::read_trim(&format!("{}/temp", p.display()))
                    .and_then(|s| s.parse::<f64>().ok())
                {
                    let c = c / 1000.0;
                    if c > 0.0 && c < 150.0 {
                        v.push(Temp {
                            chip: sys::read_trim(&format!("{}/type", p.display()))
                                .unwrap_or(n.clone()),
                            label: n,
                            celsius: c,
                            crit: None,
                        });
                    }
                }
            }
        }
    }
    v
}

pub fn temps_json(t: &[Temp]) -> String {
    json::arr(t.iter().map(|x| {
        Obj::new()
            .s("chip", &x.chip)
            .s("label", &x.label)
            .n("c", x.celsius)
            .opt_n("crit", x.crit)
            .finish()
    }))
}

pub fn max_temp(t: &[Temp]) -> Option<f64> {
    t.iter()
        .map(|x| x.celsius)
        .fold(None, |a, b| Some(a.map_or(b, |a: f64| a.max(b))))
}

#[derive(Debug, Clone, Default)]
pub struct Host {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub cpu_model: String,
    pub cores: usize,
    pub user: String,
}

pub fn parse_os_release(src: &str) -> String {
    src.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_else(|| "Linux".into())
}

pub fn host(cores: usize) -> Host {
    Host {
        hostname: sys::read_trim("/proc/sys/kernel/hostname").unwrap_or_default(),
        os: parse_os_release(&sys::read("/etc/os-release").unwrap_or_default()),
        kernel: sys::read_trim("/proc/sys/kernel/osrelease").unwrap_or_default(),
        cpu_model: crate::cpu::model_name(),
        cores,
        user: sys::users().get(&sys::uid()).cloned().unwrap_or_default(),
    }
}

pub fn uptime_s() -> f64 {
    sys::read("/proc/uptime")
        .and_then(|s| s.split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release() {
        assert_eq!(
            parse_os_release("NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n"),
            "Ubuntu 24.04 LTS"
        );
        assert_eq!(parse_os_release(""), "Linux");
    }

    #[test]
    fn max_of_temps() {
        let t = vec![
            Temp {
                chip: "a".into(),
                label: "x".into(),
                celsius: 40.0,
                crit: None,
            },
            Temp {
                chip: "b".into(),
                label: "y".into(),
                celsius: 70.5,
                crit: None,
            },
        ];
        assert_eq!(max_temp(&t), Some(70.5));
        assert_eq!(max_temp(&[]), None);
    }
}
