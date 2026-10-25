//! The gnuchess-style letter board: pieces as letters, dots for empty squares. Also draws
//! an empty board (invisible pieces) and the coordinate trainer's highlighted square.

use crate::theme::ACCENT;
use ratatui::{
    style::{Color as Tc, Stylize},
    text::{Line, Span},
};
use shakmaty::{Board, Chess, Color, EnPassantMode, File, Move, Position, Rank, Square, fen::Fen};

pub struct BoardOpts {
    /// White at the bottom.
    pub white_side: bool,
    /// Squares of this move are highlighted.
    pub last: Option<Move>,
    /// A blank line between ranks, for a bigger board.
    pub spaced: bool,
    /// Draw the pieces (false: an empty board with coordinates).
    pub pieces: bool,
    /// One square to highlight (coordinate trainer).
    pub highlight: Option<Square>,
}

/// "black  KQkq  c3", like gnuchess: side to move, castling rights, en passant square.
pub fn status_line(pos: &Chess) -> Line<'static> {
    let fen = Fen::from_position(pos, EnPassantMode::Legal).to_string();
    let fields: Vec<&str> = fen.split(' ').collect();
    let turn = if pos.turn() == Color::White {
        "white"
    } else {
        "black"
    };
    Line::from(format!("{turn}  {}  {}", fields[2], fields[3])).fg(ACCENT)
}

pub fn board_lines(board: &Board, opts: &BoardOpts) -> Vec<Line<'static>> {
    let mut ranks = Rank::ALL.to_vec();
    let mut files = File::ALL.to_vec();
    if opts.white_side {
        ranks.reverse();
    } else {
        files.reverse();
    }
    let marked = |sq: Square| {
        opts.last
            .is_some_and(|m| m.to() == sq || m.from() == Some(sq))
    };
    let mut lines = vec![];
    for (i, &r) in ranks.iter().enumerate() {
        if opts.spaced && i > 0 {
            lines.push(Line::from(""));
        }
        let mut spans = vec![Span::raw(format!("{}  ", r.char())).dim()];
        for &f in &files {
            let sq = Square::from_coords(f, r);
            let span = match board.piece_at(sq).filter(|_| opts.pieces) {
                _ if opts.highlight == Some(sq) => Span::raw("  ■ ").fg(Tc::Yellow).bold(),
                Some(p) => Span::raw(format!("  {} ", p.char())).bold(),
                None => Span::raw("  . ").dim(),
            };
            spans.push(if marked(sq) {
                span.fg(Tc::Yellow).bold()
            } else {
                span
            });
        }
        spans.push(Span::raw("  "));
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    let labels: String = files.iter().map(|f| format!("  {} ", f.char())).collect();
    lines.push(Line::from(format!("   {labels}  ")).dim());
    lines
}
