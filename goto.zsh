# goto — jump to any git repo under ~/src by its dir name.
#
# Installed by Homebrew. Source it from your ~/.zshrc:
#   source "$HOMEBREW_PREFIX/share/goto/goto.zsh"

# gt <name> — jump to a repo under ~/src by its dir name.
gt() {
  emulate -L zsh

  local out target
  local -a candidates

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

  # A rule, not a list of flags, so it can't drift. `gt -` returned above.
  if [[ "$1" == -* ]]; then
    gt-bin "$@"
    return
  fi

  out="$(gt-bin "$@")" || return 1
  # Not `${#${(f)out}}`: one line collapses to a scalar and yields its length.
  candidates=(${(f)out})
  if (( ${#candidates} > 1 )); then
    target="$(print -r -- "$out" | fzf --height=40% --reverse)" || return 1
  else
    target="$out"
  fi

  if [[ -z "$target" ]]; then
    print -u2 "gt: no target directory"
    return 1
  fi

  # `cd` to $PWD would set OLDPWD to $PWD, overwriting _GOTO_PREV.
  [[ "${target:A}" == "${PWD:A}" ]] && return 0

  # Record where we're leaving before jumping, so `gt -` can bring us back to
  # the previous repo. `cd` sets $OLDPWD to the dir we came from on success.
  cd "$target" && _GOTO_PREV="$OLDPWD"
}

# Tab-complete `gt <name>` from the same index the jump uses. Matching is
# case-insensitive and substring-anywhere (the `l:|=* r:|=*` matcher), to mirror
# how `gt` itself resolves a name.
_gt() {
  (( CURRENT == 2 )) || return 1
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
