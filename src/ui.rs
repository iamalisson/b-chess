//! Drawing: every screen, dialogs, and the gnuchess-style board.

use crate::{
    app::{App, COLORS, Modal, Screen},
    levels::{describe, step},
    store::{color_name, plural},
    theme::{ACCENT, bg},
};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Margin, Rect},
    style::{Color as Tc, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, List, Paragraph, Row, Table, Wrap},
};
use shakmaty::{Chess, Color, EnPassantMode, File, Move, Position, Rank, Square, fen::Fen};

// Help is two columns side by side when the terminal is wide enough, stacked otherwise.
pub const HELP_LEFT: &[(&str, &str)] = &[
    ("#", "How to move"),
    ("e4  d5", "pawn: just the square it goes to"),
    ("Nf3  Bb5  Qh5", "piece letter + square (K Q R B N)"),
    ("exd5  Nxe5", "x captures; a pawn names its file"),
    ("Nbd7  R1e2", "two can go there? add file or rank"),
    ("O-O  O-O-O", "castle short / long (0-0 works)"),
    ("e8=Q", "promote (=Q =R =B =N)"),
    ("e2e4  g1f3", "or: from square + to square"),
    ("", ""),
    ("#", "In a game"),
    ("board  b  Tab", "show / hide the board (a peek)"),
    ("help  ?  F1", "show this help"),
    ("save  menu", "back to the menu, game is kept"),
    ("quit", "save the game and close the app"),
    ("resign", "give up (counts as a loss)"),
    ("Esc Esc", "abandon (counts as a loss)"),
];

pub const HELP_RIGHT: &[(&str, &str)] = &[
    ("#", "In the menu"),
    ("↑↓  ←→  Enter", "choose"),
    ("r  n  h", "resume, new game, history"),
    ("e  i", "export / import progress"),
    ("b", "background: dark, black, none"),
    ("?  q", "help, quit"),
    ("", ""),
    ("#", "Modes"),
    ("Ladder", "win = level up, loss = down"),
    ("Free play", "any level; ladder unchanged"),
    ("", ""),
    ("#", "Progress"),
    ("", "Saved after every move: close the"),
    ("", "app any time and resume later."),
    ("", "Export writes one file with all"),
    ("", "your games; Import it on another PC."),
];

impl App {
    pub fn draw(&mut self, f: &mut Frame) {
        solid(f, f.area());
        // A modal gets the screen to itself: the page behind is not drawn.
        match &self.modal {
            Some(Modal::Help) => draw_help(f),
            Some(Modal::Path { import, input }) => draw_path(f, *import, input),
            None => match self.screen {
                Screen::Menu => self.draw_menu(f),
                Screen::Setup => self.draw_setup(f),
                Screen::Game => self.draw_game(f),
                Screen::History => self.draw_history(f),
            },
        }
    }

    fn draw_menu(&mut self, f: &mut Frame) {
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

    fn draw_setup(&self, f: &mut Frame) {
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

    fn draw_game(&self, f: &mut Frame) {
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
            Span::raw(if g.ladder { "ladder" } else { "free play" }),
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

        if g.peek {
            let block = rounded(" Board (peeking) ").border_style(Style::new().fg(Tc::Yellow));
            let inner = block.inner(main);
            f.render_widget(block, main);
            let board = board_lines(&g.pos, g.me, g.last, inner.height >= 20);
            let h = board.len() as u16;
            f.render_widget(Paragraph::new(board), center(inner, 35, h));
        } else {
            let mut text = vec![];
            let n = g.san.len();
            for i in n.saturating_sub(2)..n {
                let who = if (i % 2 == 0) == (g.me == Color::White) {
                    "You"
                } else {
                    "Lc0"
                };
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
                text.push(Line::from("Game start. White to move.").dim());
                text.push(Line::from(""));
            }
            if g.pos.is_check() {
                text.push(Line::from("CHECK").red().bold());
            }
            text.push(Line::from(""));
            text.push(Line::from("Type board (or Tab) to peek").dim());
            let h = text.len() as u16;
            let block = rounded(" Blindfold ");
            let inner = block.inner(main);
            f.render_widget(block, main);
            let text: Vec<Line> = text.into_iter().map(|l| l.centered()).collect();
            f.render_widget(Paragraph::new(text), center(inner, inner.width, h));
        }

        let prompt = "▶ ";
        let input_block = rounded(" Your move ").border_style(Style::new().fg(ACCENT));
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
            let change = if g.ladder {
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
            popup(f, 38, 8, text, color);
        }
    }

    fn draw_history(&self, f: &mut Frame) {
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
                if r.ladder { "ladder" } else { "free" }.into(),
                format!("Elo {}", r.level),
                color_name(r.color).into(),
                res.into(),
                r.moves.to_string(),
                r.peeks.to_string(),
                if r.ladder {
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
}

/// Clears `area` and paints the app background on it.
pub fn solid(f: &mut Frame, area: Rect) {
    f.render_widget(Clear, area);
    f.render_widget(Block::new().style(Style::new().bg(bg())), area);
}

pub fn rounded(title: &str) -> Block<'_> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
}

pub fn center(area: Rect, w: u16, h: u16) -> Rect {
    let [a] = Layout::horizontal([Constraint::Length(w)])
        .flex(Flex::Center)
        .areas(area);
    let [a] = Layout::vertical([Constraint::Length(h)])
        .flex(Flex::Center)
        .areas(a);
    a
}

pub fn popup<'a>(f: &mut Frame, w: u16, h: u16, text: impl Into<Text<'a>>, color: Tc) {
    let area = center(f.area(), w, h);
    solid(f, area);
    let block = rounded("").border_style(Style::new().fg(color));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let text = text.into();
    let th = text.height() as u16;
    f.render_widget(Paragraph::new(text), center(inner, inner.width, th));
}

pub fn help_lines(items: &[(&'static str, &'static str)]) -> Vec<Line<'static>> {
    items
        .iter()
        .map(|&(keys, what)| match keys {
            "#" => Line::from(what).bold().fg(ACCENT),
            "" => Line::from(format!("  {what}")),
            _ => Line::from(vec![
                Span::raw(format!("  {keys:<16}")).bold(),
                Span::raw(what).dim(),
            ]),
        })
        .collect()
}

pub fn draw_help(f: &mut Frame) {
    const COL: u16 = 56;
    let (left, right) = (help_lines(HELP_LEFT), help_lines(HELP_RIGHT));
    let wide = f.area().width >= 2 * COL + 8;
    let (w, h) = if wide {
        (2 * COL + 6, left.len().max(right.len()) as u16 + 4)
    } else {
        (COL + 4, (left.len() + right.len() + 1) as u16 + 4)
    };
    let area = center(f.area(), w, h.min(f.area().height));
    solid(f, area);
    let block = rounded(" Help ")
        .border_style(Style::new().fg(ACCENT))
        .title_bottom(Line::from(" any key to close ").dim().centered());
    let inner = block.inner(area).inner(Margin::new(1, 1));
    f.render_widget(block, area);
    if wide {
        let [l, r] = Layout::horizontal([Constraint::Length(COL), Constraint::Length(COL)])
            .spacing(2)
            .areas(inner);
        f.render_widget(Paragraph::new(left), l);
        f.render_widget(Paragraph::new(right), r);
    } else {
        let mut all = left;
        all.push(Line::from(""));
        all.extend(right);
        f.render_widget(Paragraph::new(all), inner);
    }
}

pub fn draw_path(f: &mut Frame, import: bool, input: &str) {
    let area = center(f.area(), 64, 8);
    solid(f, area);
    let (title, note) = if import {
        (
            " Import progress from ",
            "Replaces the games and saved game on this PC.",
        )
    } else {
        (
            " Export progress to ",
            "Copy this file to the other PC, then Import it there.",
        )
    };
    let block = rounded(title)
        .border_style(Style::new().fg(ACCENT))
        .title_bottom(Line::from(" Enter confirm · Esc cancel ").dim().centered());
    let inner = block.inner(area).inner(Margin::new(1, 1));
    f.render_widget(block, area);
    let text = vec![
        Line::from(vec![Span::raw("▶ ").fg(ACCENT), Span::raw(input).bold()]),
        Line::from(""),
        Line::from(note)
            .fg(if import { Tc::Yellow } else { Tc::Reset })
            .dim(),
    ];
    f.render_widget(Paragraph::new(text), inner);
    f.set_cursor_position((inner.x + 2 + input.chars().count() as u16, inner.y));
}

/// gnuchess-style board: letters (white uppercase), dots for empty squares, last move in
/// yellow. `spaced` puts a blank line between ranks for a bigger board.
pub fn board_lines(pos: &Chess, me: Color, last: Option<Move>, spaced: bool) -> Vec<Line<'static>> {
    let mut ranks = Rank::ALL.to_vec();
    let mut files = File::ALL.to_vec();
    if me == Color::White {
        ranks.reverse();
    } else {
        files.reverse();
    }
    let marked = |sq: Square| last.is_some_and(|m| m.to() == sq || m.from() == Some(sq));

    // "black  KQkq  c3", like gnuchess: side to move, castling rights, en passant square.
    let fen = Fen::from_position(pos, EnPassantMode::Legal).to_string();
    let fields: Vec<&str> = fen.split(' ').collect();
    let turn = if pos.turn() == Color::White {
        "white"
    } else {
        "black"
    };
    let mut lines = vec![
        Line::from(format!("{turn}  {}  {}", fields[2], fields[3])).fg(ACCENT),
        Line::from(""),
    ];
    for (i, &r) in ranks.iter().enumerate() {
        if spaced && i > 0 {
            lines.push(Line::from(""));
        }
        let mut spans = vec![Span::raw(format!("{}  ", r.char())).dim()];
        for &f in &files {
            let sq = Square::from_coords(f, r);
            let span = match pos.board().piece_at(sq) {
                Some(p) => Span::raw(format!("  {} ", p.char())).bold(),
                None => Span::raw("  . ").dim(),
            };
            spans.push(if marked(sq) {
                span.fg(Tc::Yellow).bold()
            } else {
                span
            });
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    let labels: String = files.iter().map(|f| format!("  {} ", f.char())).collect();
    lines.push(Line::from(format!("   {labels}")).dim());
    lines
}
