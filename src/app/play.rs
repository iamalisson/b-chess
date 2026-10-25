//! Input for playing: the menu, the new-game setup, the game itself and history.

use super::{App, Item, Modal, Screen};
use crate::{
    levels::Rng,
    store::{Mode, SavedGame, save_settings},
    theme::{BACKGROUNDS, BG},
};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{KeyCode, KeyEvent},
    style::Color as Tc,
};
use shakmaty::Color;
use std::{io, sync::atomic::Ordering};

impl App {
    pub fn menu_key(&mut self, code: KeyCode, terminal: &mut DefaultTerminal) -> io::Result<bool> {
        let item = match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.menu.select_previous();
                return Ok(false);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.menu.select_next();
                return Ok(false);
            }
            KeyCode::Enter => {
                let items = self.items();
                items[self.menu.selected().unwrap_or(0).min(items.len() - 1)]
            }
            KeyCode::Char('r') if self.saved.is_some() => Item::Resume,
            KeyCode::Char('n' | 'p') => Item::New,
            KeyCode::Char('t') => Item::Training,
            KeyCode::Char('h') => Item::History,
            KeyCode::Char('e') => Item::Export,
            KeyCode::Char('i') => Item::Import,
            KeyCode::Char('b') => Item::Background,
            KeyCode::Char('?') => Item::Help,
            KeyCode::Char('q') | KeyCode::Esc => Item::Quit,
            _ => return Ok(false),
        };
        match item {
            Item::Resume => {
                let saved = self.saved.take().unwrap();
                self.start_game(terminal, saved)?;
            }
            Item::New => {
                let ladder = self.ladder_level();
                self.setup.level = self.levels.iter().position(|&l| l == ladder).unwrap_or(0);
                self.setup.row = 0;
                self.screen = Screen::Setup;
            }
            Item::Training => self.open_training(),
            Item::History => self.screen = Screen::History,
            Item::Export | Item::Import => {
                let input = "~/b-chess-progress.txt".to_string();
                self.modal = Some(Modal::Path {
                    import: matches!(item, Item::Import),
                    input,
                });
            }
            Item::Background => {
                // Next preset; a custom color moves on to the first preset.
                let cur = BG.load(Ordering::Relaxed);
                let i = BACKGROUNDS
                    .iter()
                    .position(|&(_, c)| c == cur)
                    .map_or(0, |i| i + 1);
                BG.store(BACKGROUNDS[i % BACKGROUNDS.len()].1, Ordering::Relaxed);
                self.msg = save_settings()
                    .err()
                    .map(|e| (format!("Could not save: {e}"), Tc::Red));
            }
            Item::Help => self.modal = Some(Modal::Help),
            Item::Quit => return Ok(true),
        }
        Ok(false)
    }

    pub fn setup_key(&mut self, code: KeyCode, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let ladder_level = self.ladder_level();
        let s = &mut self.setup;
        let delta: i32 = match code {
            KeyCode::Up | KeyCode::Char('k') => {
                s.row = s.row.saturating_sub(1);
                return Ok(());
            }
            KeyCode::Down | KeyCode::Char('j') => {
                s.row = (s.row + 1).min(3);
                return Ok(());
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.screen = Screen::Menu;
                return Ok(());
            }
            KeyCode::Enter => {
                let mut rng = Rng::new();
                let me = match s.color {
                    0 => Color::White,
                    1 => Color::Black,
                    _ if rng.below(2) == 0 => Color::White,
                    _ => Color::Black,
                };
                let level = if s.ladder {
                    ladder_level
                } else {
                    self.levels[s.level]
                };
                let mode = if s.ladder { Mode::Ladder } else { Mode::Free };
                return self.start_game(terminal, SavedGame::new(level, me, mode, None));
            }
            KeyCode::Left | KeyCode::Char('h') => -1,
            KeyCode::Right | KeyCode::Char('l') => 1,
            _ => return Ok(()),
        };
        match s.row {
            0 => s.ladder = !s.ladder,
            1 => s.color = (s.color as i32 + delta).rem_euclid(3) as usize,
            2 if !s.ladder => {
                s.level = (s.level as i32 + delta).clamp(0, self.levels.len() as i32 - 1) as usize
            }
            _ => {}
        }
        Ok(())
    }

    /// Returns true when the app should close.
    pub fn game_key(&mut self, k: KeyEvent) -> bool {
        let Some(g) = self.game.as_mut() else {
            return false;
        };
        if g.result.is_some() {
            if matches!(k.code, KeyCode::Enter | KeyCode::Esc) {
                self.finish_game();
            }
            return false;
        }
        if k.code == KeyCode::Enter {
            match g.input.trim() {
                "help" | "?" => {
                    g.input.clear();
                    self.modal = Some(Modal::Help);
                    return false;
                }
                "save" | "menu" => {
                    g.input.clear();
                    self.leave_game();
                    return false;
                }
                "quit" | "exit" => return true,
                _ => {}
            }
        }
        g.key(k.code);
        if matches!(k.code, KeyCode::Enter | KeyCode::Tab) {
            self.autosave();
        }
        false
    }

    pub fn leave_game(&mut self) {
        // A training game goes back to its phase; it resumes from the exercise.
        if let Some(Mode::Training(id)) = self.game.take().map(|g| g.mode) {
            self.screen = Screen::Training;
            self.training
                .show_message(&id, "Game saved: open the exercise again to resume.");
            return;
        }
        self.screen = Screen::Menu;
        self.menu.select(Some(0));
        self.msg = Some(("Game saved. Resume it any time.".into(), Tc::Green));
    }

    pub fn history_key(&mut self, code: KeyCode) {
        if matches!(
            code,
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | 'h')
        ) {
            self.screen = Screen::Menu;
        }
    }
}
