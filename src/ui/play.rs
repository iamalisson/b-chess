//! Drawing the playing screens: menu, new-game setup, the game, and history.

use super::{
    board::{BoardOpts, board_lines, status_line},
    center, popup, rounded, solid,
};
use crate::{
    app::{App, COLORS},
    game::Game,
    levels::{describe, step},
    store::{Mode, color_name, plural},
    theme::ACCENT,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin},
    style::{Color as Tc, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, List, Paragraph, Row, Table, Wrap},
};
use shakmaty::{Color, Position};

impl App {
    pub(super) fn draw_menu(&mut self, f: &mut Frame) {
        let items = self.items();
        let area = center(f.area(), 56, 15 + items.len() as u16);
        solid(f, area);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ACCENT))
            .title(Line::from(" b-chess ").bold().centered());
        let inner = block.inner(area);
        f.render_widget(block, area);
        let [title, stats, list, msg, help] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(items.len() as u16 + 1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner.inner(Margin::new(2, 0)));

        let heading = Text::from(vec![
            Line::from(""),
            Line::from("♞  Blindfold Trainer")
                .bold()
                .fg(ACCENT)
                .centered(),
        ]);
        f.render_widget(heading, title);

        let [w, d, l] = self.record();
        let row = |k: &str, v: String| {
            Line::from(vec![
                Span::raw(format!("{k:<16}")).dim(),
                Span::raw(v).bold(),
            ])
        };
        let ladder = self.ladder_level();
        let stats_text = vec![
            row(
                "Ladder level",
                format!("Elo {ladder}  ({})", describe(ladder)),
            ),
            row("Record", format!("{w}W  {d}D  {l}L")),
        ];
        f.render_widget(Paragraph::new(stats_text), stats);

        let labels: Vec<String> = items.iter().map(|&i| self.label(i)).collect();
        let list_widget = List::new(labels)
            .highlight_symbol("▶ ")
            .highlight_style(Style::new().fg(ACCENT).bold());
        f.render_stateful_widget(list_widget, list, &mut self.menu);

        if let Some((m, c)) = &self.msg {
            f.render_widget(
                Paragraph::new(m.as_str()).fg(*c).wrap(Wrap { trim: true }),
                msg,
            );
        }
        f.render_widget(
            Line::from("↑↓ select · Enter open · ? help · q quit")
                .dim()
                .centered(),
            help,
        );
    }

    pub(super) fn draw_setup(&self, f: &mut Frame) {
        let s = &self.setup;
        let area = center(f.area(), 60, 15);
        solid(f, area);
        let block = rounded(" New game ")
            .border_style(Style::new().fg(ACCENT))
            .title_bottom(
                Line::from(" ↑↓ row · ←→ change · Enter start · Esc back ")
                    .dim()
                    .centered(),
            );
        let inner = block.inner(area).inner(Margin::new(2, 1));
        f.render_widget(block, area);

        let level = if s.ladder {
            self.ladder_level()
        } else {
            self.levels[s.level]
        };
        let level_value = if s.ladder {
            format!("Elo {level}  (your ladder)")
        } else {
            format!("◀ Elo {level} ▶")
        };
        let rows = [
            (
                "Mode",
                format!("◀ {} ▶", if s.ladder { "Ladder" } else { "Free play" }),
            ),
            ("Color", format!("◀ {} ▶", COLORS[s.color])),
            ("Level", level_value),
            ("", "[ Start ]".to_string()),
        ];
        let mut lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .map(|(i, (k, v))| {
                let sel = i == s.row;
                let marker = if sel { "▶ " } else { "  " };
                let value = Span::raw(v.clone());
                Line::from(vec![
                    Span::raw(marker).fg(ACCENT),
                    Span::raw(format!("{k:<8}")).dim(),
                    if sel { value.fg(ACCENT).bold() } else { value },
                ])
            })
            .collect();
        lines.insert(3, Line::from(""));
        lines.push(Line::from(""));
        let about = if s.ladder {
            "Start at the bottom and climb: a win moves you up a level, a loss down."
        } else {
            "Pick any level. Free games are recorded but do not move your ladder."
        };
        lines.push(Line::from(about).dim());
        lines.push(Line::from(format!("Bot: Lc0 with {}", describe(level))).dim());
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    }

    pub(super) fn draw_game(&self, f: &mut Frame) {
        let Some(g) = &self.game else { return };
        let full = f.area();
        let area = center(full, full.width.min(110), full.height.min(36));
        solid(f, area);
        let [header, body, input, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(area);

        let head = Line::from(vec![
            Span::raw(" ♞ b-chess ").bold().fg(ACCENT),
            Span::raw(" │ ").dim(),
            Span::raw(format!("vs Lc0 · Elo {}", g.level)).bold(),
            Span::raw(format!(" ({})", describe(g.level))).dim(),
            Span::raw(" │ ").dim(),
            Span::raw(format!("you: {}", color_name(g.me))),
            Span::raw(" │ ").dim(),
            Span::raw(g.mode.label()),
            Span::raw(" │ ").dim(),
            Span::raw(format!("peeks: {}", g.peeks)).fg(if g.peeks == 0 {
                Tc::Green
            } else {
                Tc::Yellow
            }),
        ]);
        f.render_widget(Paragraph::new(head).block(rounded("")), header);

        let [moves_area, main] =
            Layout::horizontal([Constraint::Length(26), Constraint::Min(30)]).areas(body);

        // Move list, scrolled to the bottom.
        let lines: Vec<Line> = g
            .san
            .chunks(2)
            .enumerate()
            .map(|(i, pair)| {
                let mut spans = vec![Span::raw(format!("{:>3}. ", i + 1)).dim()];
                for (j, s) in pair.iter().enumerate() {
                    let span = Span::raw(format!("{s:<9}"));
                    spans.push(if i * 2 + j == g.san.len() - 1 {
                        span.fg(Tc::Yellow).bold()
                    } else {
                        span
                    });
                }
                Line::from(spans)
            })
            .collect();
        let visible = moves_area.height.saturating_sub(2) as usize;
        let scroll = lines.len().saturating_sub(visible) as u16;
        f.render_widget(
            Paragraph::new(lines)
                .scroll((scroll, 0))
                .block(rounded(" Moves ")),
            moves_area,
        );

        if g.peek || g.aids.ghost {
            let (title, color) = if g.peek {
                (" Board (peeking) ", Tc::Yellow)
            } else {
                (" Invisible pieces ", ACCENT)
            };
            let block = rounded(title).border_style(Style::new().fg(color));
            let inner = block.inner(main);
            f.render_widget(block, main);
            let opts = BoardOpts {
                white_side: g.me == Color::White,
                last: if g.peek { g.last } else { None },
                spaced: inner.height >= 26,
                pieces: g.peek,
                highlight: None,
            };
            let mut board = board_lines(g.pos.board(), &opts);
            board.insert(0, status_line(&g.pos));
            board.insert(1, Line::from(""));
            board.push(Line::from(""));
            board.extend(self.game_text(g).into_iter().map(|l| l.centered()));
            let h = (board.len() as u16).min(inner.height);
            f.render_widget(
                Paragraph::new(board).centered(),
                center(inner, inner.width, h),
            );
        } else {
            let text = self.game_text(g);
            let h = text.len() as u16;
            let block = rounded(" Blindfold ");
            let inner = block.inner(main);
            f.render_widget(block, main);
            let text: Vec<Line> = text.into_iter().map(|l| l.centered()).collect();
            f.render_widget(Paragraph::new(text), center(inner, inner.width, h));
        }

        let prompt = "▶ ";
        let input_title = if g.asking().is_some() {
            " Your answer "
        } else {
            " Your move "
        };
        let input_block = rounded(input_title).border_style(Style::new().fg(ACCENT));
        let line = Line::from(vec![
            Span::raw(prompt).fg(ACCENT),
            Span::raw(&g.input).bold(),
        ]);
        f.render_widget(Paragraph::new(line).block(input_block), input);
        if g.result.is_none() && self.modal.is_none() {
            let x = input.x + 1 + (prompt.chars().count() + g.input.chars().count()) as u16;
            f.set_cursor_position((x, input.y + 1));
        }

        let foot = match &g.msg {
            _ if g.thinking => Line::from(" Lc0 is thinking...").fg(ACCENT),
            Some((m, c)) => Line::from(format!(" {m}")).fg(*c),
            None => Line::from(" Enter play · board · help · save · quit · resign").dim(),
        };
        f.render_widget(foot, footer);

        if let Some((result, why)) = &g.result {
            let (head, color) = match result {
                1 => ("You won!", Tc::Green),
                0 => ("Draw", Tc::Yellow),
                _ => ("You lost", Tc::Red),
            };
            let change = if let Mode::Training(_) = g.mode {
                "Training game: Enter to see the result".to_string()
            } else if g.mode.is_ladder() {
                format!(
                    "Ladder: Elo {} → {}",
                    g.level,
                    step(&self.levels, g.level, *result)
                )
            } else {
                "Free play: ladder unchanged".to_string()
            };
            let text = Text::from(vec![
                Line::from(head).bold().fg(color).centered(),
                Line::from(why.as_str()).dim().centered(),
                Line::from(""),
                Line::from(change).centered(),
                Line::from(""),
                Line::from("Enter to continue").dim().centered(),
            ]);
            popup(f, 46, 8, text, color);
        }
    }

    pub(super) fn draw_history(&self, f: &mut Frame) {
        let area = center(f.area(), 84, f.area().height.min(30));
        solid(f, area);
        let [w, d, l] = self.record();
        let best = self
            .history
            .iter()
            .filter(|r| r.result == 1)
            .map(|r| r.level)
            .max();
        let block = rounded(" History ")
            .border_style(Style::new().fg(ACCENT))
            .title_bottom(Line::from(" Esc back ").dim().centered());
        let inner = block.inner(area);
        f.render_widget(block, area);
        let [summary, table] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(inner);

        if self.history.is_empty() {
            f.render_widget(Line::from(" No games yet.").dim(), summary);
            return;
        }
        let best = best.map_or(String::new(), |b| format!(" · best win Elo {b}"));
        f.render_widget(
            Line::from(format!(
                " {} · {w}W {d}D {l}L{best}",
                plural(self.history.len(), "game")
            )),
            summary,
        );
        let rows = self.history.iter().rev().map(|r| {
            let (res, c) = match r.result {
                1 => ("win", Tc::Green),
                0 => ("draw", Tc::Yellow),
                _ => ("loss", Tc::Red),
            };
            Row::new([
                r.date.clone(),
                r.mode.label().into(),
                format!("Elo {}", r.level),
                color_name(r.color).into(),
                res.into(),
                r.moves.to_string(),
                r.peeks.to_string(),
                if r.mode.is_ladder() {
                    format!("→ {}", r.next)
                } else {
                    "-".into()
                },
            ])
            .fg(c)
        });
        let widths = [11, 7, 9, 6, 6, 6, 6, 7].map(Constraint::Length);
        let t = Table::new(rows, widths)
            .header(
                Row::new([
                    "Date", "Mode", "Lc0", "Color", "Result", "Moves", "Peeks", "Ladder",
                ])
                .bold()
                .fg(ACCENT),
            )
            .column_spacing(2);
        f.render_widget(t, table);
    }

    /// The last moves, check, and any question waiting for an answer.
    fn game_text(&self, g: &Game) -> Vec<Line<'static>> {
        let mut text = vec![];
        let n = g.san.len();
        for i in n.saturating_sub(2)..n {
            let mine = g.pos.turn() != g.me && i == n - 1 || g.pos.turn() == g.me && i + 2 == n;
            let who = if mine { "You" } else { "Lc0" };
            let line = Line::from(vec![
                Span::raw(format!("{who}  ")).dim(),
                Span::raw(g.numbered(i)).bold(),
            ]);
            text.push(if i == n - 1 {
                line.fg(Tc::Yellow)
            } else {
                line
            });
            text.push(Line::from(""));
        }
        if n == 0 {
            let side = if g.pos.turn() == Color::White {
                "White"
            } else {
                "Black"
            };
            text.push(Line::from(format!("Game start. {side} to move.")).dim());
            text.push(Line::from(""));
        }
        if g.pos.is_check() {
            text.push(Line::from("CHECK").red().bold());
        }
        if let Some(q) = g.asking() {
            text.push(Line::from(""));
            for l in q.prompt.lines() {
                text.push(Line::from(l.to_string()).fg(ACCENT).bold());
            }
            text.push(Line::from(format!("({})", q.format_hint())).dim());
        } else {
            text.push(Line::from(""));
            text.push(Line::from("Type board (or Tab) to peek").dim());
        }
        text
    }
}
