<div align="center">

# ♞ b-chess

**A blindfold chess trainer for the terminal.**

Play against [Lc0](https://lczero.org) from ~600 to ~3000 Elo without seeing the board,
climb a ladder one win at a time, and peek only when you really need to.

![Rust](https://img.shields.io/badge/Rust-2024-000?logo=rust)
![ratatui](https://img.shields.io/badge/TUI-ratatui-4b9bd6)
![Lc0](https://img.shields.io/badge/engine-Lc0-2b6cb0)
![Maia](https://img.shields.io/badge/nets-Maia%20%2B%20Leela-6b46c1)

</div>

---

## Features

- **Blindfold by default**: you see the moves, not the board. Type `board` (or press Tab) to peek; peeks are counted.
- **Two modes**
  - **Ladder**: start at the bottom; a win moves you up a level, a loss moves you down.
  - **Free play**: pick any level. Recorded, but doesn't move your ladder.
- **Play as white, black or random.**
- **Human-like opponents**: the [Maia](https://maiachess.com) networks are trained on real games at each rating, so they make the kind of mistakes people make at that level.
- **Saves itself**: every move is saved, so you can close the app and resume later.
- **Take your progress with you**: export everything to one file and import it on another PC.
- **History**: date, level, color, result, moves and peeks of every game.

## Levels

| Elo | Opponent |
|---|---|
| 600 – 1000 | `maia-1100` mixed with 50% → 10% random moves |
| 1100 – 1900 | `maia-1100` … `maia-1900`, raw policy (1 node), as Maia is meant to play |
| 2000 – 2100 | `maia-1900` with 16 / 64 nodes of search |
| 2200 – 3000 | Leela `T1-256x10` with 1 → 256 nodes |

Only the Maia ratings (1100–1900) come from the networks themselves. The other levels are rough estimates that haven't been calibrated with matches.

## Install

You need **Rust**, **[Lc0](https://github.com/LeelaChessZero/lc0)** and the network files.

```sh
git clone https://github.com/iamalisson/b-chess && cd b-chess
./scripts/fetch-weights.sh      # Maia 1100-1900 + Leela T1 (~50 MB)
```

Get `lc0` on your `PATH`:

| System | How |
|---|---|
| NixOS / Nix | `nix-shell` (uses the included `shell.nix`) |
| Arch | `yay -S lc0` |
| macOS | `brew install lc0` |
| Others | [build or download a release](https://github.com/LeelaChessZero/lc0/releases) |

Then run:

```sh
cargo run --release
```

`LC0=/path/to/lc0` and `MAIA_DIR=/path/to/weights` override the defaults.

> **Speed:** levels up to 2100 answer instantly. From 2200 up, the big network runs on the CPU,
> and a build with only the Eigen backend can take several seconds per move at 2800+.
> A GPU or oneDNN/BLAS build of Lc0 makes this much faster.

## How to play

| Where | Keys / commands |
|---|---|
| Game | `Nf3` `exd5` `O-O` `e8=Q` (SAN) or `g1f3` (UCI) |
| | `board` / `b` / Tab: show or hide the board |
| | `help` / `?` / F1: all commands |
| | `save`: back to the menu (game kept) · `quit`: save and exit |
| | `resign` · Esc Esc: abandon |
| Menu | ↑↓ Enter · `r` resume · `n` new · `h` history · `e` export · `i` import · `q` quit |

## Progress & changing PCs

Progress lives in `data/` (gitignored):

- `data/history.tsv`: one line per finished game
- `data/saved.tsv`: the game in progress

**Export progress** writes a single text file (default `~/b-chess-progress.txt`).
Copy it to the other machine and choose **Import progress** there.

## Built with

| | |
|---|---|
| [Rust](https://www.rust-lang.org) | language |
| [ratatui](https://ratatui.rs) + [crossterm](https://github.com/crossterm-rs/crossterm) | terminal UI |
| [shakmaty](https://github.com/niklasf/shakmaty) | chess rules, SAN/UCI parsing, repetition detection |
| [image](https://github.com/image-rs/image) | background picture, drawn with half-block characters |
| [Lc0](https://lczero.org) | neural-network chess engine, spoken to over UCI |
| [Maia Chess](https://github.com/CSSLab/maia-chess) | human-like networks, 1100–1900 |
| [Leela T1-256x10](https://lczero.org/play/networks/bestnets/) | strong network for 2200–3000 |
