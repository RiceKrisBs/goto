#!/usr/bin/env bash
# rx dev up step: ensure the Rust toolchain is installed.
set -euo pipefail

satisfied() {
  command -v cargo >/dev/null 2>&1 || test -x "$HOME/.cargo/bin/cargo"
}

satisfy() {
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
}

case "${1:-}" in
  --satisfied) satisfied ;;
  --satisfy)   satisfy ;;
  *) echo "usage: $0 --satisfied|--satisfy" >&2; exit 2 ;;
esac
