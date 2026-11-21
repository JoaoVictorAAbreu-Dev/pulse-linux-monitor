//! Log de eventos só-anexar (uma linha JSON por evento), com limite de linhas.
//!
//! Mesmo contrato do `LongStore`: recarrega ao abrir ignorando linhas
//! inválidas, compacta via arquivo temporário + `rename` quando passa de
//! 2× a capacidade e nunca derruba o daemon por erro de E/S (o erro fica em
//! `last_error`).

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub struct EventLog {
    path: PathBuf,
    cap: usize,
    lines: VecDeque<String>,
    file_lines: usize,
    pub last_error: Option<String>,
}

impl EventLog {
    /// Abre o log e carrega as últimas `cap` linhas que `valid` aceitar.
    pub fn open(path: impl Into<PathBuf>, cap: usize, valid: impl Fn(&str) -> bool) -> Self {
        let mut s = EventLog {
            path: path.into(),
            cap: cap.max(1),
            lines: VecDeque::new(),
            file_lines: 0,
            last_error: None,
        };
        match std::fs::read_to_string(&s.path) {
            Ok(txt) => {
                for l in txt.lines() {
                    s.file_lines += 1;
                    let l = l.trim();
                    if l.is_empty() || !valid(l) {
                        continue;
                    }
                    s.lines.push_back(l.to_string());
                    if s.lines.len() > s.cap {
                        s.lines.pop_front();
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => s.last_error = Some(format!("leitura de {}: {}", s.path.display(), e)),
        }
        s
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn len(&self) -> usize {
        self.lines.len()
    }
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
    pub fn file_lines(&self) -> usize {
        self.file_lines
    }
    /// Linhas retidas, da mais antiga para a mais recente.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(|s| s.as_str())
    }

    /// Acrescenta uma linha (quebras de linha são removidas).
    pub fn append(&mut self, line: &str) {
        let line: String = line.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        self.lines.push_back(line.clone());
        if self.lines.len() > self.cap {
            self.lines.pop_front();
        }
        if let Some(dir) = self.path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                self.last_error = Some(format!("criar {}: {}", dir.display(), e));
                return;
            }
        }
        let r = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)
            .and_then(|mut f| f.write_all(format!("{}\n", line).as_bytes()));
        match r {
            Ok(()) => {
                self.file_lines += 1;
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("escrita em {}: {}", self.path.display(), e));
                return;
            }
        }
        if self.file_lines > 2 * self.cap {
            self.compact();
        }
    }

    pub fn compact(&mut self) {
        let tmp = self.path.with_extension("tmp");
        let mut body = String::new();
        for l in &self.lines {
            body.push_str(l);
            body.push('\n');
        }
        let r = (|| -> std::io::Result<()> {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, &self.path)
        })();
        match r {
            Ok(()) => self.file_lines = self.lines.len(),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                self.last_error = Some(format!("compactação de {}: {}", self.path.display(), e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pulse-ev-{}-{}", n, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn append_reload_compact() {
        let d = dir("a");
        let p = d.join("ev.log");
        let ok = |l: &str| l.starts_with('{');
        {
            let mut l = EventLog::open(&p, 3, ok);
            for i in 0..7 {
                l.append(&format!("{{\"i\":{}}}", i));
            }
            assert_eq!(l.len(), 3);
            assert!(l.file_lines() <= 6, "compactou");
            assert!(l.last_error.is_none());
        }
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"lixo\n\n").unwrap();
        let l = EventLog::open(&p, 3, ok);
        let v: Vec<&str> = l.lines().collect();
        assert_eq!(v, vec!["{\"i\":4}", "{\"i\":5}", "{\"i\":6}"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn newline_injection_and_io_error() {
        let d = dir("b");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("arquivo"), b"x").unwrap();
        let mut l = EventLog::open(d.join("arquivo/ev.log"), 5, |_| true);
        l.append("a\nb");
        assert_eq!(l.lines().next(), Some("ab"));
        assert!(l.last_error.is_some());
        let _ = std::fs::remove_dir_all(&d);
    }
}
