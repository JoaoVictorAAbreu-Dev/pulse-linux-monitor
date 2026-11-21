//! Histórico longo: médias de 60 s com retenção de 7 dias (10 080 pontos),
//! persistidas num arquivo texto só-anexar.
//!
//! Formato de cada linha: `ts cpu mem swap rx tx rd wr temp` (ts em ms,
//! temp `-` quando não há sensores). Linhas inválidas são ignoradas ao
//! carregar. Quando o arquivo passa de 2× a capacidade, é compactado
//! escrevendo um temporário e fazendo `rename` atômico. Erros de E/S são
//! registrados em `last_error` e nunca interrompem o daemon.

use crate::history::{columns_json, Point};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub const BUCKET_MS: u64 = 60_000;
pub const DEFAULT_CAP: usize = 10_080;
pub const RETENTION_MS: u64 = 7 * 24 * 3600 * 1000;

#[derive(Default)]
struct Acc {
    bucket: u64,
    n: u32,
    sum: Point,
    temp_n: u32,
}

pub struct LongStore {
    path: PathBuf,
    cap: usize,
    points: VecDeque<Point>,
    acc: Option<Acc>,
    file_lines: usize,
    pub last_error: Option<String>,
    pub compactions: u64,
}

fn fmt_line(p: &Point) -> String {
    let f = |v: f64| if v.is_finite() { format!("{:.3}", v) } else { "-".to_string() };
    format!(
        "{} {} {} {} {} {} {} {} {}\n",
        p.ts,
        f(p.cpu),
        f(p.mem),
        f(p.swap),
        f(p.rx),
        f(p.tx),
        f(p.rd),
        f(p.wr),
        f(p.temp)
    )
}

pub fn parse_line(line: &str) -> Option<Point> {
    let f: Vec<&str> = line.split_whitespace().collect();
    if f.len() != 9 {
        return None;
    }
    let ts: u64 = f[0].parse().ok()?;
    let n = |s: &str| -> Option<f64> {
        if s == "-" {
            return Some(f64::NAN);
        }
        let v: f64 = s.parse().ok()?;
        if v.is_finite() { Some(v) } else { None }
    };
    let cpu = n(f[1])?;
    if !cpu.is_finite() {
        return None;
    }
    Some(Point {
        ts,
        cpu,
        mem: n(f[2])?,
        swap: n(f[3])?,
        rx: n(f[4])?,
        tx: n(f[5])?,
        rd: n(f[6])?,
        wr: n(f[7])?,
        temp: n(f[8])?,
    })
}

impl LongStore {
    /// Abre (ou cria) o armazenamento e recarrega pontos válidos do arquivo.
    pub fn open(path: impl Into<PathBuf>, cap: usize, now_ms: u64) -> Self {
        let path = path.into();
        let mut s = LongStore {
            path,
            cap: cap.max(1),
            points: VecDeque::new(),
            acc: None,
            file_lines: 0,
            last_error: None,
            compactions: 0,
        };
        s.load(now_ms);
        s
    }

    fn load(&mut self, now_ms: u64) {
        let txt = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                self.last_error = Some(format!("leitura de {}: {}", self.path.display(), e));
                return;
            }
        };
        let cutoff = now_ms.saturating_sub(RETENTION_MS);
        let mut last_ts = 0u64;
        for line in txt.lines() {
            self.file_lines += 1;
            let Some(p) = parse_line(line) else { continue };
            if p.ts < cutoff || p.ts <= last_ts {
                continue;
            }
            last_ts = p.ts;
            self.points.push_back(p);
            if self.points.len() > self.cap {
                self.points.pop_front();
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
    pub fn file_lines(&self) -> usize {
        self.file_lines
    }
    pub fn points(&self) -> impl Iterator<Item = &Point> + Clone {
        self.points.iter()
    }

    /// Acrescenta uma amostra (ts em ms). Fecha a média do minuto anterior
    /// quando a amostra pertence a um novo bucket.
    pub fn add(&mut self, p: Point) {
        let bucket = p.ts / BUCKET_MS;
        if let Some(acc) = &self.acc {
            if acc.bucket != bucket {
                let done = self.acc.take().unwrap();
                if bucket > done.bucket {
                    self.commit(done);
                }
            }
        }
        let acc = self.acc.get_or_insert_with(|| Acc { bucket, ..Default::default() });
        acc.n += 1;
        acc.sum.cpu += p.cpu;
        acc.sum.mem += p.mem;
        acc.sum.swap += p.swap;
        acc.sum.rx += p.rx;
        acc.sum.tx += p.tx;
        acc.sum.rd += p.rd;
        acc.sum.wr += p.wr;
        if p.temp.is_finite() {
            acc.sum.temp += p.temp;
            acc.temp_n += 1;
        }
    }

    fn commit(&mut self, a: Acc) {
        if a.n == 0 {
            return;
        }
        let n = a.n as f64;
        let p = Point {
            ts: a.bucket * BUCKET_MS,
            cpu: a.sum.cpu / n,
            mem: a.sum.mem / n,
            swap: a.sum.swap / n,
            rx: a.sum.rx / n,
            tx: a.sum.tx / n,
            rd: a.sum.rd / n,
            wr: a.sum.wr / n,
            temp: if a.temp_n > 0 { a.sum.temp / a.temp_n as f64 } else { f64::NAN },
        };
        if let Some(last) = self.points.back() {
            if p.ts <= last.ts {
                return; // relógio voltou: ignora para manter ordem
            }
        }
        self.points.push_back(p);
        let cutoff = p.ts.saturating_sub(RETENTION_MS);
        while self.points.len() > self.cap || self.points.front().map(|x| x.ts < cutoff).unwrap_or(false) {
            self.points.pop_front();
        }
        self.append(&p);
        if self.file_lines > 2 * self.cap {
            self.compact();
        }
    }

    fn ensure_dir(&mut self) -> bool {
        if let Some(dir) = self.path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                self.last_error = Some(format!("criar {}: {}", dir.display(), e));
                return false;
            }
        }
        true
    }

    fn append(&mut self, p: &Point) {
        if !self.ensure_dir() {
            return;
        }
        let r = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)
            .and_then(|mut f| f.write_all(fmt_line(p).as_bytes()));
        match r {
            Ok(()) => {
                self.file_lines += 1;
                self.last_error = None;
            }
            Err(e) => self.last_error = Some(format!("escrita em {}: {}", self.path.display(), e)),
        }
    }

    /// Reescreve o arquivo só com os pontos retidos (temporário + rename).
    pub fn compact(&mut self) {
        if !self.ensure_dir() {
            return;
        }
        let tmp = self.path.with_extension("log.tmp");
        let mut body = String::with_capacity(self.points.len() * 60);
        for p in &self.points {
            body.push_str(&fmt_line(p));
        }
        let r = (|| -> std::io::Result<()> {
            let mut f = OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&tmp)?;
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, &self.path)
        })();
        match r {
            Ok(()) => {
                self.file_lines = self.points.len();
                self.compactions += 1;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                self.last_error = Some(format!("compactação de {}: {}", self.path.display(), e));
            }
        }
    }

    /// JSON colunar com pontos a partir de `since_ms`.
    pub fn to_json(&self, since_ms: u64) -> String {
        let it = self.points.iter().filter(move |p| p.ts >= since_ms);
        crate::json::Obj::new()
            .u("bucket_ms", BUCKET_MS)
            .u("count", self.points.iter().filter(|p| p.ts >= since_ms).count() as u64)
            .u("capacity", self.cap as u64)
            .opt_s("error", self.last_error.as_deref())
            .raw("points", &columns_json(it))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pulse-lt-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn pt(ts: u64, cpu: f64) -> Point {
        Point { ts, cpu, mem: 50.0, swap: 0.0, rx: 10.0, tx: 20.0, rd: 0.0, wr: 0.0, temp: f64::NAN }
    }

    const T0: u64 = 1_700_000_000_000 - (1_700_000_000_000 % BUCKET_MS);

    #[test]
    fn averages_per_minute_using_ms_timestamps() {
        let dir = tmpdir("avg");
        let mut s = LongStore::open(dir.join("history.log"), 100, T0);
        for i in 0..60 {
            s.add(pt(T0 + i * 1000, if i % 2 == 0 { 10.0 } else { 30.0 }));
        }
        assert_eq!(s.len(), 0, "minuto ainda aberto");
        s.add(pt(T0 + 60_000, 0.0));
        assert_eq!(s.len(), 1);
        let p = s.points().next().unwrap();
        assert_eq!(p.ts, T0);
        assert!((p.cpu - 20.0).abs() < 1e-9);
        assert!(p.temp.is_nan());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persists_and_reloads_ignoring_invalid_lines() {
        let dir = tmpdir("reload");
        let path = dir.join("history.log");
        {
            let mut s = LongStore::open(&path, 100, T0);
            for m in 0..5 {
                s.add(pt(T0 + m * BUCKET_MS, m as f64));
            }
            assert_eq!(s.len(), 4);
            assert!(s.last_error.is_none());
        }
        // corrompe o arquivo
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"lixo\n1 2 3\n\n99 NaN 1 1 1 1 1 1 1\n").unwrap();
        drop(f);
        let s = LongStore::open(&path, 100, T0 + 10 * BUCKET_MS);
        assert_eq!(s.len(), 4);
        assert_eq!(s.points().last().unwrap().cpu, 3.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compacts_when_file_exceeds_twice_capacity() {
        let dir = tmpdir("compact");
        let path = dir.join("history.log");
        let mut s = LongStore::open(&path, 5, T0);
        for m in 0..13 {
            s.add(pt(T0 + m * BUCKET_MS, m as f64));
        }
        // 12 pontos fechados; compacta ao passar de 10 linhas.
        assert!(s.compactions >= 1);
        assert_eq!(s.len(), 5);
        let lines = std::fs::read_to_string(&path).unwrap().lines().count();
        assert!(lines <= 10, "arquivo com {} linhas", lines);
        assert!(!dir.join("history.log.tmp").exists());
        let r = LongStore::open(&path, 5, T0 + 13 * BUCKET_MS);
        assert_eq!(r.len(), 5);
        assert_eq!(r.points().last().unwrap().cpu, 11.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drops_points_older_than_retention_on_load() {
        let dir = tmpdir("retention");
        let path = dir.join("history.log");
        std::fs::create_dir_all(&dir).unwrap();
        let old = fmt_line(&pt(T0, 1.0));
        let new = fmt_line(&pt(T0 + RETENTION_MS + BUCKET_MS, 2.0));
        std::fs::write(&path, format!("{}{}", old, new)).unwrap();
        let s = LongStore::open(&path, 100, T0 + RETENTION_MS + 2 * BUCKET_MS);
        assert_eq!(s.len(), 1);
        assert_eq!(s.points().next().unwrap().cpu, 2.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn io_errors_do_not_panic() {
        // diretório-pai é um arquivo: criar/escrever falha, mas segue vivo.
        let dir = tmpdir("ioerr");
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("arquivo");
        std::fs::write(&blocker, b"x").unwrap();
        let mut s = LongStore::open(blocker.join("history.log"), 10, T0);
        s.add(pt(T0, 1.0));
        s.add(pt(T0 + BUCKET_MS, 1.0));
        assert_eq!(s.len(), 1);
        assert!(s.last_error.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_since_filter() {
        let dir = tmpdir("json");
        let mut s = LongStore::open(dir.join("h.log"), 100, T0);
        for m in 0..4 {
            s.add(pt(T0 + m * BUCKET_MS, 1.0));
        }
        let j = s.to_json(T0 + BUCKET_MS);
        assert!(j.contains("\"count\":2"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
