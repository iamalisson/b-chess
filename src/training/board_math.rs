//! Facts about the empty board: square colors, diagonals, walking around, what a piece
//! attacks, and knight distances. Everything here is computed, so answer keys never drift.

use shakmaty::{Bitboard, Color, File, Piece, Rank, Role, Square, attacks};
use std::collections::VecDeque;

/// The square `files` to the right (towards h) and `ranks` up (towards 8), if on the board.
pub fn walk(sq: Square, files: i32, ranks: i32) -> Option<Square> {
    let file = sq.file().offset(files)?;
    let rank = sq.rank().offset(ranks)?;
    Some(Square::from_coords(file, rank))
}

/// Board-reading order: by file, then rank (a1, a2, ..., b1, ...), the way the course lists.
pub fn sorted(mut squares: Vec<Square>) -> Vec<Square> {
    squares.sort_by_key(|sq| (sq.file(), sq.rank()));
    squares.dedup();
    squares
}

fn squares_of(bb: Bitboard) -> Vec<Square> {
    sorted(bb.into_iter().collect())
}

/// Squares from `a` to `b` inclusive if they share a diagonal, ordered from `a`.
pub fn diagonal_path(a: Square, b: Square) -> Option<Vec<Square>> {
    let df = u32::from(b.file()) as i32 - u32::from(a.file()) as i32;
    let dr = u32::from(b.rank()) as i32 - u32::from(a.rank()) as i32;
    if a == b || df.abs() != dr.abs() {
        return None;
    }
    let (sf, sr) = (df.signum(), dr.signum());
    Some(
        (0..=df.abs())
            .filter_map(|i| walk(a, sf * i, sr * i))
            .collect(),
    )
}

/// Both full diagonals through `sq`, each listed from its lower end.
pub fn diagonals_through(sq: Square) -> [Vec<Square>; 2] {
    let line = |sf: i32| -> Vec<Square> {
        let mut start = sq;
        while let Some(prev) = walk(start, -sf, -1) {
            start = prev;
        }
        (0..8).map_while(|i| walk(start, sf * i, i)).collect()
    };
    [line(1), line(-1)]
}

/// Squares a lone piece on `sq` attacks on an otherwise empty board.
pub fn attacks_on_empty(role: Role, sq: Square) -> Vec<Square> {
    let piece = Piece {
        color: Color::White,
        role,
    };
    squares_of(attacks::attacks(sq, piece, Bitboard::EMPTY))
}

/// Fewest knight moves from `a` to `b`.
pub fn knight_distance(a: Square, b: Square) -> u32 {
    let mut dist = [u32::MAX; 64];
    dist[usize::from(a)] = 0;
    let mut queue = VecDeque::from([a]);
    while let Some(sq) = queue.pop_front() {
        if sq == b {
            break;
        }
        for next in attacks::knight_attacks(sq) {
            if dist[usize::from(next)] == u32::MAX {
                dist[usize::from(next)] = dist[usize::from(sq)] + 1;
                queue.push_back(next);
            }
        }
    }
    dist[usize::from(b)]
}

/// One shortest knight route from `a` to `b`, both ends included.
pub fn knight_route(a: Square, b: Square) -> Vec<Square> {
    let mut route = vec![b];
    while let Some(&cur) = route.last().filter(|&&s| s != a) {
        let left = knight_distance(a, cur);
        let prev = attacks::knight_attacks(cur)
            .into_iter()
            .find(|&s| knight_distance(a, s) + 1 == left)
            .expect("a knight reaches every square");
        route.push(prev);
    }
    route.reverse();
    route
}

/// True if `path` goes from `from` to `to` by knight hops in the fewest possible moves.
pub fn is_shortest_knight_route(path: &[Square], from: Square, to: Square) -> bool {
    path.first() == Some(&from)
        && path.last() == Some(&to)
        && path.len() as u32 == knight_distance(from, to) + 1
        && path
            .windows(2)
            .all(|w| attacks::knight_attacks(w[0]).contains(w[1]))
}

/// Human wording for a walk, e.g. "1 file right and 2 ranks up", "3 squares diagonally up-right".
pub fn describe_walk(files: i32, ranks: i32) -> String {
    let plural = |n: i32, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let horiz = if files > 0 { "right" } else { "left" };
    let vert = if ranks > 0 { "up" } else { "down" };
    match (files, ranks) {
        (0, r) => format!("{} {vert}", plural(r.abs(), "rank")),
        (f, 0) => format!("{} {horiz}", plural(f.abs(), "file")),
        (f, r) if f.abs() == r.abs() => {
            format!("{} diagonally {vert}-{horiz}", plural(f.abs(), "square"))
        }
        (f, r) => format!(
            "{} {horiz} and {} {vert}",
            plural(f.abs(), "file"),
            plural(r.abs(), "rank")
        ),
    }
}

/// All 64 squares, for random picks.
pub fn all_squares() -> impl Iterator<Item = Square> {
    File::ALL.into_iter().flat_map(|f| {
        Rank::ALL
            .into_iter()
            .map(move |r| Square::from_coords(f, r))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::Square::*;

    #[test]
    fn colors_match_the_course_key() {
        let light = [E4, F5, G2, A8, F3, C2, B1, D3, E6, G4, B7];
        let dark = [D4, C7, B6, H8, E1, D8, A5, H4, G7, C5, F8, H2, A3];
        assert!(light.iter().all(|s| s.is_light()));
        assert!(dark.iter().all(|s| !s.is_light()));
    }

    #[test]
    fn diagonal_paths() {
        assert_eq!(diagonal_path(B3, F7), Some(vec![B3, C4, D5, E6, F7]));
        assert_eq!(diagonal_path(G5, D8), Some(vec![G5, F6, E7, D8]));
        assert_eq!(diagonal_path(B2, G6), None);
        let [up, down] = diagonals_through(D4);
        assert_eq!(up, vec![A1, B2, C3, D4, E5, F6, G7, H8]);
        assert_eq!(down, vec![G1, F2, E3, D4, C5, B6, A7]);
    }

    #[test]
    fn walking() {
        assert_eq!(walk(E4, 1, 2), Some(F6));
        assert_eq!(walk(H8, 0, -7).and_then(|s| walk(s, -3, 0)), Some(E1));
        assert_eq!(walk(H8, 1, 0), None);
        assert_eq!(describe_walk(3, 3), "3 squares diagonally up-right");
        assert_eq!(describe_walk(1, 2), "1 file right and 2 ranks up");
    }

    #[test]
    fn attack_counts_match_the_course_key() {
        assert_eq!(
            attacks_on_empty(Role::Knight, E4),
            vec![C3, C5, D2, D6, F2, F6, G3, G5]
        );
        assert_eq!(attacks_on_empty(Role::Knight, A1).len(), 2);
        assert_eq!(attacks_on_empty(Role::Knight, C2).len(), 6);
        assert_eq!(attacks_on_empty(Role::Bishop, D4).len(), 13);
        assert_eq!(attacks_on_empty(Role::Bishop, B7).len(), 9);
        assert_eq!(attacks_on_empty(Role::Rook, D4).len(), 14);
        assert_eq!(attacks_on_empty(Role::Queen, D4).len(), 27);
        assert_eq!(attacks_on_empty(Role::Queen, A1).len(), 21);
    }

    #[test]
    fn knight_distances_match_the_course_key() {
        let cases = [
            (G1, E5, 2),
            (B1, D4, 3),
            (E4, E5, 3),
            (C3, C4, 3),
            (A1, B2, 4),
            (D4, H8, 4),
            (B1, H8, 5),
            (A1, H8, 6),
            (H1, A8, 6),
        ];
        for (a, b, n) in cases {
            assert_eq!(knight_distance(a, b), n, "{a} -> {b}");
        }
        assert!(is_shortest_knight_route(&[G1, F3, E5], G1, E5));
        assert!(is_shortest_knight_route(&knight_route(A1, H8), A1, H8));
        assert!(!is_shortest_knight_route(&[G1, E2, D4, F3, E5], G1, E5));
        assert!(!is_shortest_knight_route(&[G1, G3, E5], G1, E5));
    }
}
