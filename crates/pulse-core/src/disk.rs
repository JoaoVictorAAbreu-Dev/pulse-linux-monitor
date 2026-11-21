//! Discos: sistemas de arquivos (statvfs), dispositivos (/proc/diskstats),
//! explorador de uso de espaço e candidatos à limpeza (somente listados).

use crate::json::{self, Obj};
use crate::sys;
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::Instant;

const REAL_FS: &[&str] = &[
    "ext2", "ext3", "ext4", "xfs", "btrfs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk", "f2fs",
    "zfs", "jfs", "reiserfs", "nfs", "nfs4", "cifs", "smb3", "bcachefs",
];

#[derive(Debug, Clone)]
pub struct Mount {
    pub device: String,
    pub mount: String,
    pub fstype: String,
}

/// Decodifica escapes octais de /proc/mounts (ex.: `\040` = espaço).
pub fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 4 <= b.len() {
            if let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 4]).unwrap_or("x"), 8)
            {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_mounts(src: &str) -> Vec<Mount> {
    let mut seen = std::collections::HashSet::new();
    let mut v = Vec::new();
    for line in src.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 || !REAL_FS.contains(&f[2]) {
            continue;
        }
        let mount = unescape_mount(f[1]);
        // bind mounts repetidos do mesmo dispositivo: mostra só o primeiro ponto.
        if !seen.insert((f[0].to_string(), f[2].to_string())) && mount != "/" {
            continue;
        }
        v.push(Mount {
            device: unescape_mount(f[0]),
            mount,
            fstype: f[2].to_string(),
        });
    }
    v
}

#[derive(Debug, Clone)]
pub struct FsUsage {
    pub mount: Mount,
    pub total: u64,
    pub used: u64,
    pub avail: u64,
    pub pct: f64,
    pub inodes_total: u64,
    pub inodes_used: u64,
}

impl FsUsage {
    pub fn inodes_pct(&self) -> f64 {
        if self.inodes_total == 0 {
            0.0
        } else {
            self.inodes_used as f64 * 100.0 / self.inodes_total as f64
        }
    }
    pub fn to_json(&self) -> String {
        Obj::new()
            .s("mount", &self.mount.mount)
            .s("device", &self.mount.device)
            .s("fstype", &self.mount.fstype)
            .u("total", self.total)
            .u("used", self.used)
            .u("avail", self.avail)
            .n("pct", self.pct)
            .u("inodes_total", self.inodes_total)
            .u("inodes_used", self.inodes_used)
            .n("inodes_pct", self.inodes_pct())
            .finish()
    }
}

pub fn filesystems() -> Vec<FsUsage> {
    let mounts = parse_mounts(&sys::read("/proc/self/mounts").unwrap_or_default());
    let mut out = Vec::new();
    for m in mounts {
        let Some(s) = sys::fs_stat(&m.mount) else {
            continue;
        };
        if s.total == 0 {
            continue;
        }
        let used = s.total.saturating_sub(s.free);
        // Mesmo cálculo do `df`: usado / (usado + disponível ao usuário).
        let denom = used + s.avail;
        let pct = if denom == 0 {
            0.0
        } else {
            used as f64 * 100.0 / denom as f64
        };
        out.push(FsUsage {
            mount: m,
            total: s.total,
            used,
            avail: s.avail,
            pct,
            inodes_total: s.files,
            inodes_used: s.files.saturating_sub(s.files_free),
        });
    }
    out
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DevCounters {
    pub reads: u64,
    pub sectors_read: u64,
    pub writes: u64,
    pub sectors_written: u64,
    pub io_ms: u64,
}

pub fn parse_diskstats(src: &str) -> Vec<(String, DevCounters)> {
    let mut v = Vec::new();
    for line in src.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 14 {
            continue;
        }
        let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
        v.push((
            f[2].to_string(),
            DevCounters {
                reads: n(3),
                sectors_read: n(5),
                writes: n(7),
                sectors_written: n(9),
                io_ms: n(12),
            },
        ));
    }
    v
}

/// Só discos inteiros (presentes em /sys/block), sem loop/ram.
fn is_whole_disk(name: &str) -> bool {
    !(name.starts_with("loop") || name.starts_with("ram"))
        && Path::new(&format!("/sys/block/{}", name)).exists()
}

pub fn read_devices() -> Vec<(String, DevCounters)> {
    parse_diskstats(&sys::read("/proc/diskstats").unwrap_or_default())
        .into_iter()
        .filter(|(n, _)| is_whole_disk(n))
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct DevRate {
    pub name: String,
    pub read_bps: f64,
    pub write_bps: f64,
    pub read_iops: f64,
    pub write_iops: f64,
    pub busy_pct: f64,
    pub model: String,
    pub size: u64,
    pub rotational: bool,
}

impl DevRate {
    pub fn to_json(&self) -> String {
        Obj::new()
            .s("name", &self.name)
            .s("model", &self.model)
            .u("size", self.size)
            .b("rotational", self.rotational)
            .n("read_bps", self.read_bps)
            .n("write_bps", self.write_bps)
            .n("read_iops", self.read_iops)
            .n("write_iops", self.write_iops)
            .n("busy_pct", self.busy_pct)
            .finish()
    }
}

pub fn rates(
    prev: &[(String, DevCounters)],
    cur: &[(String, DevCounters)],
    dt_s: f64,
) -> Vec<DevRate> {
    let pm: HashMap<&str, &DevCounters> = prev.iter().map(|(n, c)| (n.as_str(), c)).collect();
    cur.iter()
        .map(|(name, c)| {
            let p = pm.get(name.as_str()).copied().copied().unwrap_or(*c);
            let d = |a: u64, b: u64| b.saturating_sub(a) as f64 / dt_s.max(0.001);
            DevRate {
                name: name.clone(),
                read_bps: d(p.sectors_read, c.sectors_read) * 512.0,
                write_bps: d(p.sectors_written, c.sectors_written) * 512.0,
                read_iops: d(p.reads, c.reads),
                write_iops: d(p.writes, c.writes),
                busy_pct: (d(p.io_ms, c.io_ms) / 10.0).clamp(0.0, 100.0),
                model: sys::read_trim(&format!("/sys/block/{}/device/model", name))
                    .unwrap_or_default(),
                size: sys::read_u64(&format!("/sys/block/{}/size", name)).unwrap_or(0) * 512,
                rotational: sys::read_trim(&format!("/sys/block/{}/queue/rotational", name))
                    .as_deref()
                    == Some("1"),
            }
        })
        .collect()
}

// ------------------------------------------------------- explorador de espaço

pub struct DuEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
}

pub struct DuResult {
    pub path: String,
    pub parent: Option<String>,
    pub total: u64,
    pub entries: Vec<DuEntry>,
    pub truncated: bool,
    pub errors: u64,
}

impl DuResult {
    pub fn to_json(&self) -> String {
        Obj::new()
            .s("path", &self.path)
            .opt_s("parent", self.parent.as_deref())
            .u("total", self.total)
            .b("truncated", self.truncated)
            .u("errors", self.errors)
            .raw(
                "entries",
                &json::arr(self.entries.iter().map(|e| {
                    Obj::new()
                        .s("name", &e.name)
                        .s("path", &e.path)
                        .u("size", e.size)
                        .b("dir", e.is_dir)
                        .finish()
                })),
            )
            .finish()
    }
}

struct Budget {
    files: u64,
    start: Instant,
    max_files: u64,
    max_ms: u128,
    truncated: bool,
    errors: u64,
}

fn tree_size(p: &Path, dev: u64, b: &mut Budget) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        b.errors += 1;
        return 0;
    };
    let mut total = 0;
    for e in rd.flatten() {
        b.files += 1;
        if b.files > b.max_files || b.start.elapsed().as_millis() > b.max_ms {
            b.truncated = true;
            return total;
        }
        let Ok(md) = e.path().symlink_metadata() else {
            b.errors += 1;
            continue;
        };
        total += md.blocks() * 512;
        if md.is_dir() && md.dev() == dev {
            total += tree_size(&e.path(), dev, b);
        }
    }
    total
}

/// Tamanho de cada filho de `path` (sem atravessar outros sistemas de arquivos),
/// com limites de arquivos e de tempo.
pub fn du(path: &str, max_files: u64, max_ms: u128) -> Result<DuResult, String> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err("caminho precisa ser absoluto".into());
    }
    let p = p.canonicalize().map_err(|e| e.to_string())?;
    let md = p.metadata().map_err(|e| e.to_string())?;
    if !md.is_dir() {
        return Err("não é um diretório".into());
    }
    let dev = md.dev();
    let mut b = Budget {
        files: 0,
        start: Instant::now(),
        max_files,
        max_ms,
        truncated: false,
        errors: 0,
    };
    let mut entries = Vec::new();
    let rd = std::fs::read_dir(&p).map_err(|e| e.to_string())?;
    for e in rd.flatten() {
        let Ok(m) = e.path().symlink_metadata() else {
            continue;
        };
        let mut size = m.blocks() * 512;
        let is_dir = m.is_dir();
        if is_dir && m.dev() == dev {
            size += tree_size(&e.path(), dev, &mut b);
        }
        entries.push(DuEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            path: e.path().to_string_lossy().into_owned(),
            size,
            is_dir: is_dir && m.dev() == dev,
        });
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.size));
    let total = entries.iter().map(|e| e.size).sum();
    entries.truncate(60);
    Ok(DuResult {
        path: p.to_string_lossy().into_owned(),
        parent: p.parent().map(|x| x.to_string_lossy().into_owned()),
        total,
        entries,
        truncated: b.truncated,
        errors: b.errors,
    })
}

/// Candidatos à limpeza. O Pulse **nunca** executa estes comandos: só os lista.
pub fn cleanup_candidates() -> String {
    let home = sys::home();
    let h = home.to_string_lossy();
    let cands: Vec<(String, String, String, String)> = vec![
        ("Temporários".into(), "/tmp".into(), "Arquivos temporários antigos (>7 dias)".into(),
         "find /tmp -xdev -type f -atime +7 -user \"$USER\" -delete".into()),
        ("Cache do APT".into(), "/var/cache/apt/archives".into(), "Pacotes .deb baixados".into(),
         "sudo apt-get clean".into()),
        ("Journal do systemd".into(), "/var/log/journal".into(), "Logs persistentes do journald".into(),
         "sudo journalctl --vacuum-time=14d".into()),
        ("Cache do usuário".into(), format!("{}/.cache", h), "Caches de aplicativos (recriados sob demanda)".into(),
         "rm -rf ~/.cache/thumbnails/*".into()),
        ("Lixeira".into(), format!("{}/.local/share/Trash", h), "Itens na lixeira".into(),
         "gio trash --empty".into()),
        ("Snaps antigos".into(), "/var/lib/snapd/snaps".into(), "Revisões desativadas de snaps".into(),
         "snap list --all | awk '/desativado|disabled/{print $1, $3}'  # revise e remova com: sudo snap remove NOME --revision=REV".into()),
    ];
    json::arr(cands.into_iter().filter(|c| Path::new(&c.1).exists()).map(
        |(title, path, desc, cmd)| {
            let size = du(&path, 200_000, 1500).map(|r| r.total).ok();
            let opt = size.map(|s| s as f64);
            Obj::new()
                .s("title", &title)
                .s("path", &path)
                .s("description", &desc)
                .s("command", &cmd)
                .opt_n("size", opt)
                .finish()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_filtering_and_escapes() {
        let src = "proc /proc proc rw 0 0\n/dev/sda1 / ext4 rw 0 0\n/dev/sdb1 /media/My\\040Disk vfat rw 0 0\ntmpfs /run tmpfs rw 0 0\n/dev/loop0 /snap/core squashfs ro 0 0\n";
        let m = parse_mounts(src);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].mount, "/media/My Disk");
    }

    #[test]
    fn diskstats_and_rates() {
        let a = parse_diskstats("   8       0 sda 100 0 2000 0 50 0 1000 0 0 500 0 0 0 0 0\n");
        let b = parse_diskstats("   8       0 sda 110 0 2200 0 60 0 1400 0 0 1000 0 0 0 0 0\n");
        assert_eq!(a[0].0, "sda");
        let r = rates(&a, &b, 1.0);
        assert_eq!(r[0].read_bps, 200.0 * 512.0);
        assert_eq!(r[0].write_iops, 10.0);
        assert_eq!(r[0].busy_pct, 50.0);
    }

    #[test]
    fn du_on_temp_tree() {
        let dir = std::env::temp_dir().join(format!("pulse-du-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("big/inner")).unwrap();
        std::fs::write(dir.join("big/inner/f"), vec![1u8; 64 * 1024]).unwrap();
        std::fs::write(dir.join("small"), b"x").unwrap();
        let r = du(dir.to_str().unwrap(), 10_000, 5_000).unwrap();
        assert_eq!(r.entries[0].name, "big");
        assert!(r.entries[0].size >= 64 * 1024);
        assert!(r.entries[0].is_dir);
        assert!(du("relative", 10, 10).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
