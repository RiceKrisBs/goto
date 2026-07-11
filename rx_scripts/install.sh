#!/usr/bin/env bash
# Build gt-bin and install it into ~/.cargo/bin.
# Run as the `satisfy` command of the rx dev up "install goto" step.
set -euo pipefail

# rustup installs cargo to ~/.cargo/bin but only edits profile files, so a
# freshly installed toolchain isn't on PATH yet within this same rx run.
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"

# rx dev up runs steps from the repo root.
echo "Building gt-bin (this can take ~30-60s on a cold cache)..."
cargo install --path .

goto_zsh="$HOME/.goto.zsh"
cp .goto.zsh "$goto_zsh"

cat <<EOF

gt-bin installed to ~/.cargo/bin.

Add this line to your ~/.zshrc, then open a new shell:

  source "$goto_zsh"

EOF
