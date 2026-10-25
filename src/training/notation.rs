//! Reading and writing the course's notation: squares, square lists, piece lists,
//! position descriptions ("White: Kg6, Ra1. Black: Kg8.") and move sequences
//! ("1.e4 e5 2.Nf3 Nc6 3.Bc4").

use crate::game::parse_move;
use crate::training::board_math::sorted;
use shakmaty::{
    Board, CastlingMode, Chess, Color, FromSetup, Move, Piece, Role, Setup, Square, san::SanPlus,
};

pub fn parse_square(s: &str) -> Option<Square> {
    Square::from_ascii(s.trim().to_ascii_lowercase().as_bytes()).ok()
}

/// Squares separated by spaces, commas or dashes; "none" (or "-") is the empty list.
/// Returns `None` if any token is not a square.
pub fn parse_squares(input: &str) -> Option<Vec<Square>> {
    let input = input.trim().to_ascii_lowercase();
    if matches!(input.as_str(), "none" | "-" | "no" | "nothing") {
        return Some(Vec::new());
    }
    let tokens = input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty());
    tokens.map(parse_square).collect()
}

pub fn format_squares(squares: &[Square]) -> String {
    if squares.is_empty() {
        return "none".into();
    }
    squares
        .iter()
        .map(Square::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// One side's pieces, e.g. "Kg6 Ra1 f2 g2" (pawns bare or with P). Order doesn't matter.
pub fn parse_pieces(input: &str) -> Option<Vec<(Role, Square)>> {
    let mut out: Vec<(Role, Square)> = input
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(|t| {
            let first = t.chars().next()?;
            match Role::from_char(first.to_ascii_lowercase()) {
                Some(role) if first.is_ascii_uppercase() => Some((role, parse_square(&t[1..])?)),
                _ => Some((Role::Pawn, parse_square(t)?)),
            }
        })
        .collect::<Option<_>>()?;
    out.sort_by_key(|&(role, sq)| (std::cmp::Reverse(role), sq.file(), sq.rank()));
    Some(out)
}

/// One side's pieces in the course's order: king, queen, rooks, bishops, knights, pawns.
pub fn pieces_of(board: &Board, color: Color) -> Vec<(Role, Square)> {
    let mut out: Vec<(Role, Square)> = board
        .iter()
        .filter(|(_, p)| p.color == color)
        .map(|(sq, p)| (p.role, sq))
        .collect();
    out.sort_by_key(|&(role, sq)| (std::cmp::Reverse(role), sq.file(), sq.rank()));
    out
}

pub fn format_pieces(pieces: &[(Role, Square)]) -> String {
    let piece = |&(role, sq): &(Role, Square)| match role {
        Role::Pawn => sq.to_string(),
        _ => format!("{}{sq}", role.upper_char()),
    };
    pieces.iter().map(piece).collect::<Vec<_>>().join(", ")
}

/// "White: Kg6, Ra1, pawns f2, g2. Black: Kg8." The pawn list may also be bare squares.
pub fn parse_board(desc: &str) -> Result<Board, String> {
    let mut board = Board::empty();
    for sentence in desc.split('.').map(str::trim).filter(|s| !s.is_empty()) {
        let (side, items) = sentence
            .split_once(':')
            .ok_or_else(|| format!("expected 'White:' or 'Black:' in {sentence:?}"))?;
        let color = match side.trim() {
            "White" => Color::White,
            "Black" => Color::Black,
            other => return Err(format!("unknown side {other:?}")),
        };
        for item in items.split(',').map(str::trim) {
            let item = item
                .trim_start_matches("pawns ")
                .trim_start_matches("pawn ");
            let (role, sq) = parse_pieces(item)
                .and_then(|p| p.first().copied())
                .ok_or_else(|| format!("bad piece {item:?}"))?;
            board.set_piece_at(sq, Piece { color, role });
        }
    }
    Ok(board)
}

/// Inverse of [`parse_board`].
#[cfg(test)]
pub fn describe_board(board: &Board) -> String {
    let side = |color: Color, name: &str| {
        let pieces = pieces_of(board, color);
        let (pawns, rest): (Vec<_>, Vec<_>) = pieces.iter().partition(|(r, _)| *r == Role::Pawn);
        let mut parts = vec![];
        if !rest.is_empty() {
            parts.push(format_pieces(&rest));
        }
        if !pawns.is_empty() {
            let word = if pawns.len() == 1 { "pawn" } else { "pawns" };
            parts.push(format!("{word} {}", format_pieces(&pawns)));
        }
        format!("{name}: {}.", parts.join(", "))
    };
    format!(
        "{} {}",
        side(Color::White, "White"),
        side(Color::Black, "Black")
    )
}

/// A playable position from a board (no castling, no en passant).
pub fn to_position(board: &Board, turn: Color) -> Result<Chess, String> {
    let mut setup = Setup::empty();
    setup.board = board.clone();
    setup.turn = turn;
    Chess::from_setup(setup, CastlingMode::Standard).map_err(|e| e.to_string())
}

/// Moves of "1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.Qh5?? Nf6 10…Kc7", without numbers and !/? marks.
pub fn parse_sequence(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|t| {
            let t = t.rsplit(['.', '…']).next().unwrap_or(t);
            t.trim_end_matches(['!', '?']).to_string()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// One played move and the position after it.
pub struct Ply {
    pub san: String,
    pub mv: Move,
    pub after: Chess,
}

/// Plays `moves` from `start`, failing on the first illegal one.
pub fn play_sequence(start: &Chess, moves: &[String]) -> Result<Vec<Ply>, String> {
    let mut pos = start.clone();
    let mut plies = Vec::with_capacity(moves.len());
    for (i, text) in moves.iter().enumerate() {
        let mv =
            parse_move(&pos, text).ok_or_else(|| format!("move {} ({text}) is illegal", i + 1))?;
        let san = SanPlus::from_move_and_play_unchecked(&mut pos, mv).to_string();
        plies.push(Ply {
            san,
            mv,
            after: pos.clone(),
        });
    }
    Ok(plies)
}

/// Ply count after a marker like "4...g6" or "5.Nxe5" (checked against `moves`).
pub fn locate(moves: &[String], marker: &str) -> Option<usize> {
    let (num, san) = marker.split_once(['.', '…'])?;
    let black = san.starts_with(['.', '…']);
    let san = san.trim_start_matches(['.', '…']);
    let ply = (num.trim().parse::<usize>().ok()? - 1) * 2 + usize::from(black);
    (moves.get(ply)?.trim_end_matches(['+', '#']) == san.trim_end_matches(['+', '#']))
        .then_some(ply + 1)
}

pub fn same_squares(a: &[Square], b: &[Square]) -> bool {
    sorted(a.to_vec()) == sorted(b.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::{Position, Square::*};

    #[test]
    fn squares_parse_in_any_format() {
        assert_eq!(parse_squares("c3, c5 d2"), Some(vec![C3, C5, D2]));
        assert_eq!(parse_squares("b3 – c4 – d5"), Some(vec![B3, C4, D5]));
        assert_eq!(parse_squares("none"), Some(vec![]));
        assert_eq!(parse_squares("c3 zz"), None);
        assert_eq!(format_squares(&[C3, C5]), "c3, c5");
    }

    #[test]
    fn board_descriptions_round_trip() {
        let desc = "White: Kg1, Qc4, Bb3, Ng5, pawns f2, g2, h2. Black: Kh8, Rf8, pawns g7, h7.";
        let board = parse_board(desc).unwrap();
        assert_eq!(describe_board(&board), desc);
        assert!(to_position(&board, Color::White).is_ok());
        assert_eq!(
            parse_pieces("Kh8 h7 Rf8 g7"),
            Some(pieces_of(&board, Color::Black))
        );
    }

    #[test]
    fn sequences_parse_and_play() {
        let moves = parse_sequence("1.e4 e5 2.Bc4 Nc6 3.Qh5 Nf6?? 4.Qxf7#");
        assert_eq!(moves, ["e4", "e5", "Bc4", "Nc6", "Qh5", "Nf6", "Qxf7#"]);
        let plies = play_sequence(&Chess::default(), &moves).unwrap();
        assert!(plies.last().unwrap().after.is_checkmate());
        assert_eq!(locate(&moves, "3.Qh5"), Some(5));
        assert_eq!(locate(&moves, "3...Nf6"), Some(6));
        assert_eq!(locate(&moves, "3...Nc6"), None);
        assert_eq!(parse_sequence("10…Kc7 11.Bd8#"), ["Kc7", "Bd8#"]);
    }
}
