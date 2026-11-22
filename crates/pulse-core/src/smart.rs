//! Saúde dos discos via `smartctl -j` (smartmontools), opcional.
//! Sem o programa ou sem permissão, o Pulse explica como habilitar.
//! O número de série nunca é exposto.

use crate::json::{self, Obj, Value};
use crate::sys;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Health {
    pub device: String,
    pub model: String,
    pub passed: Option<bool>,
    pub temp_c: Option<f64>,
    pub power_on_hours: Option<f64>,
    pub reallocated: Option<f64>,
    pub pending: Option<f64>,
    pub nvme_used_pct: Option<f64>,
    pub media_errors: Option<f64>,
    pub error: Option<String>,
    /// O smartctl foi negado por falta de permissão: a leitura com senha
    /// (`POST /api/disk/smart/privileged`) resolve.
    pub needs_root: bool,
}

/// Nome de disco aceito (`sda`, `nvme0n1`…): só letras e números ASCII.
pub fn valid_device(d: &str) -> bool {
    !d.is_empty() && d.len() <= 32 && d.chars().all(|c| c.is_ascii_alphanumeric())
}

fn attr(v: &Value, id: u32) -> Option<f64> {
    v.path(&["ata_smart_attributes", "table"])?
        .as_array()?
        .iter()
        .find(|a| a.get("id").and_then(|x| x.as_f64()) == Some(id as f64))?
        .path(&["raw", "value"])?
        .as_f64()
}

pub fn parse_smartctl(device: &str, src: &str) -> Health {
    let mut h = Health {
        device: device.to_string(),
        ..Default::default()
    };
    let v = match json::parse(src.trim()) {
        Ok(v) => v,
        Err(_) => {
            h.error = Some("saída do smartctl não é JSON".into());
            return h;
        }
    };
    h.model = v
        .get("model_name")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    h.passed = v
        .path(&["smart_status", "passed"])
        .and_then(|x| x.as_bool());
    h.temp_c = v.path(&["temperature", "current"]).and_then(|x| x.as_f64());
    h.power_on_hours = v.path(&["power_on_time", "hours"]).and_then(|x| x.as_f64());
    h.reallocated = attr(&v, 5);
    h.pending = attr(&v, 197);
    let nv = |k: &str| {
        v.path(&["nvme_smart_health_information_log", k])
            .and_then(|x| x.as_f64())
    };
    h.nvme_used_pct = nv("percentage_used");
    h.media_errors = nv("media_errors");
    if h.passed.is_none() {
        let msgs: Vec<String> = v
            .path(&["smartctl", "messages"])
            .and_then(|m| m.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.get("string")?.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if !msgs.is_empty() {
            let joined = msgs.join(" · ");
            h.needs_root =
                joined.contains("Permission denied") || joined.contains("Operation not permitted");
            h.error = Some(if h.needs_root {
                "sem permissão: o smartctl precisa de root (ou do grupo disk) para ler este disco"
                    .into()
            } else {
                joined
            });
        }
    }
    h
}

pub fn health_json(list: &[Health]) -> String {
    json::arr(list.iter().map(health_obj))
}

/// Um disco em JSON (o mesmo objeto de cada item de `disks`).
pub fn health_obj(h: &Health) -> String {
    Obj::new()
        .s("device", &h.device)
        .s("model", &h.model)
        .raw(
            "passed",
            &h.passed
                .map(|b| b.to_string())
                .unwrap_or_else(|| "null".into()),
        )
        .opt_n("temp_c", h.temp_c)
        .opt_n("power_on_hours", h.power_on_hours)
        .opt_n("reallocated", h.reallocated)
        .opt_n("pending", h.pending)
        .opt_n("nvme_used_pct", h.nvme_used_pct)
        .opt_n("media_errors", h.media_errors)
        .opt_s("error", h.error.as_deref())
        .b("needs_root", h.needs_root)
        .finish()
}

/// Consulta todos os discos inteiros. Lento (≈ 1 s por disco): só sob demanda.
pub fn smart_json(devices: &[String]) -> String {
    match sys::run_cmd("smartctl", &["--version"], 3_000) {
        Ok((true, _, _)) => {}
        _ => {
            return Obj::new()
                .b("available", false)
                .s(
                    "reason",
                    "smartctl não está instalado (pacote smartmontools)",
                )
                .s("install", "sudo apt install smartmontools")
                .raw("disks", "[]")
                .finish()
        }
    }
    let list: Vec<Health> = devices
        .iter()
        .filter(|d| valid_device(d))
        .map(|d| {
            let dev = format!("/dev/{}", d);
            match sys::run_cmd("smartctl", &["-j", "-H", "-A", "-i", &dev], 15_000) {
                Ok((_, out, _)) => parse_smartctl(d, &out),
                Err(e) => Health {
                    device: d.clone(),
                    error: Some(e),
                    ..Default::default()
                },
            }
        })
        .collect();
    Obj::new()
        .b("available", true)
        // o pulse-helper (pacote .deb) permite ler com senha os discos negados
        .b(
            "privileged",
            std::path::Path::new(crate::services::HELPER).exists(),
        )
        .raw("disks", &health_json(&list))
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ata_disk() {
        let src = r#"{"smartctl":{"exit_status":0},"model_name":"KINGSTON SA400S37","serial_number":"SEGREDO123","smart_status":{"passed":true},"temperature":{"current":31},"power_on_time":{"hours":8123},"ata_smart_attributes":{"table":[{"id":5,"name":"Reallocated_Sector_Ct","raw":{"value":0}},{"id":197,"raw":{"value":2}}]}}"#;
        let h = parse_smartctl("sda", src);
        assert_eq!(h.passed, Some(true));
        assert_eq!(h.temp_c, Some(31.0));
        assert_eq!(h.power_on_hours, Some(8123.0));
        assert_eq!((h.reallocated, h.pending), (Some(0.0), Some(2.0)));
        assert!(
            !health_json(&[h]).contains("SEGREDO123"),
            "série nunca aparece"
        );
    }

    #[test]
    fn nvme_and_permission_error() {
        let n = parse_smartctl(
            "nvme0n1",
            r#"{"smart_status":{"passed":false},"nvme_smart_health_information_log":{"percentage_used":7,"media_errors":1}}"#,
        );
        assert_eq!(n.passed, Some(false));
        assert_eq!((n.nvme_used_pct, n.media_errors), (Some(7.0), Some(1.0)));
        let p = parse_smartctl(
            "sda",
            r#"{"smartctl":{"exit_status":2,"messages":[{"string":"Smartctl open device: /dev/sda failed: Permission denied","severity":"error"}]}}"#,
        );
        assert!(p.needs_root);
        assert!(health_obj(&p).contains("\"needs_root\":true"));
        assert!(p.error.unwrap().contains("root"));
        assert!(!n.needs_root);
        assert!(health_obj(&n).contains("\"needs_root\":false"));
        assert!(valid_device("sda") && valid_device("nvme0n1"));
        for bad in ["", "../sda", "sda1;x", "-a", "sd a", &"a".repeat(33)] {
            assert!(!valid_device(bad), "{}", bad);
        }
        assert!(parse_smartctl("sda", "lixo").error.is_some());
    }
}
