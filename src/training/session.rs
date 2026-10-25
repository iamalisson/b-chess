//! Today's session: a phase's daily routine as timed blocks, warm-up first.

use crate::training::curriculum::Phase;
use std::time::{Duration, Instant};

pub struct Session {
    /// (exercise id, minutes)
    pub blocks: Vec<(String, u32)>,
    pub index: usize,
    block_started: Instant,
}

impl Session {
    pub fn new(phase: &Phase) -> Session {
        Session {
            blocks: phase.routine.clone(),
            index: 0,
            block_started: Instant::now(),
        }
    }

    pub fn current(&self) -> Option<&(String, u32)> {
        self.blocks.get(self.index)
    }

    pub fn time_left(&self) -> Duration {
        let minutes = self.current().map_or(0, |b| b.1);
        Duration::from_secs(u64::from(minutes) * 60).saturating_sub(self.block_started.elapsed())
    }

    pub fn advance(&mut self) {
        self.index += 1;
        self.block_started = Instant::now();
    }

    /// What runs after the current run: the same block again while it repeats (drills,
    /// until the block's minutes are used), otherwise the next block. `true` means a repeat.
    pub fn upcoming(&self, repeats: impl Fn(&str) -> bool) -> Option<(&str, bool)> {
        let (id, _) = self.current()?;
        if repeats(id) && !self.time_left().is_zero() {
            return Some((id, true));
        }
        self.blocks
            .get(self.index + 1)
            .map(|(id, _)| (id.as_str(), false))
    }

    pub fn total_minutes(&self) -> u32 {
        self.blocks.iter().map(|b| b.1).sum()
    }
}
