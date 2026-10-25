//! Drawing training mode: course home, a phase, notes, running exercises, results,
//! the training log and the "when stuck" page.

use super::{
    board::{BoardOpts, board_lines},
    center, rounded, solid,
};
use crate::{
    app::{Activity, App, HOME_EXTRAS, Run, View},
    theme::ACCENT,
    training::{
        curriculum::{Kind, LineMode, curriculum},
        progress::{Event, game_spec, phase_of, qualifies},
        quiz::{Feedback, QuizRun},
        reader::{LineRun, Stage},
    },
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color as Tc, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Paragraph, Row, Table, Wrap},
};
use shakmaty::Board;

fn panel(f: &mut Frame, w: u16, h: u16, title: &str, keys: &str) -> Rect {
    let full = f.area();
    let area = center(full, w.min(full.width), h.min(full.height));
    solid(f, area);
    let block = rounded(title)
        .border_style(Style::new().fg(ACCENT))
        .title_bottom(Line::from(format!(" {keys} ")).dim().centered());
    let inner = block.inner(area).inner(Margin::new(2, 1));
    f.render_widget(block, area);
    inner
}

fn feedback_line(fb: &Feedback) -> Vec<Line<'static>> {
    let note = |n: &Option<String>| {
        n.iter()
            .map(|n| Line::from(n.clone()).dim())
            .collect::<Vec<_>>()
    };
    match fb {
        Feedback::Right { note: n, slow } => {
            let mut v = vec![if *slow {
                Line::from("Right, but too slow for the streak.").fg(Tc::Yellow)
            } else {
                Line::from("Right.").fg(Tc::Green)
            }];
            v.extend(note(n));
            v
        }
        Feedback::Wrong { expected, note: n } => {
            let mut v = vec![Line::from(format!("Not quite. Answer: {expected}")).fg(Tc::Red)];
            v.extend(note(n));
            v
        }
        Feedback::Hint(h) => vec![Line::from(h.clone()).fg(Tc::Yellow)],
        Feedback::Info(i) => vec![Line::from(i.clone()).fg(Tc::Green)],
    }
}

fn input_line(input: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("▶ ").fg(ACCENT),
        Span::raw(input.to_string()).bold(),
    ])
}

impl App {
    pub(super) fn draw_training(&mut self, f: &mut Frame) {
        match self.training.view {
            View::Home => self.draw_course_home(f),
            View::Phase => self.draw_phase(f),
            View::Notes => self.draw_notes(f),
            View::Activity => self.draw_activity(f),
            View::Result => self.draw_result(f),
            View::Log => self.draw_log(f),
            View::Stuck => self.draw_stuck(f),
        }
    }

    fn draw_course_home(&mut self, f: &mut Frame) {
        let course = curriculum();
        let t = &self.training;
        let rows = course.phases.len() + HOME_EXTRAS.len();
        let inner = panel(
            f,
            76,
            rows as u16 + 12,
            " Training: blindfold course ",
            "↑↓ choose · Enter open · s today's session · Esc back",
        );
        let current = t.progress.current_phase(course);
        let mut lines = vec![
            Line::from("♞  From zero to a cup of coffee")
                .bold()
                .fg(ACCENT)
                .centered(),
            Line::from("10 weeks · 15–30 minutes a day · say the moves out loud")
                .dim()
                .centered(),
            Line::from(""),
        ];
        for (i, phase) in course.phases.iter().enumerate() {
            let checkpoint: Vec<_> = phase.exercises.iter().filter(|e| e.checkpoint).collect();
            let done = checkpoint
                .iter()
                .filter(|e| t.progress.passed(&e.id))
                .count();
            let (mark, color) = if t.progress.checkpoint_done(course, phase.number) {
                ("✓", Tc::Green)
            } else if phase.number == current {
                ("▶", ACCENT)
            } else if t.progress.is_open(course, phase.number) {
                ("·", Tc::Reset)
            } else {
                ("×", Tc::DarkGray)
            };
            let text = format!(
                " {mark}  Phase {}  {:<34} checkpoint {done}/{}",
                phase.number,
                phase.title,
                checkpoint.len()
            );
            let line = Line::from(text).fg(color);
            lines.push(if t.home_cursor == i {
                line.reversed()
            } else {
                line
            });
        }
        lines.push(Line::from(""));
        for (j, extra) in HOME_EXTRAS.iter().enumerate() {
            let label = if j == 0 {
                format!("    {extra}  (Phase {current})")
            } else {
                format!("    {extra}")
            };
            let line = Line::from(label);
            lines.push(if t.home_cursor == course.phases.len() + j {
                line.reversed()
            } else {
                line
            });
        }
        lines.push(Line::from(""));
        if t.progress.graduated(course) {
            lines.push(
                Line::from("☕  You graduated. Enjoy the coffee, and keep it up.")
                    .fg(Tc::Green)
                    .centered(),
            );
        }
        if let Some((m, c)) = &t.msg {
            lines.push(Line::from(m.clone()).fg(*c).centered());
        }
        f.render_widget(Paragraph::new(lines), inner);
    }

    fn draw_phase(&mut self, f: &mut Frame) {
        let course = curriculum();
        let t = &self.training;
        let phase = &course.phases[t.phase as usize];
        let inner = panel(
            f,
            92,
            phase.exercises.len() as u16 + 14,
            &format!(" Phase {}: {} ", phase.number, phase.title),
            "↑↓ choose · Enter start · n notes · s today's session · Esc back",
        );
        let mut lines = vec![Line::from(phase.goal.clone()).italic(), Line::from("")];
        let checkpoint: Vec<_> = phase.exercises.iter().filter(|e| e.checkpoint).collect();
        let done = checkpoint
            .iter()
            .filter(|e| t.progress.passed(&e.id))
            .count();
        lines.push(
            Line::from(format!(
                "Checkpoint: {done} of {} done (◆ items)",
                checkpoint.len()
            ))
            .fg(if done == checkpoint.len() {
                Tc::Green
            } else {
                ACCENT
            }),
        );
        lines.push(Line::from(""));
        for (i, e) in phase.exercises.iter().enumerate() {
            let passed = t.progress.passed(&e.id);
            let status = match &e.kind {
                Kind::Game(spec) => {
                    let saved = t.saved_game.as_ref().is_some_and(
                        |s| matches!(&s.mode, crate::store::Mode::Training(x) if *x == e.id),
                    );
                    let status = if passed {
                        String::new()
                    } else {
                        t.progress.game_status(&e.id, spec)
                    };
                    match saved {
                        true => format!("  {status}  (saved game: Enter resumes)"),
                        false => format!("  {status}"),
                    }
                }
                _ => String::new(),
            };
            let line = Line::from(vec![
                Span::raw(if passed { " ✓ " } else { "   " }).fg(Tc::Green),
                Span::raw(if e.checkpoint { "◆ " } else { "  " }).fg(ACCENT),
                Span::raw(e.title.clone()),
                Span::raw(status).dim(),
            ]);
            lines.push(if t.cursor == i { line.reversed() } else { line });
        }
        if let Some((m, c)) = &t.msg {
            lines.push(Line::from(""));
            lines.push(Line::from(m.clone()).fg(*c));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    fn draw_notes(&mut self, f: &mut Frame) {
        let course = curriculum();
        let phase = &course.phases[self.training.phase as usize];
        let full = f.area();
        let inner = panel(
            f,
            90,
            full.height.saturating_sub(2),
            &format!(" Phase {} notes ", phase.number),
            "↑↓ scroll · Esc back",
        );
        let text = Text::from(phase.notes.clone());
        f.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((self.training.scroll, 0)),
            inner,
        );
    }

    fn draw_activity(&mut self, f: &mut Frame) {
        let t = &self.training;
        let Some(Activity { title, run, .. }) = &t.activity else {
            return;
        };
        let session = t.session.as_ref().map_or(String::new(), |s| {
            let left = s.time_left().as_secs();
            format!(
                " · session block {}/{} · {}:{:02} left",
                s.index + 1,
                s.blocks.len(),
                left / 60,
                left % 60
            )
        });
        let full = f.area();
        let inner = panel(
            f,
            96,
            full.height.saturating_sub(2).min(40),
            &format!(" {title} "),
            &format!("Enter answer · Esc stop{session}"),
        );
        match run {
            Run::Quiz(q) => draw_quiz(f, inner, q, &t.input),
            Run::Line(l) => draw_line(f, inner, l, &t.input),
        }
    }

    fn draw_result(&mut self, f: &mut Frame) {
        let t = &self.training;
        let Some(o) = &t.outcome else { return };
        let next = t.upcoming().map(|(id, again)| {
            let title = curriculum()
                .exercise(&id)
                .map_or(id, |(_, e)| e.title.clone());
            (title, again)
        });
        let keys = match &next {
            Some((_, true)) => "Enter go again · Esc end session",
            Some((_, false)) => "Enter next block · Esc end session",
            None if t.session.is_some() => "Enter finish session",
            None => "Enter back",
        };
        let inner = panel(f, 76, 18, " Result ", keys);
        let mut lines = vec![
            Line::from(o.title.clone()).bold().centered(),
            Line::from(""),
            Line::from(o.score.clone()).centered(),
            Line::from(""),
            if o.passed {
                Line::from("Passed").fg(Tc::Green).bold().centered()
            } else {
                Line::from("Not passed yet: try again tomorrow")
                    .fg(Tc::Yellow)
                    .centered()
            },
        ];
        if o.newly_passed {
            lines.push(
                Line::from("New ✓ on this exercise")
                    .fg(Tc::Green)
                    .centered(),
            );
        }
        if let Some(n) = o.unlocked {
            lines.push(
                Line::from(format!("Checkpoint complete: Phase {n} unlocked!"))
                    .fg(ACCENT)
                    .bold()
                    .centered(),
            );
        }
        if o.graduated {
            lines.push(
                Line::from("☕  You graduated! The cup of coffee is yours.")
                    .fg(Tc::Green)
                    .bold()
                    .centered(),
            );
        }
        if let Some(tip) = &o.tip {
            lines.push(Line::from(""));
            lines.push(Line::from("When stuck:").fg(Tc::Yellow).centered());
            lines.push(Line::from(tip.clone()).dim().centered());
        }
        if let Some((title, again)) = next {
            let label = if again {
                "Again (time left in this block)"
            } else {
                "Next"
            };
            lines.push(Line::from(""));
            lines.push(Line::from(format!("{label}: {title}")).dim().centered());
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    }

    fn draw_log(&mut self, f: &mut Frame) {
        let course = curriculum();
        let full = f.area();
        let inner = panel(
            f,
            100,
            full.height.saturating_sub(2),
            " Training log ",
            "↑↓ scroll · Esc back",
        );
        let title = |id: &str| {
            course
                .exercise(id)
                .map_or(id.to_string(), |(_, e)| e.title.clone())
        };
        let rows: Vec<Row> = self
            .training
            .progress
            .events
            .iter()
            .rev()
            .filter_map(|e| match e {
                Event::Log {
                    date,
                    phase,
                    exercise,
                    seconds,
                    right,
                    total,
                    passed,
                } => Some(
                    Row::new([
                        date.clone(),
                        phase.to_string(),
                        title(exercise),
                        if *seconds > 0 {
                            format!("{}:{:02}", seconds / 60, seconds % 60)
                        } else {
                            "-".into()
                        },
                        format!("{right}/{total}"),
                        if *passed { "✓".into() } else { String::new() },
                    ])
                    .fg(if *passed { Tc::Green } else { Tc::Reset }),
                ),
                game @ Event::Game {
                    date,
                    exercise,
                    result,
                    illegal,
                    peeks,
                    moves,
                    ..
                } => {
                    let r = match result {
                        1 => "won",
                        0 => "draw",
                        _ => "lost",
                    };
                    let phase = phase_of(course, exercise);
                    let ok = game_spec(course, exercise).is_some_and(|spec| qualifies(game, spec));
                    Some(Row::new([
                        date.clone(),
                        phase.to_string(),
                        title(exercise),
                        format!("{moves} mv"),
                        format!("{r} · {illegal} illegal · {peeks} peeks"),
                        if ok { "✓".into() } else { String::new() },
                    ]))
                }
                Event::Pass { .. } => None,
            })
            .skip(self.training.scroll as usize)
            .collect();
        if rows.is_empty() {
            f.render_widget(
                Line::from("Nothing yet: your training shows up here.").dim(),
                inner,
            );
            return;
        }
        let widths = [11, 5, 44, 6, 28, 2].map(Constraint::Length);
        let header = Row::new(["Date", "Phase", "Exercise", "Time", "Score", ""])
            .bold()
            .fg(ACCENT);
        f.render_widget(
            Table::new(rows, widths).header(header).column_spacing(1),
            inner,
        );
    }

    fn draw_stuck(&mut self, f: &mut Frame) {
        let full = f.area();
        let inner = panel(
            f,
            90,
            full.height.saturating_sub(2),
            " When stuck ",
            "↑↓ scroll · Esc back",
        );
        let mut lines = vec![];
        for tip in &curriculum().stuck {
            lines.push(Line::from(format!("“{}”", tip.problem)).bold().fg(ACCENT));
            lines.push(Line::from(tip.advice.clone()));
            lines.push(Line::from(""));
        }
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((self.training.scroll, 0)),
            inner,
        );
    }
}

fn draw_quiz(f: &mut Frame, area: Rect, q: &QuizRun, input: &str) {
    let mut info = vec![Span::raw(format!("right {}/{}", q.right, q.total)).bold()];
    if q.best_streak > 0 {
        info.push(Span::raw(format!(" · best streak {}", q.best_streak)));
    }
    if let Some(left) = q.time_left() {
        info.push(
            Span::raw(format!(" · {} s left", left.as_secs()))
                .fg(Tc::Yellow)
                .bold(),
        );
    } else {
        info.push(Span::raw(format!(" · {} to go", q.left())).dim());
    }
    let mut lines = vec![Line::from(info), Line::from("")];
    // The previous answer's feedback stays on screen above the next question.
    if let Some(fb) = &q.feedback {
        lines.extend(feedback_line(fb));
        lines.push(Line::from(""));
    }
    if let Some(question) = &q.current {
        lines.extend(
            question
                .prompt
                .lines()
                .map(|l| Line::from(l.to_string()).bold()),
        );
        if let Some(view) = question.board {
            let opts = BoardOpts {
                white_side: view.white_side,
                last: None,
                spaced: false,
                pieces: false,
                highlight: Some(view.highlight),
            };
            lines.push(Line::from(""));
            lines.extend(board_lines(&Board::empty(), &opts));
        }
        lines.push(Line::from(""));
        lines.push(input_line(input));
        lines.push(Line::from(format!("({})", question.format_hint())).dim());
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn draw_line(f: &mut Frame, area: Rect, l: &LineRun, input: &str) {
    let [moves, body] = Layout::vertical([Constraint::Length(6), Constraint::Min(4)]).areas(area);
    let shown = if matches!(l.mode, LineMode::Recite) {
        format!("{} of {} moves recited", l.shown, l.total_moves())
    } else {
        l.moves_text()
    };
    f.render_widget(
        Paragraph::new(shown)
            .wrap(Wrap { trim: true })
            .block(rounded(&format!(" {} ", l.title))),
        moves,
    );
    let mut lines = vec![Line::from("")];
    if let Some(fb) = &l.feedback {
        lines.extend(feedback_line(fb));
        lines.push(Line::from(""));
    }
    let hint: Option<String> = match &l.stage {
        Stage::Reading if l.questions_pending() || matches!(l.mode, LineMode::Read) => {
            if !l.intro.is_empty() && l.right + l.total == 0 {
                lines.push(Line::from(l.intro.clone()).italic());
                lines.push(Line::from(""));
            }
            lines.push(
                Line::from(if matches!(l.mode, LineMode::Read) {
                    "Read it once, imagining the board. Enter when you are ready to rebuild the final position."
                } else {
                    "Follow the moves in your head. Enter for the questions."
                })
                .fg(ACCENT),
            );
            None
        }
        Stage::Reading => {
            lines.push(Line::from("The rest of the moves are shown. Enter to finish.").fg(ACCENT));
            None
        }
        Stage::Asking(q) => q.current.as_ref().map(|question| {
            lines.extend(
                question
                    .prompt
                    .lines()
                    .map(|s| Line::from(s.to_string()).bold()),
            );
            question.format_hint().to_string()
        }),
        Stage::Reciting => {
            lines.push(Line::from("From memory, one move at a time, both sides.").dim());
            lines.push(Line::from(l.recite_prompt()).bold());
            Some("a move, e.g. Nf3".into())
        }
        Stage::Done => None,
    };
    if let Some(h) = hint {
        lines.push(Line::from(""));
        lines.push(input_line(input));
        lines.push(Line::from(format!("({h})")).dim());
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
}
