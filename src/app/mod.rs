//! App state and the event loop: screens, dialogs, starting, saving and finishing games.
//! Input for each area lives in `play` and `training`.

mod play;
mod training;

pub use training::{Activity, HOME_EXTRAS, Run, Training, View};

use crate::{
    engine::Engine,
    game::Game,
    levels::{spec, step},
    store::{
        Mode, Record, SavedGame, Slot, append_history, expand_tilde, export_text, import_text,
        load_history, load_saved, plural, today, write_history, write_saved,
    },
    theme::{ACCENT, BG, bg_name},
    training::progress::is_event_line,
    ui::popup,
};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    style::Color as Tc,
    text::Line,
    widgets::ListState,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};

pub enum Screen {
    Menu,
    Setup,
    Game,
    History,
    Training,
}

pub enum Modal {
    Help,
    Path { import: bool, input: String },
}

#[derive(Clone, Copy)]
pub enum Item {
    Resume,
    New,
    Training,
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
    pub training: Training,
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
            saved: load_saved(Slot::Play),
            game: None,
            msg: None,
            training: Training::load(),
        }
    }

    /// Current ladder level: where the last ladder game left you, or the bottom.
    pub fn ladder_level(&self) -> u32 {
        let last = self.history.iter().rev().find(|r| r.mode.is_ladder());
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
            Item::Training,
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
            Item::Training => "Training: blindfold course".into(),
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
            // A timed drill ends when its clock runs out, not on the next key press.
            self.training.tick();
            terminal.draw(|f| self.draw(f))?;
            // The bot moves between frames, so your move is on screen while it thinks.
            if self.game.as_ref().is_some_and(|g| g.thinking) {
                self.bot_move();
                continue;
            }
            // Timed drills redraw their clock even without input.
            if self.training.ticking() && !event::poll(Duration::from_millis(200))? {
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
                            self.history_key(k.code);
                            false
                        }
                        Screen::Game => self.game_key(k),
                        Screen::Training => {
                            self.training_key(k.code, terminal)?;
                            false
                        }
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
        let training = self.training.progress.lines();
        let saved: Vec<&SavedGame> = self.saved.iter().chain(&self.training.saved_game).collect();
        fs::write(path, export_text(&self.history, &saved, &training))
            .map_err(|e| format!("Export failed: {e}"))?;
        let extra = match saved.len() {
            0 => String::new(),
            n => format!(" + {}", plural(n, "saved game")),
        };
        Ok(format!(
            "Exported {}{extra} to {}",
            plural(self.history.len(), "game"),
            path.display()
        ))
    }

    pub fn import(&mut self, path: &Path) -> Result<String, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("Import failed: {e}"))?;
        let imported =
            import_text(&text, is_event_line).map_err(|e| format!("Import failed: {e}"))?;
        let (history, saved) = (imported.history, imported.saved);
        // Files without training progress (older exports) leave the course untouched.
        let has_training = !imported.training.is_empty();
        write_history(&history)
            .and_then(|_| write_saved(Slot::Play, saved.as_ref()))
            .and_then(|_| write_saved(Slot::Training, imported.training_saved.as_ref()))
            .and_then(|_| match has_training {
                true => self.training.progress.replace(&imported.training),
                false => Ok(()),
            })
            .map_err(|e| format!("Import failed while saving: {e}"))?;
        let mut extra = if saved.is_some() {
            " + saved game".to_string()
        } else {
            String::new()
        };
        if has_training {
            extra += " + training progress";
        }
        let n = history.len();
        self.history = history;
        self.saved = saved;
        self.training.saved_game = imported.training_saved;
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
        let aids = training::aids_for(&saved.mode);
        let started =
            Engine::start(&self.lc0, &weights).and_then(|e| Game::resume(e, &saved, aids));
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
                let error = format!("Could not start Lc0 ({net}): {e}");
                // A saved game stays saved; nothing was overwritten.
                match &saved.mode {
                    Mode::Training(id) => {
                        self.screen = Screen::Training;
                        self.training.show_error(id, error);
                    }
                    _ => {
                        self.screen = Screen::Menu;
                        self.msg = Some((error, Tc::Red));
                    }
                }
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
        let slot = Slot::of(&sg.mode);
        if let Err(e) = write_saved(slot, Some(&sg)) {
            g.msg = Some((format!("Could not save game: {e}"), Tc::Red));
        }
        match slot {
            Slot::Play => self.saved = Some(sg),
            Slot::Training => self.training.saved_game = Some(sg),
        }
    }

    pub fn bot_move(&mut self) {
        let Some(g) = self.game.as_mut() else { return };
        if let Err(e) = g.engine_move() {
            let error = format!("Engine error: {e}. Your game is saved.");
            match self.game.take().map(|g| g.mode) {
                Some(Mode::Training(id)) => {
                    self.screen = Screen::Training;
                    self.training.show_error(&id, error);
                }
                _ => {
                    self.screen = Screen::Menu;
                    self.msg = Some((error, Tc::Red));
                }
            }
            return;
        }
        self.autosave();
    }

    pub fn finish_game(&mut self) {
        let Some(g) = self.game.take() else { return };
        let (result, _) = g.result.clone().unwrap();
        let rec = Record {
            date: today(),
            level: g.level,
            color: g.me,
            result,
            next: if g.mode.is_ladder() {
                step(&self.levels, g.level, result)
            } else {
                g.level
            },
            moves: g.my_moves() as u32,
            peeks: g.peeks,
            mode: g.mode.clone(),
            illegal: g.illegal,
        };
        let slot = Slot::of(&g.mode);
        self.msg = append_history(&rec)
            .and_then(|_| write_saved(slot, None))
            .err()
            .map(|e| (format!("Could not save history: {e}"), Tc::Red));
        self.history.push(rec);
        match slot {
            Slot::Play => self.saved = None,
            Slot::Training => self.training.saved_game = None,
        }
        self.menu.select(Some(0));
        self.screen = Screen::Menu;
        if let Mode::Training(id) = &g.mode {
            self.training.game_finished(id, &g);
            self.screen = Screen::Training;
        }
    }
}
