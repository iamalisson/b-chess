//! The course content (`assets/curriculum.toml`): its types, and turning question specs
//! into graded [`Question`]s. A test checks every answer against the real board.

use crate::training::{
    board_math::{diagonal_path, diagonals_through},
    drills::{
        attack_count_question, attacks_question, color_question, route_question,
        same_diagonal_question, walk_question,
    },
    notation::{parse_board, parse_sequence, parse_square, parse_squares, pieces_of, to_position},
    question::{Answer, Question},
};
use serde::Deserialize;
use shakmaty::{Color, Role, Square};
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Curriculum {
    pub phases: Vec<Phase>,
    pub lines: Vec<Line>,
    pub stuck: Vec<Tip>,
}

#[derive(Deserialize)]
pub struct Phase {
    pub number: u8,
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub notes: String,
    /// Today's session: exercise ids and minutes.
    #[serde(default)]
    pub routine: Vec<(String, u32)>,
    pub exercises: Vec<Exercise>,
}

#[derive(Deserialize)]
pub struct Exercise {
    pub id: String,
    pub title: String,
    /// Part of the phase's checkpoint: needed to unlock the next phase.
    #[serde(default)]
    pub checkpoint: bool,
    #[serde(flatten)]
    pub kind: Kind,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    /// Fixed questions (or another quiz's, via `from`).
    Quiz {
        #[serde(default)]
        questions: Vec<QSpec>,
        #[serde(default)]
        from: Option<String>,
        /// Mate positions are shown once, then hidden and recited before solving.
        #[serde(default)]
        recall: bool,
        pass: Pass,
    },
    /// Random questions from a generator.
    Drill {
        generator: Generator,
        count: u32,
        /// Seconds for the whole drill (coordinate trainer).
        #[serde(default)]
        time_limit: Option<u64>,
        pass: Pass,
    },
    /// A move sequence: answer stops, read it through, or recite it.
    Line { line: String, mode: LineMode },
    /// A game against Lc0.
    Game(GameSpec),
}

/// Pass rule; with nothing set, every answer must be right.
#[derive(Deserialize, Default, Clone, Copy)]
#[serde(default)]
pub struct Pass {
    pub min_correct: Option<u32>,
    /// That many right answers in a row...
    pub streak: Option<u32>,
    /// ...each within this many seconds.
    pub max_seconds: Option<f64>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Generator {
    Color,
    SameDiagonal,
    Walk,
    CoordinatesWhite,
    CoordinatesBlack,
    Attacks,
    KnightAttacks,
    KnightRoute,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum LineMode {
    Stops,
    Read,
    Recite,
}

#[derive(Deserialize, Clone)]
pub struct GameSpec {
    /// Start position description; the standard position when absent.
    pub start: Option<String>,
    /// Lc0 level (Elo); the ladder level when absent (endgames use a strong defender).
    pub level: Option<u32>,
    #[serde(default)]
    pub ghost: bool,
    #[serde(default)]
    pub check_habit: bool,
    #[serde(default)]
    pub piece_questions: bool,
    pub goal: Goal,
    pub need: u32,
    #[serde(default)]
    pub in_a_row: bool,
    #[serde(default)]
    pub min_moves: Option<u32>,
    #[serde(default)]
    pub no_illegal: bool,
    #[serde(default)]
    pub no_peeks: bool,
    #[serde(default)]
    pub perfect_quiz: bool,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Goal {
    /// Win by mate.
    Mate,
    /// Don't lose: win or draw.
    Hold,
    /// Play the game to its end, any result.
    Finish,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QSpec {
    Color {
        square: String,
    },
    SameDiagonal {
        a: String,
        b: String,
    },
    DiagonalSquares {
        from: String,
        to: String,
    },
    DiagonalsThrough {
        square: String,
    },
    Walk {
        from: String,
        steps: Vec<(i32, i32)>,
    },
    Attacks {
        piece: String,
        square: String,
    },
    AttackCount {
        piece: String,
        square: String,
    },
    Route {
        from: String,
        to: String,
    },
    Mate1 {
        position: String,
        note: Option<String>,
    },
    Mate2 {
        position: String,
        hint: Option<String>,
        note: Option<String>,
    },
    Ask(Ask),
}

#[derive(Deserialize)]
pub struct Ask {
    pub prompt: String,
    /// Where the question happens; the content tests check `verify` against it.
    #[allow(dead_code)]
    pub position: Option<String>,
    #[allow(dead_code)]
    pub moves: Option<String>,
    pub answer: AnswerSpec,
    #[serde(default)]
    pub options: Vec<String>,
    /// Checked by the content tests.
    #[allow(dead_code)]
    pub verify: Option<String>,
    pub note: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AnswerSpec {
    Square(String),
    Squares(String),
    Count(u32),
    YesNo(bool),
    Choice(String),
}

#[derive(Deserialize)]
pub struct Line {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub intro: String,
    pub moves: String,
    #[serde(default)]
    pub stops: Vec<Stop>,
}

#[derive(Deserialize)]
pub struct Stop {
    /// "4...g6" or "5.Nxe5": the stop comes after this move.
    pub after: String,
    pub prompt: String,
    pub answer: AnswerSpec,
    #[serde(default)]
    pub options: Vec<String>,
    /// Checked by the content tests.
    #[allow(dead_code)]
    pub verify: Option<String>,
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct Tip {
    pub problem: String,
    pub advice: String,
}

/// The embedded course, parsed once. The content test guarantees it parses.
pub fn curriculum() -> &'static Curriculum {
    static CURRICULUM: OnceLock<Curriculum> = OnceLock::new();
    CURRICULUM.get_or_init(|| {
        toml::from_str(include_str!("../../assets/curriculum.toml"))
            .expect("assets/curriculum.toml is valid (checked by tests)")
    })
}

impl Curriculum {
    pub fn exercise(&self, id: &str) -> Option<(&Phase, &Exercise)> {
        self.phases
            .iter()
            .flat_map(|p| p.exercises.iter().map(move |e| (p, e)))
            .find(|(_, e)| e.id == id)
    }

    pub fn line(&self, id: &str) -> Option<&Line> {
        self.lines.iter().find(|l| l.id == id)
    }

    /// A quiz's questions, following `from` to another quiz.
    pub fn quiz_questions(&self, exercise: &Exercise) -> Vec<Question> {
        match &exercise.kind {
            Kind::Quiz { from: Some(id), .. } => match self.exercise(id) {
                Some((_, source)) => self.quiz_questions(source),
                None => Vec::new(),
            },
            Kind::Quiz {
                questions, recall, ..
            } => questions.iter().flat_map(|q| q.build(*recall)).collect(),
            _ => Vec::new(),
        }
    }
}

pub fn role_name(role: Role) -> &'static str {
    match role {
        Role::King => "king",
        Role::Queen => "queen",
        Role::Rook => "rook",
        Role::Bishop => "bishop",
        Role::Knight => "knight",
        Role::Pawn => "pawn",
    }
}

fn square(s: &str) -> Square {
    parse_square(s).unwrap_or_else(|| panic!("bad square {s:?} in the curriculum"))
}

fn role(letter: &str) -> Role {
    letter
        .chars()
        .next()
        .and_then(|c| Role::from_char(c.to_ascii_lowercase()))
        .unwrap_or_else(|| panic!("bad piece {letter:?} in the curriculum"))
}

/// Memorize the position, recite both sides, then answer with the position hidden.
fn with_recall(desc: &str, last: Question, recall: bool) -> Vec<Question> {
    if !recall {
        let prompt = format!("{desc}\n\n{}", last.prompt);
        return vec![Question { prompt, ..last }];
    }
    let board = parse_board(desc).expect("valid position (checked by tests)");
    let side = |color, name| {
        Question::new(
            format!("Recite the position. List {name}'s pieces."),
            Answer::Pieces(pieces_of(&board, color)),
        )
        .unscored()
    };
    vec![
        Question::new(
            format!(
                "Read the position once, out loud, then press Enter. It will be hidden.\n\n{desc}"
            ),
            Answer::Ready,
        ),
        side(Color::White, "White"),
        side(Color::Black, "Black"),
        last,
    ]
}

impl QSpec {
    pub fn build(&self, recall: bool) -> Vec<Question> {
        let one = |q: Question| vec![q];
        match self {
            QSpec::Color { square: s } => one(color_question(square(s))),
            QSpec::SameDiagonal { a, b } => one(same_diagonal_question(square(a), square(b))),
            QSpec::DiagonalSquares { from, to } => {
                let (a, b) = (square(from), square(to));
                one(Question::new(
                    format!("List the squares of the diagonal from {a} to {b}."),
                    Answer::Diagonal(diagonal_path(a, b)),
                ))
            }
            QSpec::DiagonalsThrough { square: s } => {
                let sq = square(s);
                let mut all: Vec<Square> = diagonals_through(sq).concat();
                all.retain(|&x| x != sq);
                one(Question::new(
                    format!(
                        "List every square on the two diagonals through {sq} (not {sq} itself)."
                    ),
                    Answer::Squares(all),
                ))
            }
            QSpec::Walk { from, steps } => one(walk_question(square(from), steps)
                .expect("walks stay on the board (checked by tests)")),
            QSpec::Attacks { piece, square: s } => one(attacks_question(role(piece), square(s))),
            QSpec::AttackCount { piece, square: s } => {
                one(attack_count_question(role(piece), square(s)))
            }
            QSpec::Route { from, to } => one(route_question(square(from), square(to))),
            QSpec::Mate1 { position, note } => {
                let pos = playable(position);
                let q = Question::new(
                    "White to play and mate in one.",
                    Answer::Move {
                        keys: vec![],
                        mate: true,
                    },
                )
                .at(pos)
                .noting(note.clone());
                with_recall(position, q, recall)
            }
            QSpec::Mate2 {
                position,
                hint,
                note,
            } => {
                let pos = playable(position);
                let hint = hint
                    .as_ref()
                    .map_or(String::new(), |h| format!(" Hint: {h}"));
                let q = Question::new(
                    format!("White to play and mate in two. Your first move?{hint}"),
                    Answer::MateIn2,
                )
                .at(pos)
                .noting(note.clone());
                with_recall(position, q, recall)
            }
            QSpec::Ask(ask) => {
                one(answer_question(&ask.prompt, &ask.answer, &ask.options)
                    .noting(ask.note.clone()))
            }
        }
    }
}

fn playable(desc: &str) -> shakmaty::Chess {
    let board = parse_board(desc).expect("valid position (checked by tests)");
    to_position(&board, Color::White).expect("legal position (checked by tests)")
}

/// A question from an explicit answer (asks and line stops).
pub fn answer_question(prompt: &str, answer: &AnswerSpec, options: &[String]) -> Question {
    let squares = |s: &str| parse_squares(s).expect("valid squares (checked by tests)");
    let answer = match answer {
        AnswerSpec::Square(s) => Answer::Square(square(s)),
        AnswerSpec::Squares(s) => Answer::Squares(squares(s)),
        AnswerSpec::Count(n) => Answer::Count(*n),
        AnswerSpec::YesNo(b) => Answer::YesNo(*b),
        AnswerSpec::Choice(c) => Answer::Choice {
            options: options.to_vec(),
            correct: options
                .iter()
                .position(|o| o == c)
                .expect("choice is an option (checked by tests)"),
        },
    };
    let prompt = if options.is_empty() {
        prompt.to_string()
    } else {
        let letters = options
            .iter()
            .enumerate()
            .map(|(i, o)| format!("  {}) {o}", (b'a' + i as u8) as char))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{prompt}\n{letters}")
    };
    Question::new(prompt, answer)
}

impl Line {
    pub fn sans(&self) -> Vec<String> {
        parse_sequence(&self.moves)
    }
}

#[cfg(test)]
mod tests;
