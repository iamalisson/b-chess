//! One game against Lc0: rules, move input, the bot's reply, and how the game ends.

use crate::{
    engine::Engine,
    levels::{Rng, spec},
    store::{Mode, SavedGame},
    training::{
        drills,
        question::{Question, Verdict},
    },
};
use ratatui::{crossterm::event::KeyCode, style::Color as Tc};
use shakmaty::{
    CastlingMode, Chess, Color, EnPassantMode, Move, Position, fen::Fen, san::SanPlus,
    uci::UciMove, zobrist::Zobrist64,
};
use std::{collections::VecDeque, io};

/// Accepts the common ways people type moves: castling with zeros or lowercase (0-0, o-o)
/// and lowercase piece letters (nf3, ra8). Not b: "bxc3" is a pawn capture.
fn normalize(s: &str) -> String {
    let s = s.trim();
    let castle = s.replace(['0', 'o'], "O");
    if castle.starts_with("O-O") {
        return castle;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c @ ('k' | 'q' | 'r' | 'n')) if s.len() >= 3 => {
            format!("{}{}", c.to_ascii_uppercase(), chars.as_str())
        }
        _ => s.to_string(),
    }
}

pub fn parse_move(pos: &Chess, s: &str) -> Option<Move> {
    let s = normalize(s);
    if let Ok(san) = SanPlus::from_ascii(s.as_bytes())
        && let Ok(m) = san.san.to_move(pos)
    {
        return Some(m);
    }
    UciMove::from_ascii(s.as_bytes()).ok()?.to_move(pos).ok()
}

/// Well-formed SAN or UCI, so an unplayable one is an illegal move rather than a typo.
pub fn looks_like_a_move(s: &str) -> bool {
    let s = normalize(s);
    SanPlus::from_ascii(s.as_bytes()).is_ok() || UciMove::from_ascii(s.as_bytes()).is_ok()
}

/// Training aids a game can switch on.
#[derive(Clone, Copy, Default)]
pub struct Aids {
    /// Show an empty board with coordinates (pieces invisible).
    pub ghost: bool,
    /// Every 5 of your moves: where the kings are and what is loose.
    pub check_habit: bool,
    /// Now and then: "where is your knight?".
    pub piece_questions: bool,
}

pub struct Game {
    pub engine: Engine,
    pub level: u32,
    pub mode: Mode,
    pub me: Color,
    /// Start position as FEN; `None` is the standard start.
    pub start: Option<String>,
    pub pos: Chess,
    pub uci: Vec<String>,
    pub san: Vec<String>,
    pub seen: Vec<Zobrist64>,
    pub last: Option<Move>,
    pub input: String,
    pub msg: Option<(String, Tc)>,
    pub peek: bool,
    pub peeks: u32,
    /// Well-formed moves that were illegal in the position (typos are not counted).
    pub illegal: u32,
    pub aids: Aids,
    /// Questions waiting to be answered, each flagged whether it counts for the score
    /// (recall questions you asked for don't).
    pub quiz: VecDeque<(Question, bool)>,
    pub quiz_right: u32,
    pub quiz_total: u32,
    pub confirm_abandon: bool,
    pub thinking: bool,
    pub rng: Rng,
    pub result: Option<(i32, String)>,
}

fn start_position(fen: Option<&str>) -> io::Result<Chess> {
    let Some(fen) = fen else {
        return Ok(Chess::default());
    };
    Fen::from_ascii(fen.as_bytes())
        .ok()
        .and_then(|f| f.into_position(CastlingMode::Standard).ok())
        .ok_or_else(|| io::Error::other(format!("bad start position: {fen}")))
}

impl Game {
    pub fn new(
        engine: Engine,
        level: u32,
        me: Color,
        mode: Mode,
        start: Option<String>,
        aids: Aids,
    ) -> io::Result<Game> {
        let pos = start_position(start.as_deref())?;
        let seen = vec![pos.zobrist_hash(EnPassantMode::Legal)];
        Ok(Game {
            engine,
            level,
            mode,
            me,
            start,
            thinking: pos.turn() != me,
            pos,
            uci: Vec::new(),
            san: Vec::new(),
            seen,
            last: None,
            input: String::new(),
            msg: None,
            peek: false,
            peeks: 0,
            illegal: 0,
            aids,
            quiz: VecDeque::new(),
            quiz_right: 0,
            quiz_total: 0,
            confirm_abandon: false,
            rng: Rng::new(),
            result: None,
        })
    }

    pub fn resume(engine: Engine, saved: &SavedGame, aids: Aids) -> io::Result<Game> {
        let mut game = Game::new(
            engine,
            saved.level,
            saved.me,
            saved.mode.clone(),
            saved.fen.clone(),
            aids,
        )?;
        game.peeks = saved.peeks;
        game.illegal = saved.illegal;
        game.quiz_right = saved.quiz_right;
        game.quiz_total = saved.quiz_total;
        for u in &saved.uci {
            let m = UciMove::from_ascii(u.as_bytes())
                .ok()
                .and_then(|m| m.to_move(&game.pos).ok())
                .ok_or_else(|| io::Error::other(format!("saved game has a bad move: {u}")))?;
            game.apply(m);
        }
        game.thinking = game.result.is_none() && game.pos.turn() != game.me;
        // Questions left unanswered when the game was saved are asked again.
        if saved.pending > 0 && !game.thinking {
            for q in drills::checking_habit(&game.pos, game.me) {
                game.quiz.push_back((q, true));
            }
        }
        game.msg = Some(("Game resumed. Type help for commands.".into(), Tc::Green));
        Ok(game)
    }

    pub fn to_saved(&self) -> SavedGame {
        SavedGame {
            level: self.level,
            me: self.me,
            peeks: self.peeks,
            mode: self.mode.clone(),
            uci: self.uci.clone(),
            fen: self.start.clone(),
            illegal: self.illegal,
            quiz_right: self.quiz_right,
            quiz_total: self.quiz_total,
            pending: self.quiz.iter().filter(|(_, counted)| *counted).count() as u32,
        }
    }

    pub fn apply(&mut self, m: Move) {
        self.uci.push(m.to_uci(CastlingMode::Standard).to_string());
        self.san
            .push(SanPlus::from_move_and_play_unchecked(&mut self.pos, m).to_string());
        self.seen.push(self.pos.zobrist_hash(EnPassantMode::Legal));
        self.last = Some(m);
        self.check_end();
    }

    pub fn check_end(&mut self) {
        let draw = |why: &str| Some((0, why.to_string()));
        self.result = if let Some(outcome) = self.pos.outcome().known() {
            match outcome.winner() {
                Some(c) => Some((if c == self.me { 1 } else { -1 }, "Checkmate".into())),
                None if self.pos.is_stalemate() => draw("Stalemate"),
                None => draw("Insufficient material"),
            }
        } else if self.pos.halfmoves() >= 100 {
            draw("50-move rule")
        } else if self
            .seen
            .iter()
            .filter(|&&h| Some(&h) == self.seen.last())
            .count()
            >= 3
        {
            draw("Threefold repetition")
        } else {
            None
        };
    }

    /// The game ended on the board (mate or a draw), not by resigning or abandoning.
    pub fn ended_naturally(&self) -> bool {
        matches!(&self.result, Some((_, why)) if why != "Resigned" && why != "Abandoned")
    }

    /// Your moves so far.
    pub fn my_moves(&self) -> usize {
        let first_is_mine = self.start_turn() == self.me;
        (self.san.len() + usize::from(first_is_mine)) / 2
    }

    fn start_turn(&self) -> Color {
        let me_to_move = self.pos.turn() == self.me;
        // The side to move flips every ply.
        if self.san.len().is_multiple_of(2) == me_to_move {
            self.me
        } else {
            !self.me
        }
    }

    pub fn engine_move(&mut self) -> io::Result<()> {
        self.thinking = false;
        if self.result.is_some() || self.pos.turn() == self.me {
            return Ok(());
        }
        let level = spec(self.level);
        let m = if self.rng.below(100) < level.random_pct {
            let legal = self.pos.legal_moves();
            legal[self.rng.below(legal.len() as u64) as usize]
        } else {
            let best = self
                .engine
                .best_move(self.start.as_deref(), &self.uci, level.nodes)?;
            UciMove::from_ascii(best.as_bytes())
                .ok()
                .and_then(|u| u.to_move(&self.pos).ok())
                .ok_or_else(|| io::Error::other(format!("lc0 sent bad move: {best}")))?
        };
        self.apply(m);
        self.ask_questions();
        Ok(())
    }

    /// After Lc0's reply: the checking habit every 5 of your moves, and sometimes a
    /// "where is your piece?" question.
    fn ask_questions(&mut self) {
        if self.result.is_some() {
            return;
        }
        let n = self.my_moves();
        if self.aids.check_habit && n > 0 && n.is_multiple_of(5) {
            for q in drills::checking_habit(&self.pos, self.me) {
                self.quiz.push_back((q, true));
            }
        }
        if self.aids.piece_questions && n >= 3 && self.rng.below(4) == 0 {
            let q = drills::piece_question(&self.pos, self.me, &mut self.rng);
            self.quiz.push_back((q, true));
        }
    }

    /// The question waiting for an answer, if any.
    pub fn asking(&self) -> Option<&Question> {
        self.quiz.front().map(|(q, _)| q)
    }

    pub fn toggle_board(&mut self) {
        self.peek = !self.peek;
        self.peeks += self.peek as u32;
    }

    fn answer(&mut self, text: &str) {
        let Some((q, counted)) = self.quiz.front() else {
            return;
        };
        let counted = *counted;
        let (line, color) = match q.grade(text) {
            Verdict::Unreadable(hint) => {
                self.msg = Some((hint, Tc::Yellow));
                return;
            }
            Verdict::Right | Verdict::Next(..) => {
                self.quiz_right += u32::from(counted);
                ("Right.".to_string(), Tc::Green)
            }
            // A wrong recall shows the position, so it counts as a peek.
            Verdict::Wrong(expected) if !counted => {
                self.peeks += 1;
                (
                    format!("Not quite: {expected}  (counted as a peek)"),
                    Tc::Red,
                )
            }
            Verdict::Wrong(expected) => (format!("Not quite: {expected}"), Tc::Red),
        };
        self.quiz_total += u32::from(counted);
        self.quiz.pop_front();
        self.msg = Some((line, color));
    }

    pub fn submit(&mut self) {
        let text = std::mem::take(&mut self.input);
        match text.trim() {
            "" => {}
            "resign" => self.result = Some((-1, "Resigned".into())),
            "board" | "b" => self.toggle_board(),
            "recall" => {
                for q in drills::recall(&self.pos, self.me).into_iter().rev() {
                    self.quiz.push_front((q, false));
                }
            }
            s if self.asking().is_some() => self.answer(s),
            s => match parse_move(&self.pos, s) {
                Some(m) => {
                    self.msg = None;
                    self.peek = false;
                    self.apply(m);
                    self.thinking = self.result.is_none();
                }
                None if looks_like_a_move(s) => {
                    self.illegal += 1;
                    self.msg = Some((format!("Illegal move: {s}"), Tc::Red));
                }
                None => {
                    self.msg = Some((
                        format!("Unknown move or command: {s}  (type help)"),
                        Tc::Red,
                    ))
                }
            },
        }
    }

    pub fn key(&mut self, code: KeyCode) {
        let abandon_pressed = code == KeyCode::Esc;
        match code {
            KeyCode::Esc if self.confirm_abandon => self.result = Some((-1, "Abandoned".into())),
            KeyCode::Esc => {
                self.msg = Some((
                    "Press Esc again to abandon (counts as a loss)".into(),
                    Tc::Yellow,
                ))
            }
            KeyCode::Tab => self.toggle_board(),
            KeyCode::Enter => self.submit(),
            KeyCode::Backspace => {
                self.input.pop();
            }
            // Long enough for a full recall answer (16 pieces).
            KeyCode::Char(c) if self.input.len() < 120 => self.input.push(c),
            _ => {}
        }
        self.confirm_abandon = abandon_pressed && !self.confirm_abandon;
    }

    /// "12... Nf6" for ply index `i`.
    pub fn numbered(&self, i: usize) -> String {
        let first_black = self.start_turn() == Color::Black;
        let ply = i + usize::from(first_black);
        let dots = if ply.is_multiple_of(2) { "." } else { "..." };
        format!("{}{dots} {}", ply / 2 + 1, self.san[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn castling_accepts_zeros_and_lowercase() {
        // 1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5: white can castle short.
        let mut pos = Chess::default();
        for m in ["e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5"] {
            let mv = parse_move(&pos, m).unwrap();
            pos.play_unchecked(mv);
        }
        for s in ["O-O", "0-0", "o-o", "0-0+"] {
            assert!(parse_move(&pos, s).is_some_and(|m| m.is_castle()), "{s}");
        }
        assert!(parse_move(&pos, "nc3").is_some_and(|m| m.role() == shakmaty::Role::Knight));
        // An illegal lowercase move is still a move (counted as illegal), not a typo.
        assert!(parse_move(&pos, "nf6").is_none() && looks_like_a_move("nf6"));
        assert!(!looks_like_a_move("hello"));
        assert!(
            parse_move(&pos, "0-0-0").is_none(),
            "long castling is not legal here"
        );
    }
}
