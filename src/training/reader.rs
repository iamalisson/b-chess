//! Following a line (opening, trap or classic game) from its moves: shown in chunks with
//! stop questions between them, read straight through then rebuilt, or recited from memory.

use crate::game::{looks_like_a_move, parse_move};
use crate::training::{
    curriculum::{Line, LineMode, Pass, answer_question},
    drills::recall_both,
    notation::{Ply, locate, play_sequence},
    question::Question,
    quiz::{Feedback, QuizRun},
};
use shakmaty::Chess;

pub enum Stage {
    /// Moves on screen; Enter goes on.
    Reading,
    /// Answering questions (a stop, or the final recall).
    Asking(Box<QuizRun>),
    /// Typing the moves from memory.
    Reciting,
    Done,
}

pub struct LineRun {
    pub title: String,
    pub intro: String,
    pub mode: LineMode,
    sans: Vec<String>,
    plies: Vec<Ply>,
    /// (ply count, question) for every stop, in order.
    stops: Vec<(usize, Question)>,
    /// Moves revealed so far.
    pub shown: usize,
    pub stage: Stage,
    pub right: u32,
    pub total: u32,
    pub errors: u32,
    pub feedback: Option<Feedback>,
}

impl LineRun {
    pub fn new(line: &Line, mode: LineMode) -> LineRun {
        let sans = line.sans();
        let plies =
            play_sequence(&Chess::default(), &sans).expect("lines are legal (checked by tests)");
        let stops = line
            .stops
            .iter()
            .map(|s| {
                let ply = locate(&sans, &s.after).expect("stops resolve (checked by tests)");
                let q = answer_question(&s.prompt, &s.answer, &s.options).noting(s.note.clone());
                (ply, q)
            })
            .collect();
        let mut run = LineRun {
            title: line.title.clone(),
            intro: line.intro.clone(),
            mode,
            sans,
            plies,
            stops,
            shown: 0,
            stage: Stage::Reading,
            right: 0,
            total: 0,
            errors: 0,
            feedback: None,
        };
        match mode {
            LineMode::Stops => run.shown = run.next_stop_after(0),
            LineMode::Read => run.shown = run.sans.len(),
            LineMode::Recite => run.stage = Stage::Reciting,
        }
        run
    }

    fn next_stop_after(&self, ply: usize) -> usize {
        self.stops
            .iter()
            .map(|&(p, _)| p)
            .find(|&p| p > ply)
            .unwrap_or(self.sans.len())
    }

    /// The revealed moves, numbered ("1.e4 e5 2.Nf3 ...").
    pub fn moves_text(&self) -> String {
        numbered(&self.sans[..self.shown])
    }

    /// Stop questions are waiting at the current move.
    pub fn questions_pending(&self) -> bool {
        self.stops.iter().any(|(p, _)| *p == self.shown)
    }

    pub fn total_moves(&self) -> usize {
        self.sans.len()
    }

    /// Enter while reading.
    pub fn go_on(&mut self) {
        if !matches!(self.stage, Stage::Reading) {
            return;
        }
        let questions: Vec<_> = match self.mode {
            LineMode::Stops => {
                let here = self.shown;
                let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.stops)
                    .into_iter()
                    .partition(|(p, _)| *p == here);
                self.stops = later;
                if now.is_empty() {
                    self.stage = Stage::Done; // the moves after the last stop were shown
                    return;
                }
                now.into_iter().map(|(_, q)| q).collect()
            }
            LineMode::Read => recall_both(&self.plies.last().expect("lines have moves").after),
            LineMode::Recite => return,
        };
        self.stage = Stage::Asking(Box::new(QuizRun::fixed(questions, Pass::default())));
    }

    /// An answer while asking or reciting.
    pub fn answer(&mut self, input: &str) {
        match &mut self.stage {
            Stage::Asking(quiz) => {
                quiz.answer(input);
                self.feedback = quiz.feedback.take();
                if quiz.is_done() {
                    self.right += quiz.right;
                    self.total += quiz.total;
                    self.after_questions();
                }
            }
            Stage::Reciting => self.recite(input),
            _ => {}
        }
    }

    fn after_questions(&mut self) {
        if self.mode == LineMode::Stops && self.shown < self.sans.len() {
            self.shown = self.next_stop_after(self.shown);
            self.stage = Stage::Reading;
        } else {
            self.stage = Stage::Done;
        }
    }

    fn recite(&mut self, input: &str) {
        let pos = if self.shown == 0 {
            Chess::default()
        } else {
            self.plies[self.shown - 1].after.clone()
        };
        let expected = self.plies[self.shown].mv;
        if parse_move(&pos, input).is_none() && !looks_like_a_move(input) {
            // A typo or an empty Enter: say so, don't count it, stay on this move.
            self.feedback = Some(Feedback::Hint("Expected a move, e.g. Nf3".into()));
            return;
        }
        match parse_move(&pos, input.trim()) {
            Some(m) if m == expected => {
                self.right += 1;
                self.feedback = None;
            }
            _ => {
                self.errors += 1;
                self.feedback = Some(Feedback::Wrong {
                    expected: self.plies[self.shown].san.clone(),
                    note: None,
                });
            }
        }
        self.total += 1;
        self.shown += 1;
        if self.shown == self.sans.len() {
            self.stage = Stage::Done;
        }
    }

    /// "Move 5, White:" for reciting.
    pub fn recite_prompt(&self) -> String {
        let side = if self.shown.is_multiple_of(2) {
            "White"
        } else {
            "Black"
        };
        format!("Move {}, {side}:", self.shown / 2 + 1)
    }

    pub fn is_done(&self) -> bool {
        matches!(self.stage, Stage::Done)
    }

    pub fn passed(&self) -> bool {
        match self.mode {
            LineMode::Recite => self.errors == 0,
            _ => self.total > 0 && self.right == self.total,
        }
    }
}

/// "1.e4 e5 2.Nf3" from SAN moves starting with White.
pub fn numbered(sans: &[String]) -> String {
    sans.chunks(2)
        .enumerate()
        .map(|(i, pair)| format!("{}.{}", i + 1, pair.join(" ")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training::curriculum::curriculum;

    #[test]
    fn stops_then_done() {
        let line = curriculum().line("scholars_mate").unwrap();
        let mut run = LineRun::new(line, LineMode::Stops);
        assert_eq!(run.moves_text(), "1.e4 e5 2.Bc4 Nc6 3.Qh5");
        run.go_on();
        run.answer("f7");
        assert!(matches!(run.stage, Stage::Reading));
        assert_eq!(run.shown, 7);
        run.go_on();
        run.answer("c4");
        run.answer("none");
        assert!(run.is_done() && run.passed());
    }

    #[test]
    fn recite_counts_errors() {
        let line = curriculum().line("fools_mate").unwrap();
        let mut run = LineRun::new(line, LineMode::Recite);
        for m in ["f3", "", "e5", "g3", "Qh4#"] {
            run.answer(m); // the empty Enter is not counted
        }
        assert!(run.is_done());
        assert_eq!((run.right, run.errors), (3, 1));
        assert!(!run.passed());
    }

    #[test]
    fn read_ends_with_recall() {
        let line = curriculum().line("fools_mate").unwrap();
        let mut run = LineRun::new(line, LineMode::Read);
        run.go_on();
        run.answer("Ke1 Qd1 Ra1 Rh1 Bc1 Bf1 Nb1 Ng1 a2 b2 c2 d2 e2 f3 g4 h2");
        run.answer("Ke8 Qh4 Ra8 Rh8 Bc8 Bf8 Nb8 Ng8 a7 b7 c7 d7 e5 f7 g7 h7");
        assert!(run.is_done() && run.passed());
    }
}
