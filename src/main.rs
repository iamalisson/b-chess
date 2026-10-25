mod engine;

use engine::Engine;
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Constraint, Flex, Layout, Margin, Rect},
    style::{Color as Tc, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, List, ListState, Paragraph, Row, Table, Wrap},
    DefaultTerminal, Frame,
};
use shakmaty::{
    fen::Fen, san::SanPlus, uci::UciMove, zobrist::Zobrist64, CastlingMode, Chess, Color, EnPassantMode,
    File, Move, Position, Rank, Square,
};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

const WEIGHTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/weights");
const HISTORY_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/history.tsv");
const SAVED_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/saved.tsv");
const ACCENT: Tc = Tc::Cyan;
/// Background brightness, in percent.
const DIM: u16 = 40;
const BACKGROUND: &[u8] = include_bytes!("../assets/a_blue_and_white_image_of_a_mountain.png");

const HELP: &[(&str, &str)] = &[
    ("#", "In a game"),
    ("Nf3 exd5 O-O e8=Q", "play a move (SAN)"),
    ("g1f3", "play a move (UCI)"),
    ("board  b  Tab", "show / hide the board (counted as a peek)"),
    ("help  ?  F1", "show this help"),
    ("save  menu", "back to the menu, game is kept"),
    ("quit", "save the game and close the app"),
    ("resign", "give up (counts as a loss)"),
    ("Esc Esc", "abandon (counts as a loss)"),
    ("", ""),
    ("#", "In the menu"),
    ("↑↓  ←→  Enter", "choose"),
    ("r  n  h", "resume, new game, history"),
    ("e  i", "export / import progress"),
    ("?  q", "help, quit"),
    ("", ""),
    ("#", "Modes"),
    ("Ladder", "start at the bottom; win = level up, loss = down"),
    ("Free play", "pick any level; does not move your ladder"),
    ("", ""),
    ("#", "Progress"),
    ("", "The game saves itself after every move:"),
    ("", "close the app any time and resume later."),
    ("", "Export writes one file with all your games;"),
    ("", "copy it to the other PC and Import it there."),
];

// ---------- levels ----------

/// One rung of the ladder: which network, how many nodes, how often a random move.
struct Level {
    elo: u32,
    weights: &'static str,
    nodes: u32,
    random_pct: u64,
}

const fn lv(elo: u32, weights: &'static str, nodes: u32, random_pct: u64) -> Level {
    Level { elo, weights, nodes, random_pct }
}

// Maia 1100-1900 are trained on human games at that rating and play from their raw policy
// (1 node). Below 1100 we mix random moves into maia-1100; above 1900 we add search, then
// switch to a full Leela network. Only the Maia ratings are real: the rest are rough guesses,
// not calibrated by matches (and depend on CPU speed only through time, not strength).
const LEVELS: &[Level] = &[
    lv(600, "maia-1100", 1, 50),
    lv(700, "maia-1100", 1, 40),
    lv(800, "maia-1100", 1, 30),
    lv(900, "maia-1100", 1, 20),
    lv(1000, "maia-1100", 1, 10),
    lv(1100, "maia-1100", 1, 0),
    lv(1200, "maia-1200", 1, 0),
    lv(1300, "maia-1300", 1, 0),
    lv(1400, "maia-1400", 1, 0),
    lv(1500, "maia-1500", 1, 0),
    lv(1600, "maia-1600", 1, 0),
    lv(1700, "maia-1700", 1, 0),
    lv(1800, "maia-1800", 1, 0),
    lv(1900, "maia-1900", 1, 0),
    lv(2000, "maia-1900", 16, 0),
    lv(2100, "maia-1900", 64, 0),
    lv(2200, "t1-256x10", 1, 0),
    lv(2300, "t1-256x10", 2, 0),
    lv(2400, "t1-256x10", 4, 0),
    lv(2500, "t1-256x10", 8, 0),
    lv(2600, "t1-256x10", 16, 0),
    lv(2700, "t1-256x10", 32, 0),
    lv(2800, "t1-256x10", 64, 0),
    lv(2900, "t1-256x10", 128, 0),
    lv(3000, "t1-256x10", 256, 0),
];

fn spec(elo: u32) -> &'static Level {
    LEVELS.iter().find(|l| l.elo >= elo).unwrap_or(&LEVELS[LEVELS.len() - 1])
}

fn describe(elo: u32) -> String {
    let l = spec(elo);
    match (l.random_pct, l.nodes) {
        (0, 1) => l.weights.to_string(),
        (0, n) => format!("{}, {n} nodes", l.weights),
        (r, _) => format!("{} + {r}% random", l.weights),
    }
}

/// Elo of every level whose weights file is present in `dir`, ascending.
fn levels(dir: &Path) -> Vec<u32> {
    LEVELS
        .iter()
        .filter(|l| dir.join(format!("{}.pb.gz", l.weights)).exists())
        .map(|l| l.elo)
        .collect()
}

/// Move `delta` steps from `cur` through `levels`, clamped to the ends.
fn step(levels: &[u32], cur: u32, delta: i32) -> u32 {
    let last = levels.len() as i32 - 1;
    let i = levels.iter().position(|&l| l >= cur).map_or(last, |i| i as i32);
    levels[(i + delta).clamp(0, last) as usize]
}

/// Tiny xorshift: enough for random moves and a random color, no dependency needed.
struct Rng(u64);

impl Rng {
    fn new() -> Rng {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Rng(t | 1)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

// ---------- history & saved game ----------

fn color_code(c: Color) -> &'static str {
    if c == Color::White { "w" } else { "b" }
}

fn color_name(c: Color) -> &'static str {
    if c == Color::White { "white" } else { "black" }
}

fn parse_color(s: &str) -> Color {
    if s == "w" { Color::White } else { Color::Black }
}

/// One finished game. Stored as a tab-separated line in data/history.tsv.
struct Record {
    date: String,
    level: u32,
    color: Color,
    result: i32, // 1 win, 0 draw, -1 loss
    next: u32,
    moves: u32,
    peeks: u32,
    ladder: bool,
}

impl Record {
    fn parse(line: &str) -> Option<Record> {
        let f: Vec<&str> = line.split('\t').collect();
        let [date, level, color, result, next, moves, peeks, mode] = f[..] else { return None };
        Some(Record {
            date: date.to_string(),
            level: level.parse().ok()?,
            color: parse_color(color),
            result: result.parse().ok()?,
            next: next.parse().ok()?,
            moves: moves.parse().ok()?,
            peeks: peeks.parse().ok()?,
            ladder: mode == "ladder",
        })
    }

    fn to_line(&self) -> String {
        let c = color_code(self.color);
        let mode = if self.ladder { "ladder" } else { "free" };
        let Record { date, level, result, next, moves, peeks, .. } = self;
        format!("{date}\t{level}\t{c}\t{result}\t{next}\t{moves}\t{peeks}\t{mode}")
    }
}

/// An unfinished game: "saved<TAB>level<TAB>w|b<TAB>peeks<TAB>ladder|free<TAB>uci moves".
struct SavedGame {
    level: u32,
    me: Color,
    peeks: u32,
    ladder: bool,
    uci: Vec<String>,
}

impl SavedGame {
    fn parse(line: &str) -> Option<SavedGame> {
        let f: Vec<&str> = line.split('\t').collect();
        let ["saved", level, color, peeks, mode, moves] = f[..] else { return None };
        Some(SavedGame {
            level: level.parse().ok()?,
            me: parse_color(color),
            peeks: peeks.parse().ok()?,
            ladder: mode == "ladder",
            uci: moves.split_whitespace().map(String::from).collect(),
        })
    }

    fn to_line(&self) -> String {
        let c = color_code(self.me);
        let mode = if self.ladder { "ladder" } else { "free" };
        format!("saved\t{}\t{c}\t{}\t{mode}\t{}", self.level, self.peeks, self.uci.join(" "))
    }
}

fn load_history() -> Vec<Record> {
    fs::read_to_string(HISTORY_FILE)
        .unwrap_or_default()
        .lines()
        .filter_map(Record::parse)
        .collect()
}

fn append_history(r: &Record) -> io::Result<()> {
    let path = Path::new(HISTORY_FILE);
    fs::create_dir_all(path.parent().unwrap())?;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", r.to_line())
}

fn write_history(all: &[Record]) -> io::Result<()> {
    let path = Path::new(HISTORY_FILE);
    fs::create_dir_all(path.parent().unwrap())?;
    let text: String = all.iter().map(|r| r.to_line() + "\n").collect();
    fs::write(path, text)
}

fn load_saved() -> Option<SavedGame> {
    SavedGame::parse(fs::read_to_string(SAVED_FILE).ok()?.trim_end_matches('\n'))
}

fn write_saved(saved: Option<&SavedGame>) -> io::Result<()> {
    let path = Path::new(SAVED_FILE);
    match saved {
        Some(s) => {
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, s.to_line() + "\n")
        }
        None => match fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

/// Export file: a header comment, one history line per game, then the saved game if any.
fn export_text(history: &[Record], saved: Option<&SavedGame>) -> String {
    let mut s = String::from("# b-chess progress\n");
    for r in history {
        s += &(r.to_line() + "\n");
    }
    if let Some(sg) = saved {
        s += &(sg.to_line() + "\n");
    }
    s
}

/// Parses an export file; any unrecognized line rejects the whole file.
fn import_text(text: &str) -> Result<(Vec<Record>, Option<SavedGame>), String> {
    let mut history = Vec::new();
    let mut saved = None;
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(r) = Record::parse(line) {
            history.push(r);
        } else if let Some(sg) = SavedGame::parse(line) {
            saved = Some(sg);
        } else {
            return Err(format!("line {} is not b-chess progress", i + 1));
        }
    }
    Ok((history, saved))
}

fn expand_tilde(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), env::var("HOME")) {
        (Some(rest), Ok(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(p),
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// Days since 1970-01-01 to YYYY-MM-DD (Howard Hinnant's civil_from_days).
fn civil(days: i64) -> String {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

// UTC date: late evenings west of Greenwich get tomorrow's date. Add chrono if that matters.
fn today() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    civil((secs / 86400) as i64)
}

// ---------- game ----------

fn parse_move(pos: &Chess, s: &str) -> Option<Move> {
    if let Ok(san) = SanPlus::from_ascii(s.as_bytes()) {
        if let Ok(m) = san.san.to_move(pos) {
            return Some(m);
        }
    }
    UciMove::from_ascii(s.as_bytes()).ok()?.to_move(pos).ok()
}

struct Game {
    engine: Engine,
    level: u32,
    ladder: bool,
    me: Color,
    pos: Chess,
    uci: Vec<String>,
    san: Vec<String>,
    seen: Vec<Zobrist64>,
    last: Option<Move>,
    input: String,
    msg: Option<(String, Tc)>,
    peek: bool,
    peeks: u32,
    confirm_abandon: bool,
    thinking: bool,
    rng: Rng,
    result: Option<(i32, String)>,
}

impl Game {
    fn new(engine: Engine, level: u32, me: Color, ladder: bool) -> Game {
        let pos = Chess::default();
        let seen = vec![pos.zobrist_hash(EnPassantMode::Legal)];
        Game {
            engine,
            level,
            ladder,
            me,
            thinking: pos.turn() != me,
            pos,
            uci: Vec::new(),
            san: Vec::new(),
            seen,
            last: None,
            input: String::new(),
            msg: None,
            peek: false,
            peeks: 0,
            confirm_abandon: false,
            rng: Rng::new(),
            result: None,
        }
    }

    fn resume(engine: Engine, saved: &SavedGame) -> io::Result<Game> {
        let mut game = Game::new(engine, saved.level, saved.me, saved.ladder);
        game.peeks = saved.peeks;
        for u in &saved.uci {
            let m = UciMove::from_ascii(u.as_bytes())
                .ok()
                .and_then(|m| m.to_move(&game.pos).ok())
                .ok_or_else(|| io::Error::other(format!("saved game has a bad move: {u}")))?;
            game.apply(m);
        }
        game.thinking = game.result.is_none() && game.pos.turn() != game.me;
        game.msg = Some(("Game resumed. Type help for commands.".into(), Tc::Green));
        Ok(game)
    }

    fn to_saved(&self) -> SavedGame {
        SavedGame {
            level: self.level,
            me: self.me,
            peeks: self.peeks,
            ladder: self.ladder,
            uci: self.uci.clone(),
        }
    }

    fn apply(&mut self, m: Move) {
        self.uci.push(m.to_uci(CastlingMode::Standard).to_string());
        self.san.push(SanPlus::from_move_and_play_unchecked(&mut self.pos, m).to_string());
        self.seen.push(self.pos.zobrist_hash(EnPassantMode::Legal));
        self.last = Some(m);
        self.check_end();
    }

    fn check_end(&mut self) {
        let draw = |why: &str| Some((0, why.to_string()));
        self.result = if let Some(outcome) = self.pos.outcome().known() {
            match outcome.winner() {
                Some(c) => Some((if c == self.me { 1 } else { -1 }, "Checkmate".into())),
                None if self.pos.is_stalemate() => draw("Stalemate"),
                None => draw("Insufficient material"),
            }
        } else if self.pos.halfmoves() >= 100 {
            draw("50-move rule")
        } else if self.seen.iter().filter(|&&h| Some(&h) == self.seen.last()).count() >= 3 {
            draw("Threefold repetition")
        } else {
            None
        };
    }

    fn engine_move(&mut self) -> io::Result<()> {
        self.thinking = false;
        if self.result.is_some() || self.pos.turn() == self.me {
            return Ok(());
        }
        let level = spec(self.level);
        let m = if self.rng.below(100) < level.random_pct {
            let legal = self.pos.legal_moves();
            legal[self.rng.below(legal.len() as u64) as usize]
        } else {
            let best = self.engine.best_move(&self.uci, level.nodes)?;
            UciMove::from_ascii(best.as_bytes())
                .ok()
                .and_then(|u| u.to_move(&self.pos).ok())
                .ok_or_else(|| io::Error::other(format!("lc0 sent bad move: {best}")))?
        };
        self.apply(m);
        Ok(())
    }

    fn toggle_board(&mut self) {
        self.peek = !self.peek;
        self.peeks += self.peek as u32;
    }

    fn submit(&mut self) {
        let text = std::mem::take(&mut self.input);
        match text.trim() {
            "" => {}
            "resign" => self.result = Some((-1, "Resigned".into())),
            "board" | "b" => self.toggle_board(),
            s => match parse_move(&self.pos, s) {
                Some(m) => {
                    self.msg = None;
                    self.peek = false;
                    self.apply(m);
                    self.thinking = self.result.is_none();
                }
                None => {
                    self.msg = Some((format!("Illegal or unknown move: {s}  (type help)"), Tc::Red))
                }
            },
        }
    }

    fn key(&mut self, code: KeyCode) {
        let abandon_pressed = code == KeyCode::Esc;
        match code {
            KeyCode::Esc if self.confirm_abandon => self.result = Some((-1, "Abandoned".into())),
            KeyCode::Esc => {
                self.msg = Some(("Press Esc again to abandon (counts as a loss)".into(), Tc::Yellow))
            }
            KeyCode::Tab => self.toggle_board(),
            KeyCode::Enter => self.submit(),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) if self.input.len() < 12 => self.input.push(c),
            _ => {}
        }
        self.confirm_abandon = abandon_pressed && !self.confirm_abandon;
    }

    /// "12... Nf6" for ply index `i`.
    fn numbered(&self, i: usize) -> String {
        let dots = if i % 2 == 0 { "." } else { "..." };
        format!("{}{dots} {}", i / 2 + 1, self.san[i])
    }
}

// ---------- app ----------

enum Screen {
    Menu,
    Setup,
    Game,
    History,
}

enum Modal {
    Help,
    Path { import: bool, input: String },
}

#[derive(Clone, Copy)]
enum Item {
    Resume,
    New,
    History,
    Export,
    Import,
    Help,
    Quit,
}

/// New-game screen: rows are mode, color, level, start.
struct Setup {
    row: usize,
    ladder: bool,
    color: usize, // 0 white, 1 black, 2 random
    level: usize, // index into App::levels, used in free play
}

const COLORS: [&str; 3] = ["White", "Black", "Random"];

struct App {
    screen: Screen,
    modal: Option<Modal>,
    menu: ListState,
    setup: Setup,
    levels: Vec<u32>,
    weights: PathBuf,
    lc0: String,
    history: Vec<Record>,
    saved: Option<SavedGame>,
    game: Option<Game>,
    msg: Option<(String, Tc)>,
    background: Option<DynamicImage>,
    bg_cache: Option<(u16, u16, Vec<(Tc, Tc)>)>,
    /// Terminal speaks the kitty graphics protocol: show the image at full resolution.
    kitty: bool,
    placed: Option<(u16, u16)>,
}

impl App {
    /// Current ladder level: where the last ladder game left you, or the bottom.
    fn ladder_level(&self) -> u32 {
        let last = self.history.iter().rev().find(|r| r.ladder);
        step(&self.levels, last.map_or(self.levels[0], |r| r.next), 0)
    }

    fn record(&self) -> [usize; 3] {
        let n = |v| self.history.iter().filter(|r| r.result == v).count();
        [n(1), n(0), n(-1)]
    }

    fn items(&self) -> Vec<Item> {
        let mut v = if self.saved.is_some() { vec![Item::Resume] } else { vec![] };
        v.extend([Item::New, Item::History, Item::Export, Item::Import, Item::Help, Item::Quit]);
        v
    }

    fn label(&self, item: Item) -> String {
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
            Item::Help => "Help".into(),
            Item::Quit => "Quit".into(),
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        loop {
            if self.kitty {
                // Resize (and its screen clear) first, then put the image under the new frame.
                terminal.autoresize()?;
                let size = terminal.size()?;
                self.place_image(size.width, size.height)?;
            }
            terminal.draw(|f| self.draw(f))?;
            // The bot moves between frames, so your move is on screen while it thinks.
            if self.game.as_ref().is_some_and(|g| g.thinking) {
                self.bot_move();
                continue;
            }
            let Event::Key(k) = event::read()? else { continue };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            let quit = if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
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
                        if matches!(k.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | 'h')) {
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


    fn menu_key(&mut self, code: KeyCode, terminal: &mut DefaultTerminal) -> io::Result<bool> {
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
                self.modal = Some(Modal::Path { import: matches!(item, Item::Import), input });
            }
            Item::Help => self.modal = Some(Modal::Help),
            Item::Quit => return Ok(true),
        }
        Ok(false)
    }

    fn setup_key(&mut self, code: KeyCode, terminal: &mut DefaultTerminal) -> io::Result<()> {
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
                let level = if s.ladder { ladder_level } else { self.levels[s.level] };
                let game = SavedGame { level, me, peeks: 0, ladder: s.ladder, uci: Vec::new() };
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

    fn modal_key(&mut self, code: KeyCode) {
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
                self.msg = Some(match if import { self.import(&path) } else { self.export(&path) } {
                    Ok(m) => (m, Tc::Green),
                    Err(e) => (e, Tc::Red),
                });
            }
            _ => {}
        }
    }

    fn export(&self, path: &Path) -> Result<String, String> {
        fs::write(path, export_text(&self.history, self.saved.as_ref()))
            .map_err(|e| format!("Export failed: {e}"))?;
        let extra = if self.saved.is_some() { " + saved game" } else { "" };
        Ok(format!("Exported {}{extra} to {}", plural(self.history.len(), "game"), path.display()))
    }

    fn import(&mut self, path: &Path) -> Result<String, String> {
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
        Ok(format!("Imported {}{extra} from {}", plural(n, "game"), path.display()))
    }

    /// Starts `saved` (a new game is just a saved game with no moves yet).
    fn start_game(&mut self, terminal: &mut DefaultTerminal, saved: SavedGame) -> io::Result<()> {
        let level = saved.level;
        let net = spec(level).weights;
        terminal.draw(|f| {
            self.draw(f);
            popup(f, 40, 3, Line::from(format!("Loading Lc0 ({net})...")).centered(), ACCENT);
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

    fn autosave(&mut self) {
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

    fn bot_move(&mut self) {
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
    fn game_key(&mut self, k: KeyEvent) -> bool {
        let Some(g) = self.game.as_mut() else { return false };
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

    fn leave_game(&mut self) {
        self.game = None;
        self.screen = Screen::Menu;
        self.menu.select(Some(0));
        self.msg = Some(("Game saved. Resume it any time.".into(), Tc::Green));
    }

    fn finish_game(&mut self) {
        let Some(g) = self.game.take() else { return };
        let (result, _) = g.result.unwrap();
        let rec = Record {
            date: today(),
            level: g.level,
            color: g.me,
            result,
            next: if g.ladder { step(&self.levels, g.level, result) } else { g.level },
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

    // ---------- drawing ----------

    fn draw(&mut self, f: &mut Frame) {
        self.draw_background(f);
        match self.screen {
            Screen::Menu => self.draw_menu(f),
            Screen::Setup => self.draw_setup(f),
            Screen::Game => self.draw_game(f),
            Screen::History => self.draw_history(f),
        }
        match &self.modal {
            Some(Modal::Help) => draw_help(f),
            Some(Modal::Path { import, input }) => draw_path(f, *import, input),
            None => {}
        }
        self.ink(f);
    }

    /// After the widgets are drawn: blank cells show the image again, and text without its
    /// own colors is drawn in the terminal's text color straight on the image.
    fn ink(&self, f: &mut Frame) {
        if self.kitty {
            return;
        }
        let Some((w, h, cells)) = &self.bg_cache else { return };
        let area = f.area();
        if (area.width, area.height) != (*w, *h) {
            return;
        }
        let buf = f.buffer_mut();
        for y in 0..*h {
            for x in 0..*w {
                let (top, bottom) = cells[(y * w + x) as usize];
                let cell = &mut buf[(area.x + x, area.y + y)];
                let untouched_bg = cell.bg == bottom || cell.bg == Tc::Reset;
                if cell.symbol() == " " && untouched_bg {
                    cell.set_char('▀').set_fg(top).set_bg(bottom);
                } else if cell.symbol() != "▀" {
                    if cell.fg == top {
                        cell.fg = Tc::Reset;
                    }
                    if untouched_bg {
                        cell.bg = blend(top, bottom);
                    }
                }
            }
        }
    }

    /// Sends the image to a kitty-protocol terminal, sized to the whole window and placed
    /// under the text (z below -2^30 also keeps it under cells with their own background,
    /// like the board squares). Re-sent only when the window size changes.
    fn place_image(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        if self.placed == Some((cols, rows)) {
            return Ok(());
        }
        let Some(img) = &self.background else { return Ok(()) };
        let (pw, ph) = match ratatui::crossterm::terminal::window_size() {
            Ok(s) if s.width > 0 && s.height > 0 => (s.width as u32, s.height as u32),
            _ => (cols as u32 * 10, rows as u32 * 20),
        };
        let mut px = img.resize_to_fill(pw, ph, FilterType::Lanczos3).to_rgb8();
        for c in px.iter_mut() {
            *c = (*c as u16 * DIM / 100) as u8;
        }
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(px)
            .write_to(&mut io::Cursor::new(&mut png), ImageFormat::Png)
            .map_err(io::Error::other)?;
        let data = base64(&png);
        let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
        let mut out = io::stdout().lock();
        write!(out, "\x1b7\x1b[H")?; // save cursor, go to top-left
        for (i, chunk) in chunks.iter().enumerate() {
            let more = (i + 1 < chunks.len()) as u8;
            if i == 0 {
                write!(out, "\x1b_Ga=T,f=100,i=1,p=1,q=2,C=1,c={cols},r={rows},z=-1073741825,m={more};")?;
            } else {
                write!(out, "\x1b_Gm={more};")?;
            }
            out.write_all(chunk)?;
            write!(out, "\x1b\\")?;
        }
        write!(out, "\x1b8")?; // restore cursor
        out.flush()?;
        self.placed = Some((cols, rows));
        Ok(())
    }

    /// The background image as half blocks: each cell shows two pixels (fg on top, bg below).
    fn draw_background(&mut self, f: &mut Frame) {
        if self.kitty {
            return;
        }
        let Some(img) = &self.background else { return };
        let area = f.area();
        let (w, h) = (area.width, area.height);
        if self.bg_cache.as_ref().is_none_or(|c| (c.0, c.1) != (w, h)) {
            let px = img.resize_to_fill(w as u32, h as u32 * 2, FilterType::Triangle).to_rgb8();
            let dim = |c: u8| (c as u16 * DIM / 100) as u8;
            let rgb = |x: u32, y: u32| {
                let [r, g, b] = px.get_pixel(x, y).0;
                Tc::Rgb(dim(r), dim(g), dim(b))
            };
            let cells = (0..h as u32)
                .flat_map(|y| (0..w as u32).map(move |x| (x, y)))
                .map(|(x, y)| (rgb(x, 2 * y), rgb(x, 2 * y + 1)))
                .collect();
            self.bg_cache = Some((w, h, cells));
        }
        let cells = &self.bg_cache.as_ref().unwrap().2;
        let buf = f.buffer_mut();
        for y in 0..h {
            for x in 0..w {
                let (top, bottom) = cells[(y * w + x) as usize];
                buf[(area.x + x, area.y + y)].set_char('▀').set_fg(top).set_bg(bottom);
            }
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
            Line::from("♞  Blindfold Trainer").bold().fg(ACCENT).centered(),
        ]);
        f.render_widget(heading, title);

        let [w, d, l] = self.record();
        let row = |k: &str, v: String| {
            Line::from(vec![Span::raw(format!("{k:<16}")).dim(), Span::raw(v).bold()])
        };
        let ladder = self.ladder_level();
        let stats_text = vec![
            row("Ladder level", format!("Elo {ladder}  ({})", describe(ladder))),
            row("Record", format!("{w}W  {d}D  {l}L")),
        ];
        f.render_widget(Paragraph::new(stats_text), stats);

        let labels: Vec<String> = items.iter().map(|&i| self.label(i)).collect();
        let list_widget = List::new(labels)
            .highlight_symbol("▶ ")
            .highlight_style(Style::new().fg(ACCENT).bold());
        f.render_stateful_widget(list_widget, list, &mut self.menu);

        if let Some((m, c)) = &self.msg {
            f.render_widget(Paragraph::new(m.as_str()).fg(*c).wrap(Wrap { trim: true }), msg);
        }
        f.render_widget(Line::from("↑↓ select · Enter open · ? help · q quit").dim().centered(), help);
    }

    fn draw_setup(&self, f: &mut Frame) {
        let s = &self.setup;
        let area = center(f.area(), 60, 15);
        solid(f, area);
        let block = rounded(" New game ")
            .border_style(Style::new().fg(ACCENT))
            .title_bottom(Line::from(" ↑↓ row · ←→ change · Enter start · Esc back ").dim().centered());
        let inner = block.inner(area).inner(Margin::new(2, 1));
        f.render_widget(block, area);

        let level = if s.ladder { self.ladder_level() } else { self.levels[s.level] };
        let level_value = if s.ladder {
            format!("Elo {level}  (your ladder)")
        } else {
            format!("◀ Elo {level} ▶")
        };
        let rows = [
            ("Mode", format!("◀ {} ▶", if s.ladder { "Ladder" } else { "Free play" })),
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
            Span::raw(format!("peeks: {}", g.peeks)).fg(if g.peeks == 0 { Tc::Green } else { Tc::Yellow }),
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
                    spans.push(if i * 2 + j == g.san.len() - 1 { span.fg(Tc::Yellow).bold() } else { span });
                }
                Line::from(spans)
            })
            .collect();
        let visible = moves_area.height.saturating_sub(2) as usize;
        let scroll = lines.len().saturating_sub(visible) as u16;
        f.render_widget(Paragraph::new(lines).scroll((scroll, 0)).block(rounded(" Moves ")), moves_area);

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
                let who = if (i % 2 == 0) == (g.me == Color::White) { "You" } else { "Lc0" };
                let line = Line::from(vec![
                    Span::raw(format!("{who}  ")).dim(),
                    Span::raw(g.numbered(i)).bold(),
                ]);
                text.push(if i == n - 1 { line.fg(Tc::Yellow) } else { line });
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
        let line = Line::from(vec![Span::raw(prompt).fg(ACCENT), Span::raw(&g.input).bold()]);
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
                format!("Ladder: Elo {} → {}", g.level, step(&self.levels, g.level, *result))
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
        let best = self.history.iter().filter(|r| r.result == 1).map(|r| r.level).max();
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
            Line::from(format!(" {} · {w}W {d}D {l}L{best}", plural(self.history.len(), "game"))),
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
                if r.ladder { format!("→ {}", r.next) } else { "-".into() },
            ])
            .fg(c)
        });
        let widths = [11, 7, 9, 6, 6, 6, 6, 7].map(Constraint::Length);
        let t = Table::new(rows, widths)
            .header(
                Row::new(["Date", "Mode", "Lc0", "Color", "Result", "Moves", "Peeks", "Ladder"])
                    .bold()
                    .fg(ACCENT),
            )
            .column_spacing(2);
        f.render_widget(t, table);
    }
}

// ---------- widgets ----------

/// Standard base64, for the kitty graphics protocol.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let byte = |i: usize| *c.get(i).unwrap_or(&0) as u32;
        let n = byte(0) << 16 | byte(1) << 8 | byte(2);
        for i in 0..4 {
            s.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

/// kitty, Ghostty and WezTerm draw images; tmux would swallow the escape codes.
fn kitty_graphics() -> bool {
    let var = |k: &str| env::var(k).unwrap_or_default();
    env::var("TMUX").is_err()
        && (var("TERM").contains("kitty")
            || env::var("KITTY_WINDOW_ID").is_ok()
            || matches!(var("TERM_PROGRAM").as_str(), "ghostty" | "WezTerm"))
}

/// Average of two colors: the image behind a text cell, which can only show one color.
fn blend(a: Tc, b: Tc) -> Tc {
    match (a, b) {
        (Tc::Rgb(r1, g1, b1), Tc::Rgb(r2, g2, b2)) => {
            let avg = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;
            Tc::Rgb(avg(r1, r2), avg(g1, g2), avg(b1, b2))
        }
        _ => a,
    }
}

/// Empties `area`; `App::ink` paints the background image back into the blank cells.
fn solid(f: &mut Frame, area: Rect) {
    f.render_widget(Clear, area);
}

fn rounded(title: &str) -> Block<'_> {
    Block::bordered().border_type(BorderType::Rounded).title(title)
}

fn center(area: Rect, w: u16, h: u16) -> Rect {
    let [a] = Layout::horizontal([Constraint::Length(w)]).flex(Flex::Center).areas(area);
    let [a] = Layout::vertical([Constraint::Length(h)]).flex(Flex::Center).areas(a);
    a
}

fn popup<'a>(f: &mut Frame, w: u16, h: u16, text: impl Into<Text<'a>>, color: Tc) {
    let area = center(f.area(), w, h);
    solid(f, area);
    let block = rounded("").border_style(Style::new().fg(color));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let text = text.into();
    let th = text.height() as u16;
    f.render_widget(Paragraph::new(text), center(inner, inner.width, th));
}

fn draw_help(f: &mut Frame) {
    let lines: Vec<Line> = HELP
        .iter()
        .map(|&(keys, what)| match keys {
            "#" => Line::from(what).bold().fg(ACCENT),
            "" => Line::from(format!("  {what}")),
            _ => Line::from(vec![Span::raw(format!("  {keys:<20}")).bold(), Span::raw(what).dim()]),
        })
        .collect();
    let area = center(f.area(), 72, lines.len() as u16 + 4);
    solid(f, area);
    let block = rounded(" Help ")
        .border_style(Style::new().fg(ACCENT))
        .title_bottom(Line::from(" any key to close ").dim().centered());
    let inner = block.inner(area).inner(Margin::new(1, 1));
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_path(f: &mut Frame, import: bool, input: &str) {
    let area = center(f.area(), 64, 8);
    solid(f, area);
    let (title, note) = if import {
        (" Import progress from ", "Replaces the games and saved game on this PC.")
    } else {
        (" Export progress to ", "Copy this file to the other PC, then Import it there.")
    };
    let block = rounded(title)
        .border_style(Style::new().fg(ACCENT))
        .title_bottom(Line::from(" Enter confirm · Esc cancel ").dim().centered());
    let inner = block.inner(area).inner(Margin::new(1, 1));
    f.render_widget(block, area);
    let text = vec![
        Line::from(vec![Span::raw("▶ ").fg(ACCENT), Span::raw(input).bold()]),
        Line::from(""),
        Line::from(note).fg(if import { Tc::Yellow } else { Tc::Reset }).dim(),
    ];
    f.render_widget(Paragraph::new(text), inner);
    f.set_cursor_position((inner.x + 2 + input.chars().count() as u16, inner.y));
}

/// gnuchess-style board: letters (white uppercase), dots for empty squares, last move in
/// yellow. `spaced` puts a blank line between ranks for a bigger board.
fn board_lines(pos: &Chess, me: Color, last: Option<Move>, spaced: bool) -> Vec<Line<'static>> {
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
    let turn = if pos.turn() == Color::White { "white" } else { "black" };
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
            spans.push(if marked(sq) { span.fg(Tc::Yellow).bold() } else { span });
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    let labels: String = files.iter().map(|f| format!("  {} ", f.char())).collect();
    lines.push(Line::from(format!("   {labels}")).dim());
    lines
}

fn main() -> io::Result<()> {
    let weights = PathBuf::from(env::var("MAIA_DIR").unwrap_or_else(|_| WEIGHTS_DIR.into()));
    let levels = levels(&weights);
    if levels.is_empty() {
        eprintln!("No weights in {} (run scripts/fetch-weights.sh)", weights.display());
        process::exit(1);
    }
    let mut app = App {
        screen: Screen::Menu,
        modal: None,
        menu: ListState::default().with_selected(Some(0)),
        setup: Setup { row: 0, ladder: true, color: 0, level: 0 },
        levels,
        weights,
        lc0: env::var("LC0").unwrap_or_else(|_| "lc0".into()),
        history: load_history(),
        saved: load_saved(),
        game: None,
        msg: None,
        background: image::load_from_memory(BACKGROUND).ok(),
        bg_cache: None,
        kitty: kitty_graphics(),
        placed: None,
    };
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal);
    if app.kitty {
        print!("\x1b_Ga=d,d=A,q=2\x1b\\"); // delete our image
    }
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_moves_and_clamps() {
        let l = [1100, 1200, 1300];
        assert_eq!(step(&l, 1100, 1), 1200);
        assert_eq!(step(&l, 1300, 1), 1300);
        assert_eq!(step(&l, 1100, -1), 1100);
        assert_eq!(step(&l, 1250, 0), 1300);
        assert_eq!(step(&l, 9999, 0), 1300);
    }

    #[test]
    fn levels_span_600_to_3000_in_100s() {
        let elos: Vec<u32> = LEVELS.iter().map(|l| l.elo).collect();
        assert_eq!(elos, (6..=30).map(|n| n * 100).collect::<Vec<_>>());
        assert_eq!(spec(1500).weights, "maia-1500");
        assert_eq!(describe(800), "maia-1100 + 30% random");
        assert_eq!(describe(2400), "t1-256x10, 4 nodes");
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b""), "");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), "1970-01-01");
        assert_eq!(civil(20000), "2024-10-04");
    }

    #[test]
    fn record_round_trips() {
        let line = "2026-10-24\t1300\tb\t-1\t1200\t34\t2\tladder";
        assert_eq!(Record::parse(line).unwrap().to_line(), line);
        assert!(Record::parse("garbage").is_none());
    }

    #[test]
    fn export_import_round_trips() {
        let history = vec![Record::parse("2026-10-24\t1300\tb\t-1\t1200\t34\t2\tfree").unwrap()];
        let saved = SavedGame {
            level: 1200,
            me: Color::White,
            peeks: 1,
            ladder: true,
            uci: vec!["e2e4".into(), "e7e5".into()],
        };
        let text = export_text(&history, Some(&saved));
        let (h, s) = import_text(&text).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].to_line(), history[0].to_line());
        assert_eq!(s.unwrap().to_line(), saved.to_line());
        assert!(import_text("hello\n").is_err());
        let fresh = SavedGame { level: 1100, me: Color::Black, peeks: 0, ladder: false, uci: vec![] };
        assert_eq!(SavedGame::parse(&fresh.to_line()).unwrap().uci.len(), 0);
    }
}
