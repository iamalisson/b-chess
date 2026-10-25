//! Drawing: dispatch, shared widgets, help and dialogs. Screens live in `play` and
//! `training`; the board in `board`.

mod board;
mod play;
mod training;

use crate::{
    app::{App, Modal, Screen},
    theme::{ACCENT, bg},
};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Margin, Rect},
    style::{Color as Tc, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, Paragraph},
};

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
    ("recall", "rebuild the position (training)"),
];

pub const HELP_RIGHT: &[(&str, &str)] = &[
    ("#", "In the menu"),
    ("↑↓  ←→  Enter", "choose"),
    ("r  n  t  h", "resume, new game, training, history"),
    ("e  i", "export / import progress"),
    ("b", "background: dark, black, none"),
    ("?  q", "help, quit"),
    ("", ""),
    ("#", "Modes"),
    ("Ladder", "win = level up, loss = down"),
    ("Free play", "any level; ladder unchanged"),
    ("Training", "the 10-week blindfold course"),
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
                Screen::Training => self.draw_training(f),
            },
        }
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
            "Replaces games and saved games here; training progress only if the file has it.",
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
