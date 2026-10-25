//! Training mode: the course home, a phase's exercises, running quizzes and lines,
//! today's session, results, the log and the "when stuck" page.

use super::{App, Screen};
use crate::{
    game::{Aids, Game},
    levels::{Rng, step},
    store::{Mode, SavedGame, Slot, load_saved, plural, today},
    training::{
        curriculum::{Kind, LineMode, curriculum},
        notation::{parse_board, to_position},
        progress::{Event, Progress, game_spec, phase_of},
        quiz::QuizRun,
        reader::{LineRun, Stage},
        session::Session,
    },
};
use ratatui::{DefaultTerminal, crossterm::event::KeyCode, style::Color as Tc};
use shakmaty::{Color, EnPassantMode, fen::Fen};
use std::io;

pub enum View {
    Home,
    Phase,
    Notes,
    Activity,
    Result,
    Log,
    Stuck,
}

pub enum Run {
    Quiz(Box<QuizRun>),
    Line(Box<LineRun>),
}

pub struct Activity {
    pub id: String,
    pub title: String,
    pub run: Run,
}

/// What the result screen shows after an exercise.
pub struct Outcome {
    pub title: String,
    pub score: String,
    pub passed: bool,
    pub newly_passed: bool,
    pub unlocked: Option<u8>,
    pub graduated: bool,
    /// Advice from "When stuck" when accuracy is low.
    pub tip: Option<String>,
}

/// The "When stuck" advice that fits a phase, shown when a run goes below 60%.
fn tip_for(phase: u8, right: u32, total: u32) -> Option<String> {
    if total < 5 || right * 100 / total >= 60 {
        return None;
    }
    let problem = match phase {
        0 | 1 => "I mix up squares",
        2 => "The knight confuses me",
        _ => "I forget where the pieces are",
    };
    curriculum()
        .stuck
        .iter()
        .find(|t| t.problem.starts_with(problem))
        .map(|t| t.advice.clone())
}

/// Drills repeat inside a session block until its minutes are used up.
fn repeats(id: &str) -> bool {
    matches!(curriculum().exercise(id), Some((_, e)) if matches!(e.kind, Kind::Drill { .. }))
}

pub struct Training {
    pub progress: Progress,
    /// The unfinished training game, in its own slot (never the ladder/free game's).
    pub saved_game: Option<SavedGame>,
    pub view: View,
    /// Cursor on the home list (phases, then session/log/stuck).
    pub home_cursor: usize,
    /// The phase open in the Phase view, and the cursor on its exercises.
    pub phase: u8,
    pub cursor: usize,
    pub input: String,
    pub activity: Option<Activity>,
    pub session: Option<Session>,
    pub outcome: Option<Outcome>,
    pub msg: Option<(String, Tc)>,
    pub scroll: u16,
}

/// Home rows after the 8 phases.
pub const HOME_EXTRAS: [&str; 3] = ["Today's session", "Training log", "When stuck"];

impl Training {
    pub fn load() -> Training {
        Training {
            progress: Progress::load(),
            saved_game: load_saved(Slot::Training),
            view: View::Home,
            home_cursor: 0,
            phase: 0,
            cursor: 0,
            input: String::new(),
            activity: None,
            session: None,
            outcome: None,
            msg: None,
            scroll: 0,
        }
    }

    /// A timed drill is running: the clock needs redrawing without input.
    pub fn ticking(&self) -> bool {
        matches!(self.view, View::Activity)
            && matches!(&self.activity, Some(Activity { run: Run::Quiz(q), .. }) if q.time_left().is_some())
    }

    /// Ends a timed drill whose clock ran out.
    pub fn tick(&mut self) {
        let expired = matches!(self.view, View::Activity)
            && matches!(&self.activity, Some(Activity { run: Run::Quiz(q), .. }) if q.is_done());
        if expired {
            self.input.clear();
            self.finish_activity();
        }
    }

    pub fn flash(&mut self, text: String, color: Tc) {
        self.msg = Some((text, color));
    }

    /// Shows a message on the exercise's phase page.
    pub fn show_message(&mut self, exercise: &str, text: &str) {
        self.open_phase_of(exercise);
        self.flash(text.to_string(), Tc::Green);
    }

    /// Shows an error (e.g. Lc0 didn't start) on the exercise's phase page.
    pub fn show_error(&mut self, exercise: &str, text: String) {
        self.open_phase_of(exercise);
        self.session = None;
        self.flash(text, Tc::Red);
    }

    fn open_phase_of(&mut self, exercise: &str) {
        let course = curriculum();
        self.phase = phase_of(course, exercise);
        self.cursor = course.phases[self.phase as usize]
            .exercises
            .iter()
            .position(|e| e.id == exercise)
            .unwrap_or(0);
        self.view = View::Phase;
    }

    /// What follows the result screen in a session: (exercise id, is a repeat).
    pub fn upcoming(&self) -> Option<(String, bool)> {
        let session = self.session.as_ref()?;
        session
            .upcoming(repeats)
            .map(|(id, again)| (id.to_string(), again))
    }

    /// Records a finished training game and checks its goal.
    pub fn game_finished(&mut self, id: &str, game: &Game) {
        let (result, why) = game.result.clone().unwrap_or((0, String::new()));
        let event = Event::Game {
            date: today(),
            exercise: id.to_string(),
            result,
            natural: game.ended_naturally(),
            illegal: game.illegal,
            peeks: game.peeks,
            moves: game.my_moves() as u32,
            quiz_right: game.quiz_right,
            quiz_total: game.quiz_total,
            reason: why.to_lowercase(),
        };
        let mut score = format!(
            "{} · {} · {} · {}",
            match result {
                1 => "won",
                0 => "draw",
                _ => "lost",
            },
            plural(game.my_moves(), "move"),
            plural(game.illegal as usize, "illegal move"),
            plural(game.peeks as usize, "peek"),
        );
        if game.quiz_total > 0 {
            score += &format!(" · questions {}/{}", game.quiz_right, game.quiz_total);
        }
        let tip = tip_for(phase_of(curriculum(), id), game.quiz_right, game.quiz_total);
        self.conclude(id, score, event, tip, |progress| {
            let spec = game_spec(curriculum(), id)?;
            Some((
                progress.game_goal_met(id, spec),
                progress.game_status(id, spec),
            ))
        });
    }

    /// Logs a finished quiz or line and records a pass.
    fn finish_activity(&mut self) {
        let Some(activity) = self.activity.take() else {
            return;
        };
        let (right, total, seconds, passed, score) = match &activity.run {
            Run::Quiz(q) => {
                let streak = match q.best_streak {
                    0 => String::new(),
                    n => format!(" · best fast streak {n}"),
                };
                let score = format!("{}/{} right{streak}", q.right, q.total);
                (q.right, q.total, q.seconds(), q.passed(), score)
            }
            Run::Line(l) => {
                let unit = if l.mode == LineMode::Recite {
                    " moves"
                } else {
                    ""
                };
                (
                    l.right,
                    l.total,
                    0,
                    l.passed(),
                    format!("{}/{}{unit} right", l.right, l.total),
                )
            }
        };
        let phase = phase_of(curriculum(), &activity.id);
        let event = Event::Log {
            date: today(),
            phase,
            exercise: activity.id.clone(),
            seconds,
            right,
            total,
            passed,
        };
        let tip = tip_for(phase, right, total);
        self.conclude(&activity.id, score, event, tip, |_| {
            Some((passed, String::new()))
        });
    }

    /// Shared ending for games and exercises: record the event, record a pass when
    /// `judge` says so (it sees the progress including the new event), note unlocks,
    /// and show the result. `judge` returns (passed, extra status text).
    fn conclude(
        &mut self,
        id: &str,
        mut score: String,
        event: Event,
        tip: Option<String>,
        judge: impl FnOnce(&Progress) -> Option<(bool, String)>,
    ) {
        let course = curriculum();
        let before = self.progress.current_phase(course);
        let mut newly_passed = false;
        let mut passed = false;
        let saved = self.progress.record(event).and_then(|_| {
            let (ok, status) = judge(&self.progress).unwrap_or_default();
            passed = ok;
            if !status.is_empty() {
                score += &format!(" · goal: {status}");
            }
            if ok {
                newly_passed = self.progress.mark_passed(id)?;
            }
            Ok(())
        });
        if let Err(e) = saved {
            self.flash(format!("Could not save progress: {e}"), Tc::Red);
        }
        let after = self.progress.current_phase(course);
        self.outcome = Some(Outcome {
            title: course
                .exercise(id)
                .map_or(id.to_string(), |(_, e)| e.title.clone()),
            score,
            passed,
            newly_passed,
            unlocked: (after > before).then_some(after),
            graduated: newly_passed && self.progress.graduated(course),
            tip,
        });
        self.phase = phase_of(course, id);
        self.view = View::Result;
    }
}

/// Training aids for a game, from its exercise.
pub fn aids_for(mode: &Mode) -> Aids {
    match mode {
        Mode::Training(id) => game_spec(curriculum(), id).map_or(Aids::default(), |s| Aids {
            ghost: s.ghost,
            check_habit: s.check_habit,
            piece_questions: s.piece_questions,
        }),
        _ => Aids::default(),
    }
}

impl App {
    pub fn open_training(&mut self) {
        let t = &mut self.training;
        t.view = View::Home;
        t.home_cursor = t.progress.current_phase(curriculum()) as usize;
        t.msg = None;
        self.screen = Screen::Training;
    }

    pub fn training_key(
        &mut self,
        code: KeyCode,
        terminal: &mut DefaultTerminal,
    ) -> io::Result<()> {
        let course = curriculum();
        let t = &mut self.training;
        match t.view {
            View::Home => {
                let rows = course.phases.len() + HOME_EXTRAS.len();
                match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        t.home_cursor = t.home_cursor.saturating_sub(1)
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        t.home_cursor = (t.home_cursor + 1).min(rows - 1)
                    }
                    KeyCode::Home => t.home_cursor = 0,
                    KeyCode::End => t.home_cursor = rows - 1,
                    KeyCode::Esc | KeyCode::Char('q') => self.screen = Screen::Menu,
                    KeyCode::Char('s') => return self.start_session(terminal),
                    KeyCode::Enter => {
                        let n = course.phases.len();
                        match t.home_cursor {
                            i if i < n => {
                                let phase = course.phases[i].number;
                                if t.progress.is_open(course, phase) {
                                    t.phase = phase;
                                    t.cursor = 0;
                                    t.view = View::Phase;
                                    t.msg = None;
                                } else {
                                    t.flash(
                                        format!(
                                            "Phase {phase} is locked: pass Phase {}'s checkpoint first.",
                                            phase - 1
                                        ),
                                        Tc::Yellow,
                                    );
                                }
                            }
                            i if i == n => return self.start_session(terminal),
                            i if i == n + 1 => {
                                t.scroll = 0;
                                t.view = View::Log;
                            }
                            _ => {
                                t.scroll = 0;
                                t.view = View::Stuck;
                            }
                        }
                    }
                    _ => {}
                }
            }
            View::Phase => {
                let phase = &course.phases[t.phase as usize];
                let last = phase.exercises.len() - 1;
                match code {
                    KeyCode::Up | KeyCode::Char('k') => t.cursor = t.cursor.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => t.cursor = (t.cursor + 1).min(last),
                    KeyCode::Home => t.cursor = 0,
                    KeyCode::End => t.cursor = last,
                    KeyCode::Char('n') => {
                        t.scroll = 0;
                        t.view = View::Notes;
                    }
                    KeyCode::Char('s') => return self.start_session(terminal),
                    KeyCode::Esc | KeyCode::Char('q') => t.view = View::Home,
                    KeyCode::Enter => {
                        let id = phase.exercises[t.cursor].id.clone();
                        t.session = None;
                        return self.start_exercise(&id, terminal);
                    }
                    _ => {}
                }
            }
            View::Notes | View::Log | View::Stuck => match code {
                KeyCode::Up | KeyCode::Char('k') => t.scroll = t.scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => t.scroll += 1,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    t.view = if matches!(t.view, View::Notes) {
                        View::Phase
                    } else {
                        View::Home
                    }
                }
                _ => {}
            },
            View::Activity => self.activity_key(code),
            View::Result => {
                if matches!(code, KeyCode::Enter | KeyCode::Esc) {
                    t.outcome = None;
                    return self.after_result(terminal, code == KeyCode::Esc);
                }
            }
        }
        Ok(())
    }

    fn activity_key(&mut self, code: KeyCode) {
        let t = &mut self.training;
        let Some(activity) = t.activity.as_mut() else {
            return;
        };
        match code {
            KeyCode::Esc => {
                let id = activity.id.clone();
                t.activity = None;
                t.input.clear();
                t.session = None;
                t.open_phase_of(&id);
                t.flash("Exercise stopped (not logged).".into(), Tc::Yellow);
                return;
            }
            KeyCode::Backspace => {
                t.input.pop();
            }
            KeyCode::Char(c) if t.input.len() < 120 => t.input.push(c),
            KeyCode::Enter => {
                let input = std::mem::take(&mut t.input);
                match &mut activity.run {
                    Run::Quiz(q) => q.answer(&input),
                    Run::Line(l) => match l.stage {
                        Stage::Reading => l.go_on(),
                        _ => l.answer(&input),
                    },
                }
            }
            _ => {}
        }
        let done = match &activity.run {
            Run::Quiz(q) => q.is_done(),
            Run::Line(l) => l.is_done(),
        };
        if done {
            t.finish_activity();
        }
    }

    /// Starts an exercise of an open phase. A game exercise resumes its saved game.
    pub fn start_exercise(&mut self, id: &str, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let course = curriculum();
        let Some((phase, exercise)) = course.exercise(id) else {
            return Ok(());
        };
        let t = &mut self.training;
        if !t.progress.is_open(course, phase.number) {
            t.flash(format!("Phase {} is locked.", phase.number), Tc::Yellow);
            return Ok(());
        }
        t.phase = phase.number;
        t.input.clear();
        t.msg = None;
        let run = match &exercise.kind {
            Kind::Quiz { pass, .. } => Run::Quiz(Box::new(QuizRun::fixed(
                course.quiz_questions(exercise),
                *pass,
            ))),
            Kind::Drill {
                generator,
                count,
                time_limit,
                pass,
            } => Run::Quiz(Box::new(QuizRun::drill(
                *generator,
                *count,
                *time_limit,
                *pass,
            ))),
            Kind::Line { line, mode } => Run::Line(Box::new(LineRun::new(
                course.line(line).expect("lines resolve (checked by tests)"),
                *mode,
            ))),
            Kind::Game(spec) => {
                let mode = Mode::Training(id.to_string());
                if let Some(saved) = t.saved_game.clone().filter(|s| s.mode == mode) {
                    return self.start_game(terminal, saved);
                }
                let start = spec.start.as_deref().map(|desc| {
                    let board = parse_board(desc).expect("valid position (checked by tests)");
                    let pos = to_position(&board, Color::White).expect("legal (checked by tests)");
                    Fen::from_position(&pos, EnPassantMode::Legal).to_string()
                });
                let level = match (spec.level, &start) {
                    (Some(l), _) => step(&self.levels, l, 0),
                    // Endgames need a defender that doesn't blunder the king away.
                    (None, Some(_)) => {
                        let strongest = *self.levels.last().expect("at least one level");
                        self.levels
                            .iter()
                            .copied()
                            .find(|&l| l >= 2200)
                            .unwrap_or(strongest)
                    }
                    (None, None) => self.ladder_level(),
                };
                let me = if start.is_some() || Rng::new().below(2) == 0 {
                    Color::White
                } else {
                    Color::Black
                };
                return self.start_game(terminal, SavedGame::new(level, me, mode, start));
            }
        };
        t.activity = Some(Activity {
            id: id.to_string(),
            title: exercise.title.clone(),
            run,
        });
        t.view = View::Activity;
        Ok(())
    }

    fn start_session(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let course = curriculum();
        let t = &mut self.training;
        let n = t.progress.current_phase(course);
        let session = Session::new(&course.phases[n as usize]);
        let first = session.current().map(|b| b.0.clone());
        t.session = Some(session);
        match first {
            Some(id) => self.start_exercise(&id, terminal),
            None => Ok(()),
        }
    }

    /// After the result screen: the next session block (or the same drill again), or back
    /// to the phase.
    fn after_result(&mut self, terminal: &mut DefaultTerminal, stop: bool) -> io::Result<()> {
        let t = &mut self.training;
        let upcoming = t.upcoming();
        let Some(session) = t.session.as_mut().filter(|_| !stop) else {
            t.session = None;
            t.view = View::Phase;
            return Ok(());
        };
        match upcoming {
            Some((id, again)) => {
                if !again {
                    session.advance();
                }
                self.start_exercise(&id, terminal)
            }
            None => {
                let minutes = session.total_minutes();
                t.session = None;
                t.view = View::Home;
                t.flash(
                    format!("Session done ({minutes} min planned). See you tomorrow."),
                    Tc::Green,
                );
                Ok(())
            }
        }
    }
}
