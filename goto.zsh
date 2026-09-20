# goto — jump to any git repo under ~/src by its dir name.
#
# Installed by Homebrew. Source it from your ~/.zshrc:
#   source "$HOMEBREW_PREFIX/share/goto/goto.zsh"

# gt <name> — jump to a repo under ~/src by its dir name.
gt() {
  local out target

  # `gt -` — jump back to where you were before your last `gt` jump. A two-item
  # toggle scoped to goto: it remembers the full path you left (subdirectory and
  # all) and ignores any manual `cd`s since, so it always returns you to the
  # previous repo. Repeating `gt -` bounces between the two. `_GOTO_PREV` is set
  # only by `gt` jumps below (not by plain `cd`), which is what makes this
  # "previous repo" rather than "previous directory".
  if [[ "$1" == "-" ]]; then
    if [[ -z "$_GOTO_PREV" || ! -d "$_GOTO_PREV" ]]; then
      print -u2 "gt: no previous directory yet"
      return 1
    fi
    cd "$_GOTO_PREV" && _GOTO_PREV="$OLDPWD"
    return
  fi

  # Informational subcommands print their output straight through — no fzf, no cd.
  if [[ "$1" == "--list" || "$1" == "--reindex" || "$1" == "--version" || "$1" == "-v" || "$1" == "--help" || "$1" == "-h" ]]; then
    gt-bin "$@"
    return $?
  fi
  out="$(gt-bin "$@")" || return 1
  if [[ "$(print -r -- "$out" | wc -l)" -gt 1 ]]; then
    target="$(print -r -- "$out" | fzf --select-1 --exit-0 --height=40% --reverse)" || return 1
  else
    target="$out"
  fi
  # Record where we're leaving before jumping, so `gt -` can bring us back to
  # the previous repo. `cd` sets $OLDPWD to the dir we came from on success.
  if [[ -n "$target" ]]; then
    cd "$target" && _GOTO_PREV="$OLDPWD"
  fi
}

# Tab-complete `gt <name>` from the same index the jump uses. Matching is
# case-insensitive and substring-anywhere (the `l:|=* r:|=*` matcher), to mirror
# how `gt` itself resolves a name.
_gt() {
  local -a repos
  repos=(${(f)"$(gt-bin --complete 2>/dev/null)"})
  compadd -M 'm:{a-zA-Z}={A-Za-z} l:|=* r:|=*' -a repos
}

# Register with the completion system. `compdef` only exists once `compinit` has
# run, and this file is often sourced from ~/.zshrc *before* a later compinit
# (e.g. one another tool's completions pull in). So register now if we can;
# otherwise defer to the first prompt — compinit has run by then — and unhook.
if (( $+functions[compdef] )); then
  compdef _gt gt
else
  autoload -Uz add-zsh-hook
  _gt_register() {
    (( $+functions[compdef] )) || return
    compdef _gt gt
    add-zsh-hook -d precmd _gt_register
    unfunction _gt_register
  }
  add-zsh-hook precmd _gt_register
fi
