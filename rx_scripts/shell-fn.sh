#!/usr/bin/env bash
# rx dev up step: install the gt shell function to ~/.goto.zsh, refreshing on change.
set -euo pipefail

dest="$HOME/.goto.zsh"

satisfied() {
  cmp -s .goto.zsh "$dest"
}

satisfy() {
  cp .goto.zsh "$dest"
}

case "${1:-}" in
  --satisfied) satisfied ;;
  --satisfy)   satisfy ;;
  *) echo "usage: $0 --satisfied|--satisfy" >&2; exit 2 ;;
esac
