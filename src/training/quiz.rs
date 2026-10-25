//! Running a set of questions: fixed ones or a generator, timing, streaks and pass rules.

use crate::levels::Rng;
use crate::training::{
    curriculum::{Generator, Pass},
    drills::generate,
    question::{Question, Verdict},
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// What the user sees after an answer.
pub enum Feedback {
    Right {
        note: Option<String>,
        slow: bool,
    },
    Wrong {
        expected: String,
        note: Option<String>,
    },
    /// Unreadable input; nothing counted.
    Hint(String),
    /// Right so far; a follow-up question comes next.
    Info(String),
}

pub struct QuizRun {
    queue: VecDeque<Question>,
    generator: Option<(Generator, Rng)>,
    /// Questions still to generate.
    remaining: u32,
    pub current: Option<Question>,
    asked: Instant,
    started: Instant,
    deadline: Option<Instant>,
    time_limit: Option<Duration>,
    pass: Pass,
    pub right: u32,
    pub total: u32,
    streak: u32,
    pub best_streak: u32,
    pub feedback: Option<Feedback>,
}

impl QuizRun {
    pub fn fixed(questions: Vec<Question>, pass: Pass) -> QuizRun {
        let mut run = QuizRun::empty(pass);
        run.queue = questions.into();
        run.next();
        run
    }

    pub fn drill(generator: Generator, count: u32, time_limit: Option<u64>, pass: Pass) -> QuizRun {
        let mut run = QuizRun::empty(pass);
        run.generator = Some((generator, Rng::new()));
        run.remaining = count;
        run.time_limit = time_limit.map(Duration::from_secs);
        run.deadline = run.time_limit.map(|t| Instant::now() + t);
        run.next();
        run
    }

    fn empty(pass: Pass) -> QuizRun {
        QuizRun {
            queue: VecDeque::new(),
            generator: None,
            remaining: 0,
            current: None,
            asked: Instant::now(),
            started: Instant::now(),
            deadline: None,
            time_limit: None,
            pass,
            right: 0,
            total: 0,
            streak: 0,
            best_streak: 0,
            feedback: None,
        }
    }

    fn next(&mut self) {
        self.current = self.queue.pop_front().or_else(|| {
            let (generator, rng) = self.generator.as_mut()?;
            (self.remaining > 0).then(|| {
                self.remaining -= 1;
                generate(*generator, rng)
            })
        });
        self.asked = Instant::now();
    }

    pub fn is_done(&self) -> bool {
        self.current.is_none() || self.time_left() == Some(Duration::ZERO)
    }

    pub fn time_left(&self) -> Option<Duration> {
        self.deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
    }

    /// Time spent, capped at the time limit (a timed drill can sit finished until noticed).
    pub fn seconds(&self) -> u64 {
        let spent = self.started.elapsed();
        self.time_limit
            .map_or(spent, |limit| spent.min(limit))
            .as_secs()
    }

    pub fn answer(&mut self, input: &str) {
        let Some(q) = &self.current else { return };
        if self.is_done() {
            return;
        }
        let secs = self.asked.elapsed().as_secs_f64();
        let note = q.note.clone();
        match q.grade(input) {
            Verdict::Unreadable(hint) => {
                self.feedback = Some(Feedback::Hint(hint));
                return;
            }
            Verdict::Next(follow, info) => {
                self.current = Some(*follow);
                self.feedback = Some(Feedback::Info(info));
                return; // same question continues, the timer keeps running
            }
            Verdict::Right if !q.graded() => {
                self.feedback = Some(Feedback::Right { note, slow: false });
            }
            Verdict::Right => {
                let slow = self.pass.max_seconds.is_some_and(|max| secs > max);
                self.right += 1;
                self.total += 1;
                self.streak = if slow { 0 } else { self.streak + 1 };
                self.best_streak = self.best_streak.max(self.streak);
                self.feedback = Some(Feedback::Right { note, slow });
            }
            Verdict::Wrong(expected) if !q.graded() => {
                self.feedback = Some(Feedback::Wrong { expected, note });
            }
            Verdict::Wrong(expected) => {
                self.total += 1;
                self.streak = 0;
                self.feedback = Some(Feedback::Wrong { expected, note });
            }
        }
        self.next();
    }

    pub fn passed(&self) -> bool {
        match (self.pass.min_correct, self.pass.streak) {
            (_, Some(n)) => self.best_streak >= n,
            (Some(n), None) => self.right >= n,
            (None, None) => self.total > 0 && self.right == self.total,
        }
    }

    /// Questions left (fixed quizzes) or still to generate.
    pub fn left(&self) -> usize {
        self.queue.len() + self.remaining as usize + usize::from(self.current.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training::question::Answer;

    fn color(light: bool) -> Question {
        Question::new("?", Answer::Color { light })
    }

    #[test]
    fn scores_and_passes() {
        let mut q = QuizRun::fixed(vec![color(true), color(false)], Pass::default());
        q.answer("light");
        q.answer("blue"); // unreadable: not counted
        assert_eq!((q.right, q.total), (1, 1));
        q.answer("light");
        assert!(q.is_done() && !q.passed());

        let pass = Pass {
            min_correct: Some(1),
            ..Pass::default()
        };
        let mut q = QuizRun::fixed(vec![color(true), color(true)], pass);
        q.answer("dark");
        q.answer("light");
        assert!(q.passed());
    }

    #[test]
    fn streak_needs_speed() {
        let pass = Pass {
            streak: Some(2),
            max_seconds: Some(60.0),
            ..Pass::default()
        };
        let mut q = QuizRun::fixed(vec![color(true), color(true), color(true)], pass);
        q.answer("light");
        q.answer("dark");
        q.answer("light");
        assert!(!q.passed());
    }
}
