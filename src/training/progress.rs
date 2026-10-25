//! Training progress in data/training.tsv: one event per line. Passes unlock phases;
//! logs fill the training log; game events decide game exercises.

use crate::store::{home, today};
use crate::training::curriculum::{Curriculum, GameSpec, Goal, Kind};
use std::{fs, io, io::Write, path::PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// One finished exercise run (the course's training log).
    Log {
        date: String,
        phase: u8,
        exercise: String,
        seconds: u64,
        right: u32,
        total: u32,
        passed: bool,
    },
    /// An exercise was passed.
    Pass { exercise: String, date: String },
    /// One finished training game.
    Game {
        date: String,
        exercise: String,
        result: i32,
        /// Ended on the board (mate or draw), not resigned or abandoned.
        natural: bool,
        illegal: u32,
        peeks: u32,
        moves: u32,
        quiz_right: u32,
        quiz_total: u32,
        /// How it ended, e.g. "checkmate", "insufficient material", "resigned".
        reason: String,
    },
}

impl Event {
    pub fn parse(line: &str) -> Option<Event> {
        let f: Vec<&str> = line.split('\t').collect();
        let num = |i: usize| f.get(i)?.parse::<u64>().ok();
        Some(match *f.first()? {
            "log" if f.len() == 8 => Event::Log {
                date: f[1].into(),
                phase: num(2)? as u8,
                exercise: f[3].into(),
                seconds: num(4)?,
                right: num(5)? as u32,
                total: num(6)? as u32,
                passed: f[7] == "1",
            },
            "pass" if f.len() == 3 => Event::Pass {
                exercise: f[1].into(),
                date: f[2].into(),
            },
            // 11 fields; lines written before the reason was kept have 10.
            "game" if f.len() == 10 || f.len() == 11 => Event::Game {
                date: f[1].into(),
                exercise: f[2].into(),
                result: f[3].parse().ok()?,
                natural: f[4] == "1",
                illegal: num(5)? as u32,
                peeks: num(6)? as u32,
                moves: num(7)? as u32,
                quiz_right: num(8)? as u32,
                quiz_total: num(9)? as u32,
                reason: f.get(10).map_or(String::new(), |r| r.to_string()),
            },
            _ => return None,
        })
    }

    pub fn to_line(&self) -> String {
        match self {
            Event::Log {
                date,
                phase,
                exercise,
                seconds,
                right,
                total,
                passed,
            } => format!(
                "log\t{date}\t{phase}\t{exercise}\t{seconds}\t{right}\t{total}\t{}",
                u8::from(*passed)
            ),
            Event::Pass { exercise, date } => format!("pass\t{exercise}\t{date}"),
            Event::Game {
                date,
                exercise,
                result,
                natural,
                illegal,
                peeks,
                moves,
                quiz_right,
                quiz_total,
                reason,
            } => format!(
                "game\t{date}\t{exercise}\t{result}\t{}\t{illegal}\t{peeks}\t{moves}\t{quiz_right}\t{quiz_total}\t{reason}",
                u8::from(*natural)
            ),
        }
    }
}

/// Is `line` a training progress line (for import)?
pub fn is_event_line(line: &str) -> bool {
    Event::parse(line).is_some()
}

fn training_file() -> PathBuf {
    home().join("data/training.tsv")
}

#[derive(Default)]
pub struct Progress {
    pub events: Vec<Event>,
}

impl Progress {
    pub fn load() -> Progress {
        let text = fs::read_to_string(training_file()).unwrap_or_default();
        Progress {
            events: text.lines().filter_map(Event::parse).collect(),
        }
    }

    /// Adds an event and appends it to the file.
    pub fn record(&mut self, event: Event) -> io::Result<()> {
        let path = training_file();
        fs::create_dir_all(path.parent().unwrap())?;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(f, "{}", event.to_line())?;
        self.events.push(event);
        Ok(())
    }

    /// Replaces everything (import).
    pub fn replace(&mut self, lines: &[String]) -> io::Result<()> {
        let path = training_file();
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(
            path,
            lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        )?;
        self.events = lines.iter().filter_map(|l| Event::parse(l)).collect();
        Ok(())
    }

    pub fn lines(&self) -> Vec<String> {
        self.events.iter().map(Event::to_line).collect()
    }

    pub fn passed(&self, exercise: &str) -> bool {
        self.events
            .iter()
            .any(|e| matches!(e, Event::Pass { exercise: x, .. } if x == exercise))
    }

    /// Records a pass unless already passed. Returns true if it is new.
    pub fn mark_passed(&mut self, exercise: &str) -> io::Result<bool> {
        if self.passed(exercise) {
            return Ok(false);
        }
        self.record(Event::Pass {
            exercise: exercise.into(),
            date: today(),
        })?;
        Ok(true)
    }

    /// Every checkpoint exercise of phase `n` is passed.
    pub fn checkpoint_done(&self, course: &Curriculum, n: u8) -> bool {
        course
            .phases
            .iter()
            .find(|p| p.number == n)
            .is_some_and(|p| {
                p.exercises
                    .iter()
                    .filter(|e| e.checkpoint)
                    .all(|e| self.passed(&e.id))
            })
    }

    /// Phase 0 is always open; phase n opens when phase n-1's checkpoint is done.
    pub fn is_open(&self, course: &Curriculum, n: u8) -> bool {
        n == 0 || self.checkpoint_done(course, n - 1)
    }

    /// The highest open phase.
    pub fn current_phase(&self, course: &Curriculum) -> u8 {
        course
            .phases
            .iter()
            .map(|p| p.number)
            .filter(|&n| self.is_open(course, n))
            .max()
            .unwrap_or(0)
    }

    pub fn graduated(&self, course: &Curriculum) -> bool {
        self.checkpoint_done(course, 7)
    }

    /// Whether a game exercise's goal is now met, from its recorded games.
    pub fn game_goal_met(&self, exercise: &str, spec: &GameSpec) -> bool {
        let games: Vec<&Event> = self
            .events
            .iter()
            .filter(|e| matches!(e, Event::Game { exercise: x, .. } if x == exercise))
            .collect();
        let need = spec.need as usize;
        if spec.in_a_row {
            games.len() >= need
                && games[games.len() - need..]
                    .iter()
                    .all(|e| qualifies(e, spec))
        } else {
            games.iter().filter(|e| qualifies(e, spec)).count() >= need
        }
    }

    /// Checkpoint progress text of a game exercise, e.g. "2 of 3 in a row".
    pub fn game_status(&self, exercise: &str, spec: &GameSpec) -> String {
        let games: Vec<&Event> = self
            .events
            .iter()
            .filter(|e| matches!(e, Event::Game { exercise: x, .. } if x == exercise))
            .collect();
        let count = if spec.in_a_row {
            games
                .iter()
                .rev()
                .take_while(|e| qualifies(e, spec))
                .count()
        } else {
            games.iter().filter(|e| qualifies(e, spec)).count()
        };
        let tail = if spec.in_a_row { " in a row" } else { "" };
        format!("{} of {}{tail}", count.min(spec.need as usize), spec.need)
    }
}

/// One recorded game meets the exercise's conditions.
pub fn qualifies(event: &Event, spec: &GameSpec) -> bool {
    let Event::Game {
        result,
        natural,
        illegal,
        peeks,
        moves,
        quiz_right,
        quiz_total,
        reason,
        ..
    } = event
    else {
        return false;
    };
    let goal = match spec.goal {
        Goal::Mate => *result == 1,
        // A draw only counts if it was held on the board: losing the pawn leaves bare
        // kings ("insufficient material"), which is not holding anything.
        Goal::Hold => {
            *result == 1 || (*result == 0 && *natural && reason != "insufficient material")
        }
        Goal::Finish => *natural,
    };
    goal && (!spec.no_illegal || *illegal == 0)
        && (!spec.no_peeks || *peeks == 0)
        && spec.min_moves.is_none_or(|m| *moves >= m)
        && (!spec.perfect_quiz || quiz_right == quiz_total)
}

/// The exercise's phase number.
pub fn phase_of(course: &Curriculum, exercise: &str) -> u8 {
    course.exercise(exercise).map_or(0, |(p, _)| p.number)
}

/// Game spec of a game exercise.
pub fn game_spec<'a>(course: &'a Curriculum, exercise: &str) -> Option<&'a GameSpec> {
    match &course.exercise(exercise)?.1.kind {
        Kind::Game(spec) => Some(spec),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training::curriculum::curriculum;

    fn game(result: i32, illegal: u32) -> Event {
        Event::Game {
            date: "d".into(),
            exercise: "endgame_kq".into(),
            result,
            natural: true,
            illegal,
            peeks: 0,
            moves: 10,
            quiz_right: 0,
            quiz_total: 0,
            reason: "checkmate".into(),
        }
    }

    #[test]
    fn events_round_trip() {
        for e in [
            Event::Log {
                date: "2026-10-25".into(),
                phase: 1,
                exercise: "colors_random".into(),
                seconds: 40,
                right: 20,
                total: 20,
                passed: true,
            },
            Event::Pass {
                exercise: "diagnostic".into(),
                date: "2026-10-25".into(),
            },
            game(1, 0),
        ] {
            assert_eq!(Event::parse(&e.to_line()), Some(e));
        }
        assert!(Event::parse("2026-10-24\t1300\tb\t-1\t1200\t34\t2\tladder").is_none());
    }

    #[test]
    fn phases_unlock_in_order() {
        let c = curriculum();
        let mut p = Progress::default();
        assert!(p.is_open(c, 0) && !p.is_open(c, 1));
        p.events.push(Event::Pass {
            exercise: "diagnostic".into(),
            date: "d".into(),
        });
        assert!(p.is_open(c, 1) && !p.is_open(c, 2));
        assert_eq!(p.current_phase(c), 1);
    }

    #[test]
    fn games_in_a_row() {
        let c = curriculum();
        let spec = game_spec(c, "endgame_kq").unwrap();
        let mut p = Progress::default();
        p.events
            .extend([game(1, 0), game(0, 0), game(1, 0), game(1, 0)]);
        assert!(!p.game_goal_met("endgame_kq", spec));
        assert_eq!(p.game_status("endgame_kq", spec), "2 of 3 in a row");
        p.events.push(game(1, 0));
        assert!(p.game_goal_met("endgame_kq", spec));
    }

    #[test]
    fn losing_the_pawn_is_not_holding() {
        let spec = game_spec(curriculum(), "endgame_kp").unwrap();
        let draw = |reason: &str| Event::Game {
            date: "d".into(),
            exercise: "endgame_kp".into(),
            result: 0,
            natural: true,
            illegal: 0,
            peeks: 0,
            moves: 20,
            quiz_right: 0,
            quiz_total: 0,
            reason: reason.into(),
        };
        assert!(!qualifies(&draw("insufficient material"), spec));
        assert!(qualifies(&draw("stalemate"), spec));
        // Old lines (no reason) still parse.
        let old = "game\td\tendgame_kq\t1\t1\t0\t0\t10\t0\t0";
        assert!(matches!(Event::parse(old), Some(Event::Game { reason, .. }) if reason.is_empty()));
    }
}
