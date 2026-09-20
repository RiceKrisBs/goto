# goto — jump to any git repo under ~/src by its dir name.
#
# Installed by Homebrew. Source it from your ~/.zshrc:
#   source "$HOMEBREW_PREFIX/share/goto/goto.zsh"

# gt <name> — jump to a repo under ~/src by its dir name.
gt() {
  # Sourced into arbitrary user configs, so don't inherit their option set
  # (`setopt nounset` in particular breaks the `_GOTO_PREV` reads below).
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

  # Any remaining leading-dash argument is a flag for the binary, not a repo
  # name: `gt -` already returned above, and a repo whose dir name starts with
  # `-` isn't reachable either way. Print it straight through — no fzf, no cd.
  # A rule rather than a list of flags, so it can't drift from the binary's set.
  if [[ "$1" == -* ]]; then
    gt-bin "$@"
    return
  fi

  out="$(gt-bin "$@")" || return 1
  # Split on newlines natively rather than forking `wc -l`, which cost about as
  # much as the whole binary run. Note `${#${(f)out}}` can't replace the array:
  # a single-line result collapses to a scalar and yields its character count.
  candidates=(${(f)out})
  if (( ${#candidates} > 1 )); then
    target="$(print -r -- "$out" | fzf --select-1 --exit-0 --height=40% --reverse)" || return 1
  else
    target="$out"
  fi

  if [[ -z "$target" ]]; then
    print -u2 "gt: no target directory"
    return 1
  fi

  # Already here: return without touching _GOTO_PREV, which `cd` to the current
  # directory would otherwise overwrite with $PWD, silently killing `gt -`.
  # Compares resolved paths, so /A/sub → /A still records /A/sub.
  [[ "${target:A}" == "${PWD:A}" ]] && return 0

  # Record where we're leaving before jumping, so `gt -` can bring us back to
  # the previous repo. `cd` sets $OLDPWD to the dir we came from on success.
  cd "$target" && _GOTO_PREV="$OLDPWD"
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
