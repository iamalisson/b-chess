//! App state and input: screens, menu, new-game setup, dialogs, and the game loop.

use crate::{
    engine::Engine,
    game::Game,
    levels::{Rng, spec, step},
    store::{
        Record, SavedGame, append_history, expand_tilde, export_text, import_text, load_history,
        load_saved, plural, save_settings, today, write_history, write_saved,
    },
    theme::{ACCENT, BACKGROUNDS, BG, bg_name},
    ui::popup,
};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    style::Color as Tc,
    text::Line,
    widgets::ListState,
};
use shakmaty::Color;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

pub enum Screen {
    Menu,
    Setup,
    Game,
    History,
}

pub enum Modal {
    Help,
    Path { import: bool, input: String },
}

#[derive(Clone, Copy)]
pub enum Item {
    Resume,
    New,
    History,
    Export,
    Import,
    Background,
    Help,
    Quit,
}

/// New-game screen: rows are mode, color, level, start.
pub struct Setup {
    pub row: usize,
    pub ladder: bool,
    pub color: usize, // 0 white, 1 black, 2 random
    pub level: usize, // index into App::levels, used in free play
}

pub const COLORS: [&str; 3] = ["White", "Black", "Random"];

pub struct App {
    pub screen: Screen,
    pub modal: Option<Modal>,
    pub menu: ListState,
    pub setup: Setup,
    pub levels: Vec<u32>,
    pub weights: PathBuf,
    pub lc0: String,
    pub history: Vec<Record>,
    pub saved: Option<SavedGame>,
    pub game: Option<Game>,
    pub msg: Option<(String, Tc)>,
}

impl App {
    pub fn new(levels: Vec<u32>, weights: PathBuf, lc0: String) -> App {
        App {
            screen: Screen::Menu,
            modal: None,
            menu: ListState::default().with_selected(Some(0)),
            setup: Setup {
                row: 0,
                ladder: true,
                color: 0,
                level: 0,
            },
            levels,
            weights,
            lc0,
            history: load_history(),
            saved: load_saved(),
            game: None,
            msg: None,
        }
    }

    /// Current ladder level: where the last ladder game left you, or the bottom.
    pub fn ladder_level(&self) -> u32 {
        let last = self.history.iter().rev().find(|r| r.ladder);
        step(&self.levels, last.map_or(self.levels[0], |r| r.next), 0)
    }

    pub fn record(&self) -> [usize; 3] {
        let n = |v| self.history.iter().filter(|r| r.result == v).count();
        [n(1), n(0), n(-1)]
    }

    pub fn items(&self) -> Vec<Item> {
        let mut v = if self.saved.is_some() {
            vec![Item::Resume]
        } else {
            vec![]
        };
        v.extend([
            Item::New,
            Item::History,
            Item::Export,
            Item::Import,
            Item::Background,
            Item::Help,
        ]);
        v.push(Item::Quit);
        v
    }

    pub fn label(&self, item: Item) -> String {
        match item {
            Item::Resume => {
                let s = self.saved.as_ref().unwrap();
                let mv = s.uci.len() / 2 + 1;
                format!("Resume game  (Elo {}, move {mv})", s.level)
            }
            Item::New if self.saved.is_some() => "New game  (replaces saved game)".into(),
            Item::New => "New game".into(),
            Item::History => "History".into(),
            Item::Export => "Export progress".into(),
            Item::Import => "Import progress".into(),
            Item::Background => format!("Background: {}", bg_name(BG.load(Ordering::Relaxed))),
            Item::Help => "Help".into(),
            Item::Quit => "Quit".into(),
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;
            // The bot moves between frames, so your move is on screen while it thinks.
            if self.game.as_ref().is_some_and(|g| g.thinking) {
                self.bot_move();
                continue;
            }
            let Event::Key(k) = event::read()? else {
                continue;
            };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            let quit =
                if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                    true
                } else if self.modal.is_some() {
                    self.modal_key(k.code);
                    false
                } else if k.code == KeyCode::F(1) {
                    self.modal = Some(Modal::Help);
                    false
                } else {
                    match self.screen {
                        Screen::Menu => self.menu_key(k.code, terminal)?,
                        Screen::Setup => {
                            self.setup_key(k.code, terminal)?;
                            false
                        }
                        Screen::History => {
                            if matches!(
                                k.code,
                                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | 'h')
                            ) {
                                self.screen = Screen::Menu;
                            }
                            false
                        }
                        Screen::Game => self.game_key(k),
                    }
                };
            if quit {
                // An unconfirmed game-over popup still has to land in the history.
                if self.game.as_ref().is_some_and(|g| g.result.is_some()) {
                    self.finish_game();
                }
                return Ok(());
            }
        }
    }

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
                let game = SavedGame {
                    level,
                    me,
                    peeks: 0,
                    ladder: s.ladder,
                    uci: Vec::new(),
                };
                return self.start_game(terminal, game);
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

    pub fn modal_key(&mut self, code: KeyCode) {
        let Some(Modal::Path { import, input }) = &mut self.modal else {
            self.modal = None; // help: any key closes
            return;
        };
        match code {
            KeyCode::Esc => self.modal = None,
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c) => input.push(c),
            KeyCode::Enter => {
                let (import, path) = (*import, expand_tilde(input.trim()));
                self.modal = None;
                self.msg = Some(
                    match if import {
                        self.import(&path)
                    } else {
                        self.export(&path)
                    } {
                        Ok(m) => (m, Tc::Green),
                        Err(e) => (e, Tc::Red),
                    },
                );
            }
            _ => {}
        }
    }

    pub fn export(&self, path: &Path) -> Result<String, String> {
        fs::write(path, export_text(&self.history, self.saved.as_ref()))
            .map_err(|e| format!("Export failed: {e}"))?;
        let extra = if self.saved.is_some() {
            " + saved game"
        } else {
            ""
        };
        Ok(format!(
            "Exported {}{extra} to {}",
            plural(self.history.len(), "game"),
            path.display()
        ))
    }

    pub fn import(&mut self, path: &Path) -> Result<String, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("Import failed: {e}"))?;
        let (history, saved) = import_text(&text).map_err(|e| format!("Import failed: {e}"))?;
        write_history(&history)
            .and_then(|_| write_saved(saved.as_ref()))
            .map_err(|e| format!("Import failed while saving: {e}"))?;
        let extra = if saved.is_some() { " + saved game" } else { "" };
        let n = history.len();
        self.history = history;
        self.saved = saved;
        self.menu.select(Some(0));
        Ok(format!(
            "Imported {}{extra} from {}",
            plural(n, "game"),
            path.display()
        ))
    }

    /// Starts `saved` (a new game is just a saved game with no moves yet).
    pub fn start_game(
        &mut self,
        terminal: &mut DefaultTerminal,
        saved: SavedGame,
    ) -> io::Result<()> {
        let level = saved.level;
        let net = spec(level).weights;
        terminal.draw(|f| {
            self.draw(f);
            popup(
                f,
                40,
                3,
                Line::from(format!("Loading Lc0 ({net})...")).centered(),
                ACCENT,
            );
        })?;
        let weights = self.weights.join(format!("{net}.pb.gz"));
        let started = Engine::start(&self.lc0, &weights).and_then(|e| Game::resume(e, &saved));
        match started {
            Ok(mut game) => {
                if saved.uci.is_empty() {
                    game.msg = None;
                }
                self.msg = None;
                self.game = Some(game);
                self.screen = Screen::Game;
                self.autosave();
            }
            Err(e) => {
                if !saved.uci.is_empty() {
                    self.saved = Some(saved); // keep the saved game if resuming failed
                }
                self.screen = Screen::Menu;
                self.msg = Some((format!("Could not start Lc0 ({net}): {e}"), Tc::Red));
            }
        }
        Ok(())
    }

    pub fn autosave(&mut self) {
        let Some(g) = self.game.as_mut() else { return };
        if g.result.is_some() {
            return;
        }
        let sg = g.to_saved();
        if let Err(e) = write_saved(Some(&sg)) {
            g.msg = Some((format!("Could not save game: {e}"), Tc::Red));
        }
        self.saved = Some(sg);
    }

    pub fn bot_move(&mut self) {
        let Some(g) = self.game.as_mut() else { return };
        if let Err(e) = g.engine_move() {
            self.msg = Some((format!("Engine error: {e}. Your game is saved."), Tc::Red));
            self.game = None;
            self.screen = Screen::Menu;
            return;
        }
        self.autosave();
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
        self.game = None;
        self.screen = Screen::Menu;
        self.menu.select(Some(0));
        self.msg = Some(("Game saved. Resume it any time.".into(), Tc::Green));
    }

    pub fn finish_game(&mut self) {
        let Some(g) = self.game.take() else { return };
        let (result, _) = g.result.unwrap();
        let rec = Record {
            date: today(),
            level: g.level,
            color: g.me,
            result,
            next: if g.ladder {
                step(&self.levels, g.level, result)
            } else {
                g.level
            },
            moves: g.san.len().div_ceil(2) as u32,
            peeks: g.peeks,
            ladder: g.ladder,
        };
        self.msg = append_history(&rec)
            .and_then(|_| write_saved(None))
            .err()
            .map(|e| (format!("Could not save history: {e}"), Tc::Red));
        self.history.push(rec);
        self.saved = None;
        self.menu.select(Some(0));
        self.screen = Screen::Menu;
    }
}
