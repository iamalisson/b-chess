mod app;
mod engine;
mod game;
mod levels;
mod store;
mod theme;
mod ui;

use app::App;
use std::{env, io, path::PathBuf, process};
use store::{fetch_networks, home, load_settings};

fn main() -> io::Result<()> {
    load_settings();
    let weights = env::var("MAIA_DIR").map_or_else(|_| home().join("weights"), PathBuf::from);
    if let Err(e) = fetch_networks(&weights) {
        eprintln!("{e}");
    }
    let levels = levels::available(&weights);
    if levels.is_empty() {
        eprintln!(
            "No networks in {} (needs curl and internet on first run)",
            weights.display()
        );
        process::exit(1);
    }
    let lc0 = env::var("LC0").unwrap_or_else(|_| "lc0".into());
    let mut app = App::new(levels, weights, lc0);
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}
