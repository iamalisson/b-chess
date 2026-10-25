#!/usr/bin/env bash
# Downloads the neural networks b-chess plays with into ./weights.
#   Maia 1100-1900 (~1.3 MB each): human-like play at those ratings.
#   Leela T1-256x10 (~37 MB): full-strength net used for levels 2200-3000.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p weights

get() {
  [ -s "weights/$1" ] && { echo "have  $1"; return; }
  echo "fetch $1"
  curl -fL --progress-bar -o "weights/$1.part" "$2" && mv "weights/$1.part" "weights/$1"
}

for r in 1100 1200 1300 1400 1500 1600 1700 1800 1900; do
  get "maia-$r.pb.gz" "https://github.com/CSSLab/maia-chess/releases/download/v1.0/maia-$r.pb.gz"
done
get "t1-256x10.pb.gz" "https://storage.lczero.org/files/networks-contrib/t1-256x10-distilled-swa-2432500.pb.gz"
