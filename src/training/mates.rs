//! Brute-force mate checks for the small puzzle positions of Phase 3.

use shakmaty::{Chess, Move, Position};

fn after(pos: &Chess, m: Move) -> Chess {
    let mut next = pos.clone();
    next.play_unchecked(m);
    next
}

/// Every move that mates at once.
pub fn mating_moves(pos: &Chess) -> Vec<Move> {
    pos.legal_moves()
        .into_iter()
        .filter(|&m| after(pos, m).is_checkmate())
        .collect()
}

/// True if `first` mates, or mates next move whatever the reply.
pub fn forces_mate_in_2(pos: &Chess, first: Move) -> bool {
    let next = after(pos, first);
    if next.is_checkmate() {
        return true;
    }
    let replies = next.legal_moves();
    !replies.is_empty()
        && replies
            .into_iter()
            .all(|r| !mating_moves(&after(&next, r)).is_empty())
}

/// All first moves that force mate in two.
pub fn mate_in_2_keys(pos: &Chess) -> Vec<Move> {
    pos.legal_moves()
        .into_iter()
        .filter(|&m| forces_mate_in_2(pos, m))
        .collect()
}

/// The defense that leaves the fewest mates, so the second move still needs finding.
pub fn toughest_reply(pos: &Chess) -> Option<Move> {
    pos.legal_moves()
        .into_iter()
        .min_by_key(|&r| mating_moves(&after(pos, r)).len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training::notation::{parse_board, to_position};
    use shakmaty::Color;

    fn pos(desc: &str) -> Chess {
        to_position(&parse_board(desc).unwrap(), Color::White).unwrap()
    }

    #[test]
    fn finds_mates() {
        let p = pos("White: Kg6, Ra1. Black: Kg8.");
        assert_eq!(mating_moves(&p).len(), 1);
        let p = pos("White: Kf6, Ra2. Black: Kg8.");
        assert!(mating_moves(&p).is_empty());
        assert!(!mate_in_2_keys(&p).is_empty());
    }
}
