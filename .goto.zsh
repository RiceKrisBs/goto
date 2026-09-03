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
  # `gt upgrade` — bring the install up to date with the clone this binary was
  # built from: pull, rebuild, refresh this file, and re-source it so the new
  # function is live in the current shell (no new shell needed). Runs from any
  # directory. Calls the same install scripts `rx dev up` orchestrates, but
  # directly, so upgrading needs no `rx`.
  if [[ "$1" == "upgrade" ]]; then
    local src
    src="$(gt-bin --source)" || return 1
    if [[ ! -d "$src/.git" ]]; then
      print -u2 "gt: source clone not found at ${src:-<unknown>} (moved or deleted?)"
      return 1
    fi

    # Upgrade means "get the latest release", which lives on main — not whatever
    # branch the dev clone happens to be on. Refuse elsewhere rather than pull
    # and rebuild from a feature branch by surprise. (main is hardcoded; update
    # this if the default branch is ever renamed.)
    local branch
    branch="$(git -C "$src" symbolic-ref --short -q HEAD)"
    if [[ "$branch" != "main" ]]; then
      print -u2 "gt: upgrade runs only on main, but $src is on '${branch:-a detached HEAD}'."
      print -u2 "gt: switch it to main, or rebuild that branch with: cargo install --path \"$src\""
      return 1
    fi

    # Fast-forward to the latest commit. Contacting the remote is the only way
    # to learn whether a newer version exists; --ff-only refuses to merge if the
    # clone has diverged, rather than silently mangling it.
    local before after
    before="$(git -C "$src" rev-parse HEAD)" || return 1
    git -C "$src" pull --ff-only --quiet || {
      print -u2 "gt: 'git pull --ff-only' failed in $src — resolve it manually"
      return 1
    }
    after="$(git -C "$src" rev-parse HEAD)" || return 1

    # Nothing pulled, and the installed binary + function already match the
    # source? Then there is genuinely nothing to do — don't rebuild or re-source.
    if [[ "$before" == "$after" ]] \
       && ( cd "$src" && ./rx_scripts/build.sh --satisfied && ./rx_scripts/shell-fn.sh --satisfied ); then
      print "gt: already up to date ($(gt-bin --version))"
      return 0
    fi

    # Something changed (new commits, or a stale binary/function) — rebuild and
    # refresh what's actually out of date (each step skips itself when already
    # satisfied), then re-source so the new function is live now.
    (
      cd "$src" || exit 1
      ./rx_scripts/build.sh --satisfied    || ./rx_scripts/build.sh --satisfy    || exit 1
      ./rx_scripts/shell-fn.sh --satisfied || ./rx_scripts/shell-fn.sh --satisfy || exit 1
    ) || return 1
    source "$HOME/.goto.zsh"

    if [[ "$before" != "$after" ]]; then
      print "gt upgraded to $(gt-bin --version)"
    else
      print "gt: rebuilt $(gt-bin --version) (installed copy was stale)"
    fi
    return 0
  fi
  # Informational subcommands print their output straight through — no fzf, no cd.
  if [[ "$1" == "--list" || "$1" == "--reindex" || "$1" == "--version" || "$1" == "-v" || "$1" == "--source" || "$1" == "--help" || "$1" == "-h" ]]; then
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

# Tab-complete `gt <name>` from the same index the jump uses. Matching is
# case-insensitive and substring-anywhere (the `l:|=* r:|=*` matcher), to mirror
# how `gt` itself resolves a name. Only wired up if the completion system is
# loaded (compinit) — source this after compinit, e.g. after oh-my-zsh.
if (( $+functions[compdef] )); then
  _gt() {
    local -a repos
    repos=(${(f)"$(gt-bin --complete 2>/dev/null)"})
    compadd -M 'm:{a-zA-Z}={A-Za-z} l:|=* r:|=*' -a repos
  }
  compdef _gt gt
fi
