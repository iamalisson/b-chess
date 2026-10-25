//! Question generators: random drills (colors, diagonals, walks, coordinates, attacks,
//! knight routes) and the questions asked during training games (checking habit,
//! "where is your knight?", recall).

use crate::levels::Rng;
use crate::training::{
    board_math::{
        all_squares, attacks_on_empty, describe_walk, diagonal_path, knight_distance, walk,
    },
    curriculum::{Generator, role_name},
    notation::pieces_of,
    question::{Answer, BoardView, Question},
};
use shakmaty::{Board, Chess, Color, Position, Role, Square};

fn random_square(rng: &mut Rng) -> Square {
    Square::new(rng.below(64) as u32)
}

fn pick<T: Copy>(rng: &mut Rng, items: &[T]) -> T {
    items[rng.below(items.len() as u64) as usize]
}

// Question templates, shared by the random drills and the course's fixed exercises.

pub fn color_question(sq: Square) -> Question {
    Question::new(
        format!("What color is {sq}?"),
        Answer::Color {
            light: sq.is_light(),
        },
    )
}

pub fn same_diagonal_question(a: Square, b: Square) -> Question {
    Question::new(
        format!("Are {a} and {b} on the same diagonal? If yes, list the squares of the path."),
        Answer::Diagonal(diagonal_path(a, b)),
    )
}

/// `None` if the walk leaves the board.
pub fn walk_question(start: Square, steps: &[(i32, i32)]) -> Option<Question> {
    let target = steps.iter().try_fold(start, |sq, &(f, r)| walk(sq, f, r))?;
    let path: Vec<String> = steps.iter().map(|&(f, r)| describe_walk(f, r)).collect();
    Some(Question::new(
        format!(
            "Start on {start}. Go {}. Which square do you reach?",
            path.join(", then ")
        ),
        Answer::Square(target),
    ))
}

pub fn attacks_question(role: Role, sq: Square) -> Question {
    Question::new(
        format!(
            "Empty board: list every square a {} on {sq} attacks.",
            role_name(role)
        ),
        Answer::Squares(attacks_on_empty(role, sq)),
    )
}

pub fn attack_count_question(role: Role, sq: Square) -> Question {
    Question::new(
        format!(
            "Empty board: how many squares does a {} on {sq} attack?",
            role_name(role)
        ),
        Answer::Count(attacks_on_empty(role, sq).len() as u32),
    )
}

pub fn route_question(a: Square, b: Square) -> Question {
    let n = knight_distance(a, b);
    Question::new(
        format!(
            "Shortest knight route from {a} to {b} ({n} moves). List the squares, both ends included."
        ),
        Answer::Route { from: a, to: b },
    )
}

/// One random question from `generator`.
pub fn generate(generator: Generator, rng: &mut Rng) -> Question {
    match generator {
        Generator::Color => color_question(random_square(rng)),
        Generator::SameDiagonal => {
            let a = random_square(rng);
            // Half the time pick a square on one of a's diagonals, so "yes" comes up often.
            let partners: Vec<Square> = all_squares()
                .filter(|&b| diagonal_path(a, b).is_some_and(|p| p.len() > 2))
                .collect();
            let b = if rng.below(2) == 0 && !partners.is_empty() {
                pick(rng, &partners)
            } else {
                let mut b = random_square(rng);
                while b == a {
                    b = random_square(rng);
                }
                b
            };
            same_diagonal_question(a, b)
        }
        Generator::Walk => loop {
            let start = random_square(rng);
            let n = 1 + rng.below(2);
            let steps: Vec<(i32, i32)> = (0..n)
                .map(|_| {
                    let f = rng.below(7) as i32 - 3;
                    let r = rng.below(7) as i32 - 3;
                    if rng.below(3) == 0 {
                        (f, f * if rng.below(2) == 0 { 1 } else { -1 })
                    } else {
                        (f, r)
                    }
                })
                .filter(|&s| s != (0, 0))
                .collect();
            if let Some(q) = walk_question(start, &steps).filter(|_| !steps.is_empty()) {
                break q;
            }
        },
        Generator::CoordinatesWhite | Generator::CoordinatesBlack => {
            let sq = random_square(rng);
            let white_side = generator == Generator::CoordinatesWhite;
            Question::new("Name the highlighted square.", Answer::Square(sq)).showing(BoardView {
                highlight: sq,
                white_side,
            })
        }
        Generator::Attacks => {
            let roles = [
                Role::Knight,
                Role::Bishop,
                Role::Rook,
                Role::Queen,
                Role::King,
            ];
            let role = pick(rng, &roles);
            let sq = random_square(rng);
            if matches!(role, Role::Queen | Role::Rook) && rng.below(2) == 0 {
                attack_count_question(role, sq)
            } else {
                attacks_question(role, sq)
            }
        }
        Generator::KnightAttacks => attacks_question(Role::Knight, random_square(rng)),
        Generator::KnightRoute => loop {
            let (a, b) = (random_square(rng), random_square(rng));
            if (2..=5).contains(&knight_distance(a, b)) {
                break route_question(a, b);
            }
        },
    }
}

/// Pieces of `color` (not the king) that are attacked and not defended.
pub fn loose_pieces(board: &Board, color: Color) -> Vec<Square> {
    let occupied = board.occupied();
    board
        .by_color(color)
        .into_iter()
        .filter(|&sq| board.role_at(sq) != Some(Role::King))
        .filter(|&sq| {
            board.attacks_to(sq, !color, occupied).any()
                && !board.attacks_to(sq, color, occupied).any()
        })
        .collect()
}

/// The checking habit: where both kings are, and what is loose on each side.
pub fn checking_habit(pos: &Chess, me: Color) -> Vec<Question> {
    let board = pos.board();
    let king = |c| board.king_of(c).expect("both kings are on the board");
    vec![
        Question::new(
            "Checking habit. Where is your king?",
            Answer::Square(king(me)),
        ),
        Question::new("Where is Lc0's king?", Answer::Square(king(!me))),
        Question::new(
            "Which of your pieces are attacked and undefended? (squares, or none)",
            Answer::Squares(loose_pieces(board, me)),
        ),
        Question::new(
            "Which of Lc0's pieces are attacked and undefended? (squares, or none)",
            Answer::Squares(loose_pieces(board, !me)),
        ),
    ]
}

/// "Where is your knight?" for a random piece type you still have (kings and pawns aside
/// when possible).
pub fn piece_question(pos: &Chess, me: Color, rng: &mut Rng) -> Question {
    let mine = pieces_of(pos.board(), me);
    let mut roles: Vec<Role> = mine
        .iter()
        .map(|&(r, _)| r)
        .filter(|r| !matches!(r, Role::King | Role::Pawn))
        .collect();
    roles.dedup();
    let role = if roles.is_empty() {
        Role::King
    } else {
        pick(rng, &roles)
    };
    let squares: Vec<Square> = mine
        .iter()
        .filter(|&&(r, _)| r == role)
        .map(|&(_, s)| s)
        .collect();
    let plural = if squares.len() > 1 { "s" } else { "" };
    Question::new(
        format!(
            "Quick: where is your {}{plural}? (square{plural})",
            role_name(role)
        ),
        Answer::Squares(squares),
    )
}

/// Rebuild the position: list both sides' pieces.
pub fn recall(pos: &Chess, me: Color) -> Vec<Question> {
    let board = pos.board();
    vec![
        Question::new(
            "Recall: list your pieces.",
            Answer::Pieces(pieces_of(board, me)),
        ),
        Question::new(
            "Recall: list Lc0's pieces.",
            Answer::Pieces(pieces_of(board, !me)),
        ),
    ]
}

/// Rebuild a position by color: list White's pieces, then Black's.
pub fn recall_both(pos: &Chess) -> Vec<Question> {
    let board = pos.board();
    vec![
        Question::new(
            "Rebuild the final position. List White's pieces.",
            Answer::Pieces(pieces_of(board, Color::White)),
        ),
        Question::new(
            "List Black's pieces.",
            Answer::Pieces(pieces_of(board, Color::Black)),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training::{
        notation::{parse_board, to_position},
        question::Verdict,
    };
    use shakmaty::Square::*;

    #[test]
    fn generated_questions_answer_themselves() {
        let mut rng = Rng::new();
        let gens = [
            Generator::Color,
            Generator::SameDiagonal,
            Generator::Walk,
            Generator::CoordinatesBlack,
            Generator::Attacks,
            Generator::KnightAttacks,
            Generator::KnightRoute,
        ];
        for g in gens {
            for _ in 0..50 {
                let q = generate(g, &mut rng);
                let expected = q.expected();
                let given = expected.trim_start_matches("yes: ");
                assert!(
                    matches!(q.grade(given), Verdict::Right),
                    "{g:?}: {} -> {expected}",
                    q.prompt
                );
            }
        }
    }

    #[test]
    fn checking_habit_sees_loose_pieces() {
        let board = parse_board("White: Kg1, Nc3. Black: Kg8, Bb4.").unwrap();
        let pos = to_position(&board, Color::White).unwrap();
        assert_eq!(loose_pieces(pos.board(), Color::White), vec![C3]);
        assert_eq!(
            loose_pieces(pos.board(), Color::Black),
            Vec::<Square>::new()
        );
        let qs = checking_habit(&pos, Color::White);
        assert!(matches!(qs[0].grade("g1"), Verdict::Right));
        assert!(matches!(qs[2].grade("c3"), Verdict::Right));
        assert!(matches!(qs[3].grade("none"), Verdict::Right));
    }
}
