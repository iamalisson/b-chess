//! One game against Lc0: rules, move input, the bot's reply, and how the game ends.

use crate::{
    engine::Engine,
    levels::{Rng, spec},
    store::SavedGame,
};
use ratatui::{crossterm::event::KeyCode, style::Color as Tc};
use shakmaty::{
    CastlingMode, Chess, Color, EnPassantMode, Move, Position, san::SanPlus, uci::UciMove,
    zobrist::Zobrist64,
};
use std::io;

pub fn parse_move(pos: &Chess, s: &str) -> Option<Move> {
    // Castling is written with the letter O, but 0-0 and o-o are common too.
    let castle = s.replace(['0', 'o'], "O");
    let s = if castle.starts_with("O-O") {
        castle.as_str()
    } else {
        s
    };
    if let Ok(san) = SanPlus::from_ascii(s.as_bytes())
        && let Ok(m) = san.san.to_move(pos)
    {
        return Some(m);
    }
    UciMove::from_ascii(s.as_bytes()).ok()?.to_move(pos).ok()
}

pub struct Game {
    pub engine: Engine,
    pub level: u32,
    pub ladder: bool,
    pub me: Color,
    pub pos: Chess,
    pub uci: Vec<String>,
    pub san: Vec<String>,
    pub seen: Vec<Zobrist64>,
    pub last: Option<Move>,
    pub input: String,
    pub msg: Option<(String, Tc)>,
    pub peek: bool,
    pub peeks: u32,
    pub confirm_abandon: bool,
    pub thinking: bool,
    pub rng: Rng,
    pub result: Option<(i32, String)>,
}

impl Game {
    pub fn new(engine: Engine, level: u32, me: Color, ladder: bool) -> Game {
        let pos = Chess::default();
        let seen = vec![pos.zobrist_hash(EnPassantMode::Legal)];
        Game {
            engine,
            level,
            ladder,
            me,
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
            confirm_abandon: false,
            rng: Rng::new(),
            result: None,
        }
    }

    pub fn resume(engine: Engine, saved: &SavedGame) -> io::Result<Game> {
        let mut game = Game::new(engine, saved.level, saved.me, saved.ladder);
        game.peeks = saved.peeks;
        for u in &saved.uci {
            let m = UciMove::from_ascii(u.as_bytes())
                .ok()
                .and_then(|m| m.to_move(&game.pos).ok())
                .ok_or_else(|| io::Error::other(format!("saved game has a bad move: {u}")))?;
            game.apply(m);
        }
        game.thinking = game.result.is_none() && game.pos.turn() != game.me;
        game.msg = Some(("Game resumed. Type help for commands.".into(), Tc::Green));
        Ok(game)
    }

    pub fn to_saved(&self) -> SavedGame {
        SavedGame {
            level: self.level,
            me: self.me,
            peeks: self.peeks,
            ladder: self.ladder,
            uci: self.uci.clone(),
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
            let best = self.engine.best_move(&self.uci, level.nodes)?;
            UciMove::from_ascii(best.as_bytes())
                .ok()
                .and_then(|u| u.to_move(&self.pos).ok())
                .ok_or_else(|| io::Error::other(format!("lc0 sent bad move: {best}")))?
        };
        self.apply(m);
        Ok(())
    }

    pub fn toggle_board(&mut self) {
        self.peek = !self.peek;
        self.peeks += self.peek as u32;
    }

    pub fn submit(&mut self) {
        let text = std::mem::take(&mut self.input);
        match text.trim() {
            "" => {}
            "resign" => self.result = Some((-1, "Resigned".into())),
            "board" | "b" => self.toggle_board(),
            s => match parse_move(&self.pos, s) {
                Some(m) => {
                    self.msg = None;
                    self.peek = false;
                    self.apply(m);
                    self.thinking = self.result.is_none();
                }
                None => {
                    self.msg = Some((
                        format!("Illegal or unknown move: {s}  (type help to see how to move)"),
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
            KeyCode::Char(c) if self.input.len() < 12 => self.input.push(c),
            _ => {}
        }
        self.confirm_abandon = abandon_pressed && !self.confirm_abandon;
    }

    /// "12... Nf6" for ply index `i`.
    pub fn numbered(&self, i: usize) -> String {
        let dots = if i.is_multiple_of(2) { "." } else { "..." };
        format!("{}{dots} {}", i / 2 + 1, self.san[i])
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
        assert!(
            parse_move(&pos, "0-0-0").is_none(),
            "long castling is not legal here"
        );
    }
}
