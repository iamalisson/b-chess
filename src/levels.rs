//! The ladder: which network plays at each Elo, and how to step through it.

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// One rung of the ladder: which network, how many nodes, how often a random move.
pub struct Level {
    pub elo: u32,
    pub weights: &'static str,
    pub nodes: u32,
    pub random_pct: u64,
}

pub const fn lv(elo: u32, weights: &'static str, nodes: u32, random_pct: u64) -> Level {
    Level {
        elo,
        weights,
        nodes,
        random_pct,
    }
}

// Maia 1100-1900 are trained on human games at that rating and play from their raw policy
// (1 node). Below 1100 we mix random moves into maia-1100; above 1900 we add search, then
// switch to a full Leela network. Only the Maia ratings are real: the rest are rough guesses,
// not calibrated by matches (and depend on CPU speed only through time, not strength).
pub const LEVELS: &[Level] = &[
    lv(600, "maia-1100", 1, 50),
    lv(700, "maia-1100", 1, 40),
    lv(800, "maia-1100", 1, 30),
    lv(900, "maia-1100", 1, 20),
    lv(1000, "maia-1100", 1, 10),
    lv(1100, "maia-1100", 1, 0),
    lv(1200, "maia-1200", 1, 0),
    lv(1300, "maia-1300", 1, 0),
    lv(1400, "maia-1400", 1, 0),
    lv(1500, "maia-1500", 1, 0),
    lv(1600, "maia-1600", 1, 0),
    lv(1700, "maia-1700", 1, 0),
    lv(1800, "maia-1800", 1, 0),
    lv(1900, "maia-1900", 1, 0),
    lv(2000, "maia-1900", 16, 0),
    lv(2100, "maia-1900", 64, 0),
    lv(2200, "t1-256x10", 1, 0),
    lv(2300, "t1-256x10", 2, 0),
    lv(2400, "t1-256x10", 4, 0),
    lv(2500, "t1-256x10", 8, 0),
    lv(2600, "t1-256x10", 16, 0),
    lv(2700, "t1-256x10", 32, 0),
    lv(2800, "t1-256x10", 64, 0),
    lv(2900, "t1-256x10", 128, 0),
    lv(3000, "t1-256x10", 256, 0),
];

pub fn spec(elo: u32) -> &'static Level {
    LEVELS
        .iter()
        .find(|l| l.elo >= elo)
        .unwrap_or(&LEVELS[LEVELS.len() - 1])
}

pub fn describe(elo: u32) -> String {
    let l = spec(elo);
    match (l.random_pct, l.nodes) {
        (0, 1) => l.weights.to_string(),
        (0, n) => format!("{}, {n} nodes", l.weights),
        (r, _) => format!("{} + {r}% random", l.weights),
    }
}

/// Elo of every level whose weights file is present in `dir`, ascending.
pub fn available(dir: &Path) -> Vec<u32> {
    LEVELS
        .iter()
        .filter(|l| dir.join(format!("{}.pb.gz", l.weights)).exists())
        .map(|l| l.elo)
        .collect()
}

/// Move `delta` steps from `cur` through `levels`, clamped to the ends.
pub fn step(levels: &[u32], cur: u32, delta: i32) -> u32 {
    let last = levels.len() as i32 - 1;
    let i = levels
        .iter()
        .position(|&l| l >= cur)
        .map_or(last, |i| i as i32);
    levels[(i + delta).clamp(0, last) as usize]
}

/// Tiny xorshift: enough for random moves and a random color, no dependency needed.
pub struct Rng(u64);

impl Rng {
    pub fn new() -> Rng {
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Rng(t | 1)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_moves_and_clamps() {
        let l = [1100, 1200, 1300];
        assert_eq!(step(&l, 1100, 1), 1200);
        assert_eq!(step(&l, 1300, 1), 1300);
        assert_eq!(step(&l, 1100, -1), 1100);
        assert_eq!(step(&l, 1250, 0), 1300);
        assert_eq!(step(&l, 9999, 0), 1300);
    }

    #[test]
    fn levels_span_600_to_3000_in_100s() {
        let elos: Vec<u32> = LEVELS.iter().map(|l| l.elo).collect();
        assert_eq!(elos, (6..=30).map(|n| n * 100).collect::<Vec<_>>());
        assert_eq!(spec(1500).weights, "maia-1500");
        assert_eq!(describe(800), "maia-1100 + 30% random");
        assert_eq!(describe(2400), "t1-256x10, 4 nodes");
    }
}
