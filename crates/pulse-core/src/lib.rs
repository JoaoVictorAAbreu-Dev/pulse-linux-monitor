//! Núcleo do Pulse: coletores do Linux (/proc, /sys, statvfs, systemctl,
//! journalctl), histórico curto e longo, e regras de alerta.
//! Usa somente a biblioteca padrão.

pub mod alerts;
pub mod cgroups;
pub mod collector;
pub mod cpu;
pub mod disk;
pub mod eventlog;
pub mod gpu;
pub mod history;
pub mod json;
pub mod longterm;
pub mod mem;
pub mod net;
pub mod power;
pub mod procs;
pub mod schedule;
pub mod sensors;
pub mod services;
pub mod smart;
pub mod sys;
