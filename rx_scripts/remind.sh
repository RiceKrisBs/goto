#!/usr/bin/env bash
# rx dev up step: remind the user to source ~/.goto.zsh (only if not already done).
#
# The marker file makes the two-phase satisfied?/satisfy contract work: it is
# absent on the first check (so satisfy runs) and deleted by the second check
# (so the reminder re-fires on the next rx dev up). Whether we actually remind
# is decided by grepping the user's shell rc files.
#
# rx captures and discards a step's stdout on success, and its spinner clobbers
# writes to /dev/tty, so we surface the reminder via a macOS dialog (osascript),
# which talks to the WindowServer and dodges both problems.
set -euo pipefail

marker="$HOME/.cache/goto/reminded"

satisfied() {
  # False when absent (triggers satisfy); when present, delete it and return true.
  test -f "$marker" && rm -f "$marker"
}

satisfy() {
  mkdir -p "$HOME/.cache/goto"
  touch "$marker"

  # Already sourced somewhere? Nothing to say. Match a real source/. directive,
  # not a mere mention (e.g. a comment) of the path.
  for rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.profile"; do
    [[ -f "$rc" ]] && grep -qE '^[[:space:]]*(source|\.)[[:space:]].*\.goto\.zsh' "$rc" && return 0
  done

  # macOS only; skip silently elsewhere (Linux/CI) so the step still succeeds.
  command -v osascript >/dev/null 2>&1 || return 0

  osascript -e 'display dialog "To finish setup, add this line to your shell rc file and open a new shell:\n\n  source \"$HOME/.goto.zsh\"" with title "goto — finish setup" buttons {"OK"} default button "OK" with icon note' >/dev/null
}

case "${1:-}" in
  --satisfied) satisfied ;;
  --satisfy)   satisfy ;;
  *) echo "usage: $0 --satisfied|--satisfy" >&2; exit 2 ;;
esac
