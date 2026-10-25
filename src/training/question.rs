//! A question, its exact answer, and grading. Every answer is checked automatically.

use crate::game::{looks_like_a_move, parse_move};
use crate::training::{
    board_math::{is_shortest_knight_route, knight_route},
    mates::{forces_mate_in_2, mate_in_2_keys, mating_moves, toughest_reply},
    notation::{format_pieces, format_squares, parse_pieces, parse_squares, same_squares},
};
use shakmaty::{Chess, Move, Position, Role, Square, san::SanPlus};

pub enum Answer {
    /// Not graded: the user reads something (a position to memorize) and presses Enter.
    Ready,
    Color {
        light: bool,
    },
    Square(Square),
    Squares(Vec<Square>),
    /// Same diagonal? `None` means no; otherwise the path, ends included.
    Diagonal(Option<Vec<Square>>),
    Count(u32),
    YesNo(bool),
    Choice {
        options: Vec<String>,
        correct: usize,
    },
    /// A move in the question's position: one of `keys` (SAN), or any mate when `mate`.
    Move {
        keys: Vec<String>,
        mate: bool,
    },
    /// First move of a mate in two; the app then plays a defense and asks for the mate.
    MateIn2,
    Route {
        from: Square,
        to: Square,
    },
    Pieces(Vec<(Role, Square)>),
}

/// What to draw next to the question, e.g. the coordinate trainer's highlighted square.
#[derive(Clone, Copy)]
pub struct BoardView {
    pub highlight: Square,
    pub white_side: bool,
}

pub struct Question {
    pub prompt: String,
    pub answer: Answer,
    /// Needed by `Move` and `MateIn2`.
    pub position: Option<Chess>,
    pub board: Option<BoardView>,
    /// Shown after answering, right or wrong.
    pub note: Option<String>,
    /// Counts for the score. Practice steps (recite the position) are corrected but not scored.
    pub scored: bool,
}

pub enum Verdict {
    Right,
    /// Wrong; the text is the expected answer.
    Wrong(String),
    /// The input could not be read (typo); not counted.
    Unreadable(String),
    /// Right so far; answer this follow-up next (the message says what happened).
    Next(Box<Question>, String),
}

impl Question {
    pub fn new(prompt: impl Into<String>, answer: Answer) -> Question {
        Question {
            prompt: prompt.into(),
            answer,
            position: None,
            board: None,
            note: None,
            scored: true,
        }
    }

    pub fn at(mut self, pos: Chess) -> Question {
        self.position = Some(pos);
        self
    }

    pub fn noting(mut self, note: Option<String>) -> Question {
        self.note = note;
        self
    }

    /// Counted towards the score (not `Ready`, not practice steps).
    pub fn graded(&self) -> bool {
        self.scored && !matches!(self.answer, Answer::Ready)
    }

    pub fn unscored(mut self) -> Question {
        self.scored = false;
        self
    }

    pub fn showing(mut self, view: BoardView) -> Question {
        self.board = Some(view);
        self
    }

    /// A short hint of the expected input format.
    pub fn format_hint(&self) -> &'static str {
        match self.answer {
            Answer::Ready => "press Enter when ready",
            Answer::Color { .. } => "light / dark",
            Answer::Square(_) => "a square, e.g. f7",
            Answer::Squares(_) => "squares, e.g. c3 c5 d2 (or none)",
            Answer::Diagonal(_) => "no, or the squares of the path",
            Answer::Count(_) => "a number",
            Answer::YesNo(_) => "yes / no",
            Answer::Choice { .. } => "the option's letter or name",
            Answer::Move { .. } | Answer::MateIn2 => "a move, e.g. Ra8",
            Answer::Route { .. } => "the route, e.g. g1 f3 e5",
            Answer::Pieces(_) => "pieces, e.g. Kg1 Rd1 f2 g2",
        }
    }

    pub fn expected(&self) -> String {
        let pos = self.position.as_ref();
        let sans = |moves: Vec<Move>| -> String {
            let pos = pos.expect("move questions carry a position");
            moves
                .into_iter()
                .map(|m| SanPlus::from_move(pos.clone(), m).to_string())
                .collect::<Vec<_>>()
                .join(" or ")
        };
        match &self.answer {
            Answer::Ready => String::new(),
            Answer::Color { light } => (if *light { "light" } else { "dark" }).into(),
            Answer::Square(sq) => sq.to_string(),
            Answer::Squares(sqs) => format_squares(sqs),
            Answer::Diagonal(None) => "no".into(),
            Answer::Diagonal(Some(path)) => format!("yes: {}", format_squares(path)),
            Answer::Count(n) => n.to_string(),
            Answer::YesNo(b) => (if *b { "yes" } else { "no" }).into(),
            Answer::Choice { options, correct } => options[*correct].clone(),
            Answer::Move { keys, mate: false } => keys.join(" or "),
            Answer::Move { mate: true, .. } => sans(mating_moves(pos.unwrap())),
            Answer::MateIn2 => sans(mate_in_2_keys(pos.unwrap())),
            Answer::Route { from, to } => format_squares(&knight_route(*from, *to)),
            Answer::Pieces(p) => format_pieces(p),
        }
    }

    pub fn grade(&self, input: &str) -> Verdict {
        let text = input.trim().to_ascii_lowercase();
        let right = |ok: bool| {
            if ok {
                Verdict::Right
            } else {
                Verdict::Wrong(self.expected())
            }
        };
        let unreadable = || Verdict::Unreadable(format!("Expected {}", self.format_hint()));
        // An empty Enter is a slip, not an answer (type "none" for an empty list).
        if text.is_empty() && !matches!(self.answer, Answer::Ready) {
            return unreadable();
        }
        let yes_no = || match text.as_str() {
            "y" | "yes" => Some(true),
            "n" | "no" => Some(false),
            _ => None,
        };
        match &self.answer {
            Answer::Ready => Verdict::Right,
            Answer::Color { light } => match text.as_str() {
                "l" | "light" | "white" => right(*light),
                "d" | "dark" | "black" => right(!*light),
                _ => unreadable(),
            },
            Answer::Square(sq) => match parse_squares(&text).as_deref() {
                Some([given]) => right(given == sq),
                _ => unreadable(),
            },
            Answer::Squares(sqs) => match parse_squares(&text) {
                Some(given) => right(same_squares(&given, sqs)),
                None => unreadable(),
            },
            Answer::Diagonal(path) => match (yes_no(), parse_squares(&text), path) {
                (Some(false), _, None) => Verdict::Right,
                (Some(_), _, _) => right(false),
                // Neighbours have no squares in between: "none" is not an answer for them.
                (None, Some(given), Some(_)) if given.is_empty() => right(false),
                (None, Some(given), Some(path)) => {
                    let inner = &path[1..path.len() - 1];
                    right(same_squares(&given, path) || same_squares(&given, inner))
                }
                (None, Some(_), None) => right(false),
                (None, None, _) => unreadable(),
            },
            Answer::Count(n) => match text.parse::<u32>() {
                Ok(given) => right(given == *n),
                Err(_) => unreadable(),
            },
            Answer::YesNo(b) => match yes_no() {
                Some(given) => right(given == *b),
                None => unreadable(),
            },
            Answer::Choice { options, correct } => {
                let by_letter = (text.len() == 1)
                    .then(|| text.as_bytes()[0].wrapping_sub(b'a') as usize)
                    .filter(|&i| i < options.len());
                let by_name = options.iter().position(|o| o.to_ascii_lowercase() == text);
                match by_letter.or(by_name) {
                    Some(i) => right(i == *correct),
                    None => unreadable(),
                }
            }
            Answer::Move { keys, mate } => self.grade_move(input, keys, *mate),
            Answer::MateIn2 => self.grade_mate_in_2(input),
            Answer::Route { from, to } => match parse_squares(&text) {
                Some(path) => right(is_shortest_knight_route(&path, *from, *to)),
                None => unreadable(),
            },
            Answer::Pieces(expected) => match parse_pieces(input) {
                Some(given) => right(&given == expected),
                None => unreadable(),
            },
        }
    }

    /// A well-formed move that can't be played is wrong; anything else is a typo.
    fn not_a_move(&self, input: &str) -> Verdict {
        if looks_like_a_move(input) {
            Verdict::Wrong(self.expected())
        } else {
            Verdict::Unreadable("Expected a move, e.g. Ra8".into())
        }
    }

    fn grade_move(&self, input: &str, keys: &[String], mate: bool) -> Verdict {
        let pos = self
            .position
            .as_ref()
            .expect("move questions carry a position");
        let Some(m) = parse_move(pos, input.trim()) else {
            return self.not_a_move(input);
        };
        let keyed = keys.iter().any(|k| parse_move(pos, k) == Some(m));
        let mates = mate && {
            let mut next = pos.clone();
            next.play_unchecked(m);
            next.is_checkmate()
        };
        if keyed || mates {
            Verdict::Right
        } else {
            Verdict::Wrong(self.expected())
        }
    }

    fn grade_mate_in_2(&self, input: &str) -> Verdict {
        let pos = self
            .position
            .as_ref()
            .expect("mate questions carry a position");
        let Some(m) = parse_move(pos, input.trim()) else {
            return self.not_a_move(input);
        };
        if !forces_mate_in_2(pos, m) {
            return Verdict::Wrong(self.expected());
        }
        let mut next = pos.clone();
        next.play_unchecked(m);
        let Some(reply) = toughest_reply(&next) else {
            return Verdict::Right; // it was already mate
        };
        let reply_san = SanPlus::from_move_and_play_unchecked(&mut next, reply).to_string();
        let follow = Question::new(
            "Mate in one:",
            Answer::Move {
                keys: vec![],
                mate: true,
            },
        )
        .at(next);
        Verdict::Next(
            Box::new(follow),
            format!("Good. Black replies {reply_san}."),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training::notation::{parse_board, to_position};
    use shakmaty::{Color, Square::*};

    fn ok(v: Verdict) -> bool {
        matches!(v, Verdict::Right)
    }

    #[test]
    fn simple_answers() {
        let q = Question::new("Color of e4?", Answer::Color { light: true });
        assert!(ok(q.grade("light")) && ok(q.grade("L")) && !ok(q.grade("dark")));
        assert!(matches!(q.grade("blue"), Verdict::Unreadable(_)));
        let q = Question::new("Knight e4?", Answer::Squares(vec![C3, C5, D2]));
        assert!(ok(q.grade("d2, c5 c3")) && !ok(q.grade("c3 c5")));
        let q = Question::new(
            "Pick",
            Answer::Choice {
                options: vec!["fork".into(), "pin".into()],
                correct: 1,
            },
        );
        assert!(ok(q.grade("b")) && ok(q.grade("Pin")) && !ok(q.grade("a")));
    }

    #[test]
    fn diagonal_answers() {
        let q = Question::new("b3-f7?", Answer::Diagonal(Some(vec![B3, C4, D5, E6, F7])));
        assert!(ok(q.grade("b3 c4 d5 e6 f7")) && ok(q.grade("c4 d5 e6")) && !ok(q.grade("no")));
        let q = Question::new("b2-g6?", Answer::Diagonal(None));
        assert!(ok(q.grade("no")) && !ok(q.grade("c3 d4")));
    }

    #[test]
    fn move_answers() {
        let pos = to_position(
            &parse_board("White: Kg6, Ra1. Black: Kg8.").unwrap(),
            Color::White,
        )
        .unwrap();
        let q = Question::new(
            "Mate",
            Answer::Move {
                keys: vec![],
                mate: true,
            },
        )
        .at(pos);
        assert!(ok(q.grade("Ra8")) && ok(q.grade("ra8")) && !ok(q.grade("Ra7")));
        let pos = to_position(
            &parse_board("White: Kf6, Ra2. Black: Kg8.").unwrap(),
            Color::White,
        )
        .unwrap();
        let q = Question::new("Mate in 2", Answer::MateIn2).at(pos);
        assert!(!ok(q.grade("Ra8")));
        let Verdict::Next(follow, _) = q.grade("Rh2") else {
            panic!("Rh2 forces mate")
        };
        assert!(ok(follow.grade("Rh8")));
    }

    #[test]
    fn slips_are_not_counted() {
        let q = Question::new("Knight e4?", Answer::Squares(vec![C3, C5]));
        assert!(matches!(q.grade("   "), Verdict::Unreadable(_)));
        // c1-d2 are neighbours on a diagonal: "none" is not the path.
        let q = Question::new("c1-d2?", Answer::Diagonal(Some(vec![C1, D2])));
        assert!(!ok(q.grade("none")) && ok(q.grade("c1 d2")));
        let pos = to_position(
            &parse_board("White: Kf6, Ra2. Black: Kg8.").unwrap(),
            Color::White,
        )
        .unwrap();
        let q = Question::new("Mate in 2", Answer::MateIn2).at(pos);
        assert!(matches!(q.grade("Rh"), Verdict::Unreadable(_)));
        assert!(matches!(q.grade("Rb8"), Verdict::Wrong(_)));
    }

    #[test]
    fn route_and_pieces() {
        let q = Question::new("g1-e5", Answer::Route { from: G1, to: E5 });
        assert!(ok(q.grade("g1 f3 e5")) && !ok(q.grade("g1 e2 e5")));
        let q = Question::new(
            "Black?",
            Answer::Pieces(vec![(Role::King, G8), (Role::Pawn, H7)]),
        );
        assert!(ok(q.grade("h7 Kg8")) && !ok(q.grade("Kg8")));
    }
}
