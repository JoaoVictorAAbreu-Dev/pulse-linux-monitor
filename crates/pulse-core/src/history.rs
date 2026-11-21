//! Ponto de histórico e anel de curto prazo (900 amostras = 15 min a 1 s).

use crate::json::{self, Obj};
use std::collections::VecDeque;

/// Uma amostra. `ts` em **milissegundos** desde a época.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub ts: u64,
    pub cpu: f64,
    pub mem: f64,
    pub swap: f64,
    pub rx: f64,
    pub tx: f64,
    pub rd: f64,
    pub wr: f64,
    /// Temperatura máxima (°C) ou NaN se não houver sensores.
    pub temp: f64,
}

impl Point {
    pub fn to_json(&self) -> String {
        Obj::new()
            .u("ts", self.ts)
            .n("cpu", self.cpu)
            .n("mem", self.mem)
            .n("swap", self.swap)
            .n("rx", self.rx)
            .n("tx", self.tx)
            .n("rd", self.rd)
            .n("wr", self.wr)
            .n("temp", self.temp)
            .finish()
    }
}

/// Serializa pontos em formato colunar (menor que um array de objetos).
pub fn columns_json<'a, I: Iterator<Item = &'a Point> + Clone>(pts: I) -> String {
    let col = |f: fn(&Point) -> f64| json::arr(pts.clone().map(move |p| json::num(f(p))));
    Obj::new()
        .raw("ts", &json::arr(pts.clone().map(|p| p.ts.to_string())))
        .raw("cpu", &col(|p| p.cpu))
        .raw("mem", &col(|p| p.mem))
        .raw("swap", &col(|p| p.swap))
        .raw("rx", &col(|p| p.rx))
        .raw("tx", &col(|p| p.tx))
        .raw("rd", &col(|p| p.rd))
        .raw("wr", &col(|p| p.wr))
        .raw("temp", &col(|p| p.temp))
        .finish()
}

pub struct Ring {
    cap: usize,
    pub items: VecDeque<Point>,
}

impl Ring {
    pub fn new(cap: usize) -> Self {
        Ring { cap, items: VecDeque::with_capacity(cap) }
    }
    pub fn push(&mut self, p: Point) {
        if self.items.len() == self.cap {
            self.items.pop_front();
        }
        self.items.push_back(p);
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn to_json(&self) -> String {
        columns_json(self.items.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_caps() {
        let mut r = Ring::new(3);
        for i in 0..5 {
            r.push(Point { ts: i, ..Default::default() });
        }
        assert_eq!(r.len(), 3);
        assert_eq!(r.items.front().unwrap().ts, 2);
        let j = r.to_json();
        assert!(j.starts_with("{\"ts\":[2,3,4]"));
    }

    #[test]
    fn nan_temp_is_null() {
        let p = Point { temp: f64::NAN, ..Default::default() };
        assert!(p.to_json().contains("\"temp\":null"));
    }
}
