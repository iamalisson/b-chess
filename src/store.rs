//! Everything on disk: where files live, network download, finished games, the game in
//! progress, settings, and export/import of progress.

use crate::theme::{BG, bg_name, parse_bg};
use shakmaty::Color;
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    sync::{OnceLock, atomic::Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAIA_URL: &str = "https://github.com/CSSLab/maia-chess/releases/download/v1.0";
pub const T1_URL: &str =
    "https://storage.lczero.org/files/networks-contrib/t1-256x10-distilled-swa-2432500.pb.gz";

/// Where weights/ and data/ live: $B_CHESS_HOME, else this source checkout when running
/// from it, else ~/.local/share/b-chess. Packaged builds (Nix, releases) set
/// B_CHESS_PACKAGED at compile time so a stray copy of their build path is never used.
pub fn home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        if let Ok(h) = env::var("B_CHESS_HOME") {
            return h.into();
        }
        let src = Path::new(env!("CARGO_MANIFEST_DIR"));
        if option_env!("B_CHESS_PACKAGED").is_none() && src.join("Cargo.toml").exists() {
            return src.into();
        }
        let data = env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                Path::new(&env::var("HOME").unwrap_or_default()).join(".local/share")
            });
        data.join("b-chess")
    })
}

pub fn history_file() -> PathBuf {
    home().join("data/history.tsv")
}

/// data/settings.tsv: one "key<TAB>value" per line; only "background" so far.
pub fn settings_file() -> PathBuf {
    home().join("data/settings.tsv")
}

pub fn load_settings() {
    let text = fs::read_to_string(settings_file()).unwrap_or_default();
    for line in text.lines() {
        if let Some(c) = line.strip_prefix("background\t").and_then(parse_bg) {
            BG.store(c, Ordering::Relaxed);
        }
    }
}

pub fn save_settings() -> io::Result<()> {
    let path = settings_file();
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(
        path,
        format!("background\t{}\n", bg_name(BG.load(Ordering::Relaxed))),
    )
}

/// Where an unfinished game is kept: play games and training games have separate slots,
/// so starting a training exercise never replaces a saved ladder or free game.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Slot {
    Play,
    Training,
}

impl Slot {
    pub fn of(mode: &Mode) -> Slot {
        match mode {
            Mode::Training(_) => Slot::Training,
            _ => Slot::Play,
        }
    }

    fn file(self) -> PathBuf {
        home().join(match self {
            Slot::Play => "data/saved.tsv",
            Slot::Training => "data/training_saved.tsv",
        })
    }
}

/// Downloads the networks missing from `dir` with curl (Maia 1100-1900 + Leela T1, ~50 MB).
pub fn fetch_networks(dir: &Path) -> io::Result<()> {
    let mut nets: Vec<(String, String)> = (11..=19)
        .map(|n| {
            (
                format!("maia-{n}00.pb.gz"),
                format!("{MAIA_URL}/maia-{n}00.pb.gz"),
            )
        })
        .collect();
    nets.push(("t1-256x10.pb.gz".into(), T1_URL.into()));
    fs::create_dir_all(dir)?;
    for (name, url) in nets {
        let path = dir.join(&name);
        if path.exists() {
            continue;
        }
        println!("Downloading {name} into {}", dir.display());
        let part = dir.join(format!("{name}.part"));
        let ok = process::Command::new("curl")
            .args(["-fL", "--progress-bar", "-o"])
            .arg(&part)
            .arg(&url)
            .status()?
            .success();
        if !ok {
            return Err(io::Error::other(format!("download failed: {url}")));
        }
        fs::rename(part, path)?;
    }
    Ok(())
}

pub fn color_code(c: Color) -> &'static str {
    if c == Color::White { "w" } else { "b" }
}

pub fn color_name(c: Color) -> &'static str {
    if c == Color::White { "white" } else { "black" }
}

pub fn parse_color(s: &str) -> Color {
    if s == "w" { Color::White } else { Color::Black }
}

/// What a game counts for: the ladder, nothing (free play), or a training exercise.
#[derive(Clone, PartialEq, Debug)]
pub enum Mode {
    Ladder,
    Free,
    Training(String),
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s {
            "ladder" => Mode::Ladder,
            "free" => Mode::Free,
            _ => s
                .strip_prefix("training:")
                .map_or(Mode::Free, |id| Mode::Training(id.to_string())),
        }
    }

    pub fn code(&self) -> String {
        match self {
            Mode::Ladder => "ladder".into(),
            Mode::Free => "free".into(),
            Mode::Training(id) => format!("training:{id}"),
        }
    }

    pub fn is_ladder(&self) -> bool {
        *self == Mode::Ladder
    }

    pub fn label(&self) -> &'static str {
        match self {
            Mode::Ladder => "ladder",
            Mode::Free => "free play",
            Mode::Training(_) => "training",
        }
    }
}

/// One finished game. Stored as a tab-separated line in data/history.tsv.
pub struct Record {
    pub date: String,
    pub level: u32,
    pub color: Color,
    pub result: i32, // 1 win, 0 draw, -1 loss
    pub next: u32,
    pub moves: u32,
    pub peeks: u32,
    pub mode: Mode,
    pub illegal: u32,
}

impl Record {
    /// Reads 9 fields, or the 8 of lines written before illegal moves were counted.
    pub fn parse(line: &str) -> Option<Record> {
        let f: Vec<&str> = line.split('\t').collect();
        let (fields, illegal) = match f.len() {
            8 => (&f[..], 0),
            9 => (&f[..8], f[8].parse().ok()?),
            _ => return None,
        };
        let [date, level, color, result, next, moves, peeks, mode] = fields else {
            return None;
        };
        Some(Record {
            date: date.to_string(),
            level: level.parse().ok()?,
            color: parse_color(color),
            result: result.parse().ok()?,
            next: next.parse().ok()?,
            moves: moves.parse().ok()?,
            peeks: peeks.parse().ok()?,
            mode: Mode::parse(mode),
            illegal,
        })
    }

    pub fn to_line(&self) -> String {
        let c = color_code(self.color);
        let mode = self.mode.code();
        let Record {
            date,
            level,
            result,
            next,
            moves,
            peeks,
            illegal,
            ..
        } = self;
        format!("{date}\t{level}\t{c}\t{result}\t{next}\t{moves}\t{peeks}\t{mode}\t{illegal}")
    }
}

/// An unfinished game: "saved, level, w|b, peeks, mode, uci moves, start fen, illegal,
/// quiz right, quiz total, questions pending", tab-separated. Fields after the moves are
/// optional (older saves); an empty fen means the standard start.
#[derive(Clone)]
pub struct SavedGame {
    pub level: u32,
    pub me: Color,
    pub peeks: u32,
    pub mode: Mode,
    pub uci: Vec<String>,
    pub fen: Option<String>,
    pub illegal: u32,
    pub quiz_right: u32,
    pub quiz_total: u32,
    /// Scored questions still unanswered when the game was saved (asked again on resume).
    pub pending: u32,
}

impl SavedGame {
    /// A game about to start.
    pub fn new(level: u32, me: Color, mode: Mode, fen: Option<String>) -> SavedGame {
        SavedGame {
            level,
            me,
            peeks: 0,
            mode,
            uci: Vec::new(),
            fen,
            illegal: 0,
            quiz_right: 0,
            quiz_total: 0,
            pending: 0,
        }
    }

    pub fn parse(line: &str) -> Option<SavedGame> {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 6 || f.len() > 11 || f[0] != "saved" {
            return None;
        }
        let num = |i: usize| f.get(i).map_or(Some(0), |s| s.parse().ok());
        Some(SavedGame {
            level: f[1].parse().ok()?,
            me: parse_color(f[2]),
            peeks: f[3].parse().ok()?,
            mode: Mode::parse(f[4]),
            uci: f[5].split_whitespace().map(String::from).collect(),
            fen: f.get(6).filter(|s| !s.is_empty()).map(|s| s.to_string()),
            illegal: num(7)?,
            quiz_right: num(8)?,
            quiz_total: num(9)?,
            pending: num(10)?,
        })
    }

    pub fn to_line(&self) -> String {
        format!(
            "saved\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.level,
            color_code(self.me),
            self.peeks,
            self.mode.code(),
            self.uci.join(" "),
            self.fen.as_deref().unwrap_or(""),
            self.illegal,
            self.quiz_right,
            self.quiz_total,
            self.pending
        )
    }
}

pub fn load_history() -> Vec<Record> {
    fs::read_to_string(history_file())
        .unwrap_or_default()
        .lines()
        .filter_map(Record::parse)
        .collect()
}

pub fn append_history(r: &Record) -> io::Result<()> {
    let path = &history_file();
    fs::create_dir_all(path.parent().unwrap())?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{}", r.to_line())
}

pub fn write_history(all: &[Record]) -> io::Result<()> {
    let path = &history_file();
    fs::create_dir_all(path.parent().unwrap())?;
    let text: String = all.iter().map(|r| r.to_line() + "\n").collect();
    fs::write(path, text)
}

pub fn load_saved(slot: Slot) -> Option<SavedGame> {
    SavedGame::parse(fs::read_to_string(slot.file()).ok()?.trim_end_matches('\n'))
}

/// Saves (or with `None`, clears) the game in `slot`.
pub fn write_saved(slot: Slot, saved: Option<&SavedGame>) -> io::Result<()> {
    let path = &slot.file();
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

/// Everything an export file carries.
pub struct Progress {
    pub history: Vec<Record>,
    pub saved: Option<SavedGame>,
    pub training_saved: Option<SavedGame>,
    /// data/training.tsv lines, kept as text (the training module owns their format).
    pub training: Vec<String>,
}

/// Export file: a header comment, one history line per game, the saved games (play and
/// training), then the training progress.
pub fn export_text(history: &[Record], saved: &[&SavedGame], training: &[String]) -> String {
    let mut s = String::from("# b-chess progress\n");
    for r in history {
        s += &(r.to_line() + "\n");
    }
    for sg in saved {
        s += &(sg.to_line() + "\n");
    }
    for line in training {
        s += &(line.clone() + "\n");
    }
    s
}

/// Parses an export file; any unrecognized line rejects the whole file.
/// `is_training` recognizes training progress lines.
pub fn import_text(text: &str, is_training: impl Fn(&str) -> bool) -> Result<Progress, String> {
    let mut progress = Progress {
        history: Vec::new(),
        saved: None,
        training_saved: None,
        training: Vec::new(),
    };
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(r) = Record::parse(line) {
            progress.history.push(r);
        } else if let Some(sg) = SavedGame::parse(line) {
            match Slot::of(&sg.mode) {
                Slot::Play => progress.saved = Some(sg),
                Slot::Training => progress.training_saved = Some(sg),
            }
        } else if is_training(line) {
            progress.training.push(line.to_string());
        } else {
            return Err(format!("line {} is not b-chess progress", i + 1));
        }
    }
    Ok(progress)
}

pub fn expand_tilde(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), env::var("HOME")) {
        (Some(rest), Ok(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(p),
    }
}

pub fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// Days since 1970-01-01 to YYYY-MM-DD (Howard Hinnant's civil_from_days).
pub fn civil(days: i64) -> String {
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
pub fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    civil((secs / 86400) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), "1970-01-01");
        assert_eq!(civil(20000), "2024-10-04");
    }

    #[test]
    fn record_round_trips() {
        let line = "2026-10-24\t1300\tb\t-1\t1200\t34\t2\tladder\t1";
        assert_eq!(Record::parse(line).unwrap().to_line(), line);
        assert!(Record::parse("garbage").is_none());
        // Lines from before illegal moves were counted still load.
        let old = Record::parse("2026-10-24\t1300\tb\t-1\t1200\t34\t2\tfree").unwrap();
        assert_eq!((old.mode, old.illegal), (Mode::Free, 0));
        let t =
            Record::parse("2026-10-24\t800\tw\t1\t800\t12\t0\ttraining:ghost_games\t0").unwrap();
        assert_eq!(t.mode, Mode::Training("ghost_games".into()));
    }

    #[test]
    fn export_import_round_trips() {
        let history = vec![Record::parse("2026-10-24\t1300\tb\t-1\t1200\t34\t2\tfree\t0").unwrap()];
        let training_game = SavedGame {
            level: 1200,
            me: Color::White,
            peeks: 1,
            mode: Mode::Training("endgame_kq".into()),
            uci: vec!["d1d4".into()],
            fen: Some("8/8/8/4k3/8/8/8/3QK3 w - - 0 1".into()),
            illegal: 2,
            quiz_right: 3,
            quiz_total: 4,
            pending: 1,
        };
        let play_game = SavedGame {
            mode: Mode::Ladder,
            fen: None,
            ..training_game.clone()
        };
        let training = vec!["pass\tdiagnostic\t2026-10-24".to_string()];
        let text = export_text(&history, &[&play_game, &training_game], &training);
        let p = import_text(&text, |l| l.starts_with("pass\t")).unwrap();
        assert_eq!(p.history.len(), 1);
        assert_eq!(p.history[0].to_line(), history[0].to_line());
        // Each saved game lands in its own slot.
        assert_eq!(p.saved.unwrap().to_line(), play_game.to_line());
        assert_eq!(p.training_saved.unwrap().to_line(), training_game.to_line());
        assert_eq!(p.training, training);
        assert!(import_text("hello\n", |_| false).is_err());
        // A save from before training games still loads.
        let old = SavedGame::parse("saved\t1100\tb\t0\tladder\te2e4").unwrap();
        assert_eq!(
            (
                old.mode,
                old.fen,
                old.illegal,
                old.quiz_total,
                old.uci.len()
            ),
            (Mode::Ladder, None, 0, 0, 1)
        );
    }
}
