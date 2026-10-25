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

pub fn saved_file() -> PathBuf {
    home().join("data/saved.tsv")
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

/// One finished game. Stored as a tab-separated line in data/history.tsv.
pub struct Record {
    pub date: String,
    pub level: u32,
    pub color: Color,
    pub result: i32, // 1 win, 0 draw, -1 loss
    pub next: u32,
    pub moves: u32,
    pub peeks: u32,
    pub ladder: bool,
}

impl Record {
    pub fn parse(line: &str) -> Option<Record> {
        let f: Vec<&str> = line.split('\t').collect();
        let [date, level, color, result, next, moves, peeks, mode] = f[..] else {
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
            ladder: mode == "ladder",
        })
    }

    pub fn to_line(&self) -> String {
        let c = color_code(self.color);
        let mode = if self.ladder { "ladder" } else { "free" };
        let Record {
            date,
            level,
            result,
            next,
            moves,
            peeks,
            ..
        } = self;
        format!("{date}\t{level}\t{c}\t{result}\t{next}\t{moves}\t{peeks}\t{mode}")
    }
}

/// An unfinished game: "saved<TAB>level<TAB>w|b<TAB>peeks<TAB>ladder|free<TAB>uci moves".
pub struct SavedGame {
    pub level: u32,
    pub me: Color,
    pub peeks: u32,
    pub ladder: bool,
    pub uci: Vec<String>,
}

impl SavedGame {
    pub fn parse(line: &str) -> Option<SavedGame> {
        let f: Vec<&str> = line.split('\t').collect();
        let ["saved", level, color, peeks, mode, moves] = f[..] else {
            return None;
        };
        Some(SavedGame {
            level: level.parse().ok()?,
            me: parse_color(color),
            peeks: peeks.parse().ok()?,
            ladder: mode == "ladder",
            uci: moves.split_whitespace().map(String::from).collect(),
        })
    }

    pub fn to_line(&self) -> String {
        let c = color_code(self.me);
        let mode = if self.ladder { "ladder" } else { "free" };
        format!(
            "saved\t{}\t{c}\t{}\t{mode}\t{}",
            self.level,
            self.peeks,
            self.uci.join(" ")
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

pub fn load_saved() -> Option<SavedGame> {
    SavedGame::parse(
        fs::read_to_string(saved_file())
            .ok()?
            .trim_end_matches('\n'),
    )
}

pub fn write_saved(saved: Option<&SavedGame>) -> io::Result<()> {
    let path = &saved_file();
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
pub fn export_text(history: &[Record], saved: Option<&SavedGame>) -> String {
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
pub fn import_text(text: &str) -> Result<(Vec<Record>, Option<SavedGame>), String> {
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
        let fresh = SavedGame {
            level: 1100,
            me: Color::Black,
            peeks: 0,
            ladder: false,
            uci: vec![],
        };
        assert_eq!(SavedGame::parse(&fresh.to_line()).unwrap().uci.len(), 0);
    }
}
