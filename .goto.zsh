# goto — jump to any git repo under ~/src by its dir name.
#
# Source this file from your ~/.zshrc:
#   source "/path/to/goto/goto.zsh"

# Make the cargo-installed gt-bin reachable without a separate PATH edit.
if [[ ":$PATH:" != *":$HOME/.cargo/bin:"* ]]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi

# gt <name> — jump to a repo under ~/src by its dir name.
gt() {
  local out target
  # Informational subcommands print their output straight through — no fzf, no cd.
  if [[ "$1" == "--list" || "$1" == "--reindex" ]]; then
    gt-bin "$@"
    return $?
  fi
  out="$(gt-bin "$@")" || return 1
  if [[ "$(print -r -- "$out" | wc -l)" -gt 1 ]]; then
    target="$(print -r -- "$out" | fzf --select-1 --exit-0 --height=40% --reverse)" || return 1
  else
    target="$out"
  fi
  [[ -n "$target" ]] && cd "$target"
}
