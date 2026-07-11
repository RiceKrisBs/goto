#!/usr/bin/env bash
# rx dev up step: build gt-bin into ~/.cargo/bin, rebuilding when sources change.
set -euo pipefail

bin="$HOME/.cargo/bin/gt-bin"

satisfied() {
  # Binary exists and nothing in the sources is newer than it.
  test -x "$bin" && ! find src Cargo.toml -newer "$bin" | grep -q .
}

satisfy() {
  # rustup installs cargo to ~/.cargo/bin but only edits profile files, so a
  # freshly installed toolchain isn't on PATH yet within this same rx run.
  [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
  echo "Building gt-bin (this can take ~30-60s on a cold cache)..."
  cargo install --path .
}

case "${1:-}" in
  --satisfied) satisfied ;;
  --satisfy)   satisfy ;;
  *) echo "usage: $0 --satisfied|--satisfy" >&2; exit 2 ;;
esac
