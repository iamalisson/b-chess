//! Content validation: the course must parse, every reference must resolve, every move
//! must be legal, every mate must be a mate, and every `verify` rule must hold.

use super::*;
use crate::training::{
    mates::{mate_in_2_keys, mating_moves},
    notation::{locate, play_sequence, same_squares},
};
use shakmaty::{Bitboard, Board, Chess, Piece, Position, attacks};

fn squares_of(bb: Bitboard) -> Vec<Square> {
    bb.into_iter().collect()
}

fn piece(letter: &str) -> Piece {
    Piece::from_char(letter.chars().next().unwrap()).unwrap()
}

fn side(c: &str) -> Color {
    if c == "w" { Color::White } else { Color::Black }
}

/// Compares a computed set with a squares (or count) answer.
fn same(expected: Vec<Square>, answer: &AnswerSpec) -> Result<(), String> {
    let ok = match answer {
        AnswerSpec::Square(s) => same_squares(&expected, &[square(s)]),
        AnswerSpec::Squares(s) => same_squares(&expected, &parse_squares(s).unwrap()),
        AnswerSpec::Count(n) => expected.len() as u32 == *n,
        _ => return Err("rule needs a squares or count answer".into()),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("board says {expected:?}"))
    }
}

fn yes(expected: bool, answer: &AnswerSpec) -> Result<(), String> {
    match answer {
        AnswerSpec::YesNo(b) if *b == expected => Ok(()),
        AnswerSpec::YesNo(_) => Err(format!("board says {expected}")),
        _ => Err("rule needs a yes/no answer".into()),
    }
}

fn check(
    rule: &str,
    board: &Board,
    pos: Option<&Chess>,
    answer: &AnswerSpec,
) -> Result<(), String> {
    let parts: Vec<&str> = rule.split(':').collect();
    let occupied = board.occupied();
    match parts[..] {
        ["holds", p] => match answer {
            AnswerSpec::Square(s) if board.piece_at(square(s)) == Some(piece(p)) => Ok(()),
            _ => Err(format!("{p} is not on the answer square")),
        },
        ["holds_choice", sq] => match (answer, board.piece_at(square(sq))) {
            (AnswerSpec::Choice(c), Some(p)) if c == role_name(p.role) => Ok(()),
            _ => Err(format!("wrong piece on {sq}")),
        },
        ["pieces", p] => same(squares_of(board.by_piece(piece(p))), answer),
        ["check", c] => {
            let king = board.king_of(side(c)).ok_or("no king")?;
            yes(board.attacks_to(king, !side(c), occupied).any(), answer)
        }
        ["checkers"] => same(
            squares_of(pos.ok_or("needs a position")?.checkers()),
            answer,
        ),
        ["flight"] => {
            let pos = pos.ok_or("needs a position")?;
            let kings = pos
                .legal_moves()
                .into_iter()
                .filter(|m| m.role() == Role::King);
            same(kings.map(|m| m.to()).collect(), answer)
        }
        ["attackers", sq, c] => same(
            squares_of(board.attacks_to(square(sq), side(c), occupied)),
            answer,
        ),
        ["attacked_by", sq] => {
            let from = square(sq);
            let color = board.color_at(from).ok_or("empty square")?;
            same(
                squares_of(board.attacks_from(from) & board.by_color(!color)),
                answer,
            )
        }
        ["attacks", from, to] => yes(
            board.attacks_from(square(from)).contains(square(to)),
            answer,
        ),
        ["between", a, b] => same(
            squares_of(attacks::between(square(a), square(b)) & !occupied),
            answer,
        ),
        ["between", a, b, "occupied"] => same(
            squares_of(attacks::between(square(a), square(b)) & occupied),
            answer,
        ),
        _ => Err(format!("unknown rule {rule:?}")),
    }
}

#[test]
fn references_resolve() {
    let c = curriculum();
    let mut ids = std::collections::HashSet::new();
    for phase in &c.phases {
        for e in &phase.exercises {
            assert!(ids.insert(e.id.as_str()), "duplicate exercise id {}", e.id);
        }
    }
    for phase in &c.phases {
        for (id, _) in &phase.routine {
            assert!(
                c.exercise(id).is_some(),
                "routine of phase {} names unknown {id}",
                phase.number
            );
        }
        for e in &phase.exercises {
            match &e.kind {
                Kind::Quiz {
                    from: Some(src), ..
                } => assert!(c.exercise(src).is_some(), "{}: from {src}", e.id),
                Kind::Line { line, mode } => {
                    let l = c
                        .line(line)
                        .unwrap_or_else(|| panic!("{}: unknown line {line}", e.id));
                    if *mode == LineMode::Stops {
                        assert!(!l.stops.is_empty(), "{}: line {line} has no stops", e.id);
                    }
                }
                _ => {}
            }
        }
    }
    let numbers: Vec<u8> = c.phases.iter().map(|p| p.number).collect();
    assert_eq!(numbers, (0..=7).collect::<Vec<_>>());
}

#[test]
fn quizzes_build_and_pass_rules_fit() {
    let c = curriculum();
    for (_, e) in c
        .phases
        .iter()
        .flat_map(|p| p.exercises.iter().map(move |e| (p, e)))
    {
        if let Kind::Quiz { pass, .. } = &e.kind {
            let graded = c.quiz_questions(e).iter().filter(|q| q.graded()).count() as u32;
            assert!(graded > 0, "{} has no questions", e.id);
            assert!(
                pass.min_correct.unwrap_or(0) <= graded,
                "{}: min_correct too high",
                e.id
            );
            assert!(
                pass.streak.unwrap_or(0) <= graded,
                "{}: streak too long",
                e.id
            );
        }
    }
}

#[test]
fn mates_are_mates() {
    let c = curriculum();
    for e in c.phases.iter().flat_map(|p| &p.exercises) {
        let Kind::Quiz { questions, .. } = &e.kind else {
            continue;
        };
        for q in questions {
            match q {
                QSpec::Mate1 { position, .. } => {
                    assert!(
                        !mating_moves(&playable(position)).is_empty(),
                        "no mate in one: {position}"
                    )
                }
                QSpec::Mate2 { position, .. } => {
                    let pos = playable(position);
                    assert!(
                        mating_moves(&pos).is_empty(),
                        "already mate in one: {position}"
                    );
                    assert!(
                        !mate_in_2_keys(&pos).is_empty(),
                        "no mate in two: {position}"
                    );
                }
                _ => {}
            }
        }
    }
}

#[test]
fn asks_hold_on_their_positions() {
    let c = curriculum();
    for e in c.phases.iter().flat_map(|p| &p.exercises) {
        let Kind::Quiz { questions, .. } = &e.kind else {
            continue;
        };
        for q in questions {
            let QSpec::Ask(ask) = q else { continue };
            let (board, pos) = match (&ask.position, &ask.moves) {
                (Some(desc), _) => (parse_board(desc).unwrap(), None),
                (None, Some(moves)) => {
                    let plies = play_sequence(&Chess::default(), &parse_sequence(moves)).unwrap();
                    let last = plies.last().unwrap().after.clone();
                    (last.board().clone(), Some(last))
                }
                (None, None) => (Board::empty(), None),
            };
            if let Some(rule) = &ask.verify {
                check(rule, &board, pos.as_ref(), &ask.answer)
                    .unwrap_or_else(|err| panic!("{}: {:?}: {err}", e.id, ask.prompt));
            }
        }
    }
}

#[test]
fn lines_are_legal_and_stops_hold() {
    for line in &curriculum().lines {
        let sans = line.sans();
        let plies = play_sequence(&Chess::default(), &sans)
            .unwrap_or_else(|err| panic!("{}: {err}", line.id));
        for (san, ply) in sans.iter().zip(&plies) {
            assert_eq!(
                san.ends_with('#'),
                ply.after.is_checkmate(),
                "{}: {san}",
                line.id
            );
        }
        for stop in &line.stops {
            let n = locate(&sans, &stop.after)
                .unwrap_or_else(|| panic!("{}: stop after {} not found", line.id, stop.after));
            let pos = &plies[n - 1].after;
            answer_question(&stop.prompt, &stop.answer, &stop.options);
            if let Some(rule) = &stop.verify {
                check(rule, pos.board(), Some(pos), &stop.answer).unwrap_or_else(|err| {
                    panic!("{} after {}: {:?}: {err}", line.id, stop.after, stop.prompt)
                });
            }
        }
    }
}

#[test]
fn game_starts_are_legal() {
    for e in curriculum().phases.iter().flat_map(|p| &p.exercises) {
        if let Kind::Game(spec) = &e.kind
            && let Some(start) = &spec.start
        {
            playable(start);
        }
    }
}
