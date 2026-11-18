# Tests for the `gt` shell function:  zsh -f tests/goto_zsh_test.zsh
#
# The -f matters: without it an alias named `gt-bin` or `fzf` in ~/.zshrc is
# expanded into the stub definitions below at parse time.

emulate -R zsh

GOTO_ZSH=${GOTO_ZSH:-${0:A:h}/../goto.zsh}
[[ -r $GOTO_ZSH ]] || { print -u2 "cannot read $GOTO_ZSH"; exit 1 }
source $GOTO_ZSH

TMP=$(mktemp -d)
TMP=${TMP:A}
trap 'rm -rf $TMP' EXIT
mkdir -p $TMP/start $TMP/A/sub $TMP/B

integer total=0 failures=0

assert_eq() {
  [[ "$1" == "$2" ]] && return 0
  print -u2 "    $3"
  print -u2 "      expected: ${(qq)1}"
  print -u2 "      actual:   ${(qq)2}"
  return 1
}

run() {
  (( total++ ))
  if ( $1 ); then
    print "  ok    $1"
  else
    (( failures++ ))
    print "  FAIL  $1"
  fi
}

jump_stub() { gt-bin() { case $1 in A) print $TMP/A;; B) print $TMP/B;; esac } }
fzf_stub()  { rm -f $TMP/fzf-ran; fzf() { : > $TMP/fzf-ran; head -1 } }

expect_fzf() {
  if [[ -e $TMP/fzf-ran ]]; then
    return 0
  fi
  print -u2 "    $1"
  return 1
}

refute_fzf() {
  if [[ -e $TMP/fzf-ran ]]; then
    print -u2 "    $1"
    return 1
  fi
  return 0
}

toggle_bounces_between_two_repos() {
  jump_stub; fzf_stub
  cd $TMP/start
  gt A; gt B
  gt -; assert_eq $TMP/A $PWD "first gt - should return to A" || return 1
  gt -; assert_eq $TMP/B $PWD "second gt - should bounce back to B" || return 1
}

toggle_ignores_manual_cd() {
  jump_stub; fzf_stub
  cd $TMP/start
  gt A
  cd $TMP/B
  gt -; assert_eq $TMP/start $PWD "gt - should return to the pre-jump dir" || return 1
}

repeat_jump_preserves_previous() {
  jump_stub; fzf_stub
  cd $TMP/start
  gt A
  assert_eq $TMP/start "$_GOTO_PREV" "first jump should record start" || return 1
  gt A
  assert_eq $TMP/start "$_GOTO_PREV" "repeat jump should not clobber _GOTO_PREV" || return 1
  gt -
  assert_eq $TMP/start $PWD "gt - should still work after a repeat jump" || return 1
}

repeat_jump_succeeds_quietly() {
  jump_stub; fzf_stub
  cd $TMP/A
  gt A
  assert_eq 0 $? "a repeat jump should exit 0" || return 1
  assert_eq $TMP/A $PWD "a repeat jump should not move" || return 1
}

jump_from_subdirectory_records_subdirectory() {
  jump_stub; fzf_stub
  cd $TMP/A/sub
  gt A
  assert_eq $TMP/A $PWD "should have moved to the repo root" || return 1
  gt -
  assert_eq $TMP/A/sub $PWD "gt - should return into the subdirectory" || return 1
}

previous_dir_unset_reports_cleanly() {
  setopt nounset
  jump_stub; fzf_stub
  cd $TMP/start
  local msg; msg="$(gt - 2>&1)"
  assert_eq 1 $? "gt - with no previous jump should exit 1" || return 1
  assert_eq "gt: no previous directory yet" "$msg" || return 1
  assert_eq $TMP/start $PWD "gt - should not move" || return 1
}

previous_dir_gone_reports_cleanly() {
  jump_stub; fzf_stub
  mkdir -p $TMP/doomed
  cd $TMP/doomed
  gt A
  rmdir $TMP/doomed
  local msg; msg="$(gt - 2>&1)"
  assert_eq 1 $? "gt - to a deleted dir should exit 1" || return 1
  assert_eq "gt: no previous directory yet" "$msg" || return 1
}

toggle_to_current_dir_reports_cleanly() {
  jump_stub; fzf_stub
  cd $TMP/start
  gt A
  cd $TMP/start
  local msg; msg="$(gt - 2>&1)"
  assert_eq 1 $? "gt - to the current dir should exit 1" || return 1
  assert_eq "gt: already in the previous directory" "$msg" || return 1
  assert_eq $TMP/start $PWD "gt - should not move" || return 1
}

complete_flag_does_not_cd() {
  fzf_stub
  gt-bin() { print -l alpha beta }
  mkdir -p $TMP/start/alpha
  cd $TMP/start
  local out; out="$(gt --complete)"
  assert_eq 0 $? "--complete should exit 0" || return 1
  assert_eq $'alpha\nbeta' "$out" "--complete should print candidates" || return 1
  assert_eq $TMP/start $PWD "--complete must not move the shell" || return 1
  assert_eq "" "${_GOTO_PREV-}" "--complete must not set _GOTO_PREV" || return 1
  refute_fzf "--complete must not reach the fzf picker"
}

unknown_flag_passes_through() {
  fzf_stub
  gt-bin() { print -u2 "gt: unknown option '$1' (see: gt --help)"; return 1 }
  cd $TMP/start
  local msg; msg="$(gt --lst 2>&1)"
  assert_eq 1 $? "an unknown flag should exit 1" || return 1
  assert_eq "gt: unknown option '--lst' (see: gt --help)" "$msg" || return 1
  assert_eq $TMP/start $PWD "an unknown flag must not move the shell" || return 1
}

single_candidate_skips_the_picker() {
  fzf_stub
  gt-bin() { print $TMP/B }
  cd $TMP/start
  gt only
  assert_eq $TMP/B $PWD "should have jumped to the sole candidate" || return 1
  refute_fzf "a single candidate must not invoke fzf"
}

multiple_candidates_use_the_picker() {
  fzf_stub
  gt-bin() { print -l $TMP/A $TMP/B }
  cd $TMP/start
  gt many
  expect_fzf "multiple candidates should invoke fzf" || return 1
  assert_eq $TMP/A $PWD "should have jumped to the picker's choice" || return 1
}

empty_output_is_an_error() {
  fzf_stub
  gt-bin() { return 0 }
  cd $TMP/start
  gt whatever 2>/dev/null
  assert_eq 1 $? "empty binary output should exit 1" || return 1
  assert_eq $TMP/start $PWD "empty binary output must not move the shell" || return 1
}

completion_only_offers_the_first_argument() {
  gt-bin() { print -l alpha beta }
  local offered=0
  compadd() { offered=1 }
  local CURRENT=2; _gt
  assert_eq 1 $offered "gt <TAB> should offer repo names" || return 1
  offered=0; CURRENT=3; _gt
  assert_eq 0 $offered "gt foo <TAB> must not offer repo names" || return 1
}

# ---- against the real binary: set GT_BIN (e.g. target/debug/gt-bin) ----

real_bin() {
  unfunction gt-bin 2>/dev/null
  # One match per case, so reaching the picker means stdout had extra lines.
  fzf() { print -u2 "    fzf was invoked"; return 1 }
  path=(${GT_BIN:A:h} $path)
  export XDG_CACHE_HOME=$(mktemp -d $TMP/cache.XXXX)
  # `zsh -f` skips your rc files but still inherits your environment.
  unset GOTO_EXTRA_PRUNE
}

real_binary_jumps() {
  real_bin
  mkdir -p $TMP/root/x/alpha/.git $TMP/root/y/beta/.git
  export GOTO_ROOT=$TMP/root
  cd $TMP/start
  gt alpha
  assert_eq $TMP/root/x/alpha $PWD "should have jumped to alpha" || return 1
  assert_eq $TMP/start "$_GOTO_PREV" "should record where we came from" || return 1
}

real_binary_miss_stays_put() {
  real_bin
  mkdir -p $TMP/root/x/alpha/.git
  export GOTO_ROOT=$TMP/root
  cd $TMP/start
  gt nope 2>/dev/null
  assert_eq 1 $? "a miss should exit 1" || return 1
  assert_eq $TMP/start $PWD "a miss must not move the shell" || return 1
}

real_binary_typo_jumps_with_a_note() {
  real_bin
  mkdir -p $TMP/root/x/alpha/.git
  export GOTO_ROOT=$TMP/root
  cd $TMP/start
  # Not `$(gt ...)`: a subshell's `cd` wouldn't reach this shell.
  gt alpah 2>$TMP/err
  assert_eq $TMP/root/x/alpha $PWD "a typo should still reach alpha" || return 1
  assert_eq "gt: no repo matching 'alpah'; using alpha" "$(<$TMP/err)" "should say what it picked" || return 1
}

real_binary_several_matches_use_the_picker() {
  real_bin
  local root=$TMP/picker
  mkdir -p $root/a/neovim/.git $root/b/neovim-qt/.git $root/c/ripgrep/.git $root/d/ripgrep-all/.git
  export GOTO_ROOT=$root
  fzf() { cat > $TMP/fzf-input; head -1 $TMP/fzf-input }
  cd $TMP/start
  gt neov
  assert_eq "$root/a/neovim"$'\n'"$root/b/neovim-qt" "$(<$TMP/fzf-input)" "fzf should get both substring matches" || return 1
  assert_eq $root/a/neovim $PWD "should have jumped to the picker's choice" || return 1
  cd $TMP/start
  gt ripgerp 2>$TMP/err
  assert_eq "$root/c/ripgrep"$'\n'"$root/d/ripgrep-all" "$(<$TMP/fzf-input)" "fzf should get both tied close matches" || return 1
  assert_eq "gt: no repo matching 'ripgerp'; showing close matches" "$(<$TMP/err)" || return 1
}

real_binary_new_clone_found_after_background_reindex() {
  real_bin
  local root=$TMP/clone index=$XDG_CACHE_HOME/goto/index i
  mkdir -p $root/a/alpha/.git
  export GOTO_ROOT=$root
  cd $TMP/start; gt alpha
  mkdir -p $root/b/zola/.git
  # Age the index past the refresh debounce so the next call spawns a reindex.
  touch -t 202001010000 $index
  cd $TMP/start; gt zola 2>/dev/null
  assert_eq 1 $? "the first call answers from the old index" || return 1
  for i in {1..50}; do grep -q zola $index && break; sleep 0.1; done
  cd $TMP/start; gt zola
  assert_eq $root/b/zola $PWD "the miss should still have refreshed the index" || return 1
}

real_binary_extra_prune_applies_on_the_next_call() {
  real_bin
  local root=$TMP/prune
  mkdir -p $root/a/alpha/.git $root/vendor/vendored-lib/.git
  export GOTO_ROOT=$root
  cd $TMP/start; gt vendored-lib
  assert_eq $root/vendor/vendored-lib $PWD "the repo is found before pruning" || return 1
  export GOTO_EXTRA_PRUNE=vendor
  cd $TMP/start; gt vendored-lib 2>/dev/null
  assert_eq 1 $? "the next call should crawl with the new prune set" || return 1
  assert_eq $TMP/start $PWD || return 1
}

real_binary_rebuilds_a_0_4_1_cache() {
  real_bin
  local root=$TMP/oldcache index=$XDG_CACHE_HOME/goto/index
  mkdir -p $root/a/alpha/.git $root/b/beta/.git $index:h
  # 0.4.1 format: root line, then repo paths. Read as the new format, alpha's
  # path would be taken for the prune line and beta would be missing.
  print -l $root $root/a/alpha > $index
  export GOTO_ROOT=$root
  cd $TMP/start; gt beta
  assert_eq $root/b/beta $PWD "an old index should be rebuilt, not trusted" || return 1
  assert_eq ".git,.terraform,node_modules" "$(sed -n 2p $index)" "should be rewritten in the new format" || return 1
}

real_binary_skips_a_deleted_repo() {
  real_bin
  local root=$TMP/deleted
  mkdir -p $root/a/neovim/.git $root/b/neovim-qt/.git
  export GOTO_ROOT=$root
  cd $TMP/start; gt neovim
  rm -rf $root/a/neovim
  cd $TMP/start; gt neovim
  assert_eq $root/b/neovim-qt $PWD "a cached but deleted exact match should fall through" || return 1
}

# Take the refresh lock from another process, the way a running crawl would.
# Perl's flock is flock(2), the same lock gt-bin takes; zsh's `zsystem flock`
# is an fcntl lock, which doesn't conflict with it on Linux. Sets HOLDER.
hold_refresh_lock() {
  mkdir -p $1:h
  rm -f $TMP/holder
  perl -e 'open(my $f, ">>", $ARGV[0]) or die; flock($f, 2) or die; $| = 1; print "held\n"; sleep 10' $1 > $TMP/holder &
  HOLDER=$!
  local i
  for i in {1..50}; do [[ -s $TMP/holder ]] && return 0; sleep 0.1; done
  print -u2 "    could not take the lock"
  return 1
}

real_binary_skips_refresh_while_a_crawl_holds_the_lock() {
  real_bin
  local root=$TMP/locked index=$XDG_CACHE_HOME/goto/index i
  mkdir -p $root/a/alpha/.git
  export GOTO_ROOT=$root
  cd $TMP/start; gt alpha
  mkdir -p $root/b/zola/.git
  touch -t 202001010000 $index
  {
    hold_refresh_lock $index.lock || return 1
    cd $TMP/start; gt zola 2>/dev/null
    gt-bin --refresh
    assert_eq 0 $? "a refresh that finds the lock taken should exit cleanly" || return 1
    sleep 0.5
    assert_eq 0 "$(grep -c zola $index)" "no crawl may run while the lock is held" || return 1
  } always {
    kill $HOLDER 2>/dev/null; wait $HOLDER 2>/dev/null
  }
  cd $TMP/start; gt zola 2>/dev/null
  for i in {1..50}; do grep -q zola $index && break; sleep 0.1; done
  cd $TMP/start; gt zola
  assert_eq $root/b/zola $PWD "refreshes should resume once the lock is free" || return 1
}

real_binary_reindex_waits_for_a_running_crawl() {
  real_bin
  local root=$TMP/waits index=$XDG_CACHE_HOME/goto/index job rc
  mkdir -p $root/a/alpha/.git
  export GOTO_ROOT=$root
  {
    hold_refresh_lock $index.lock || return 1
    gt-bin --reindex 2>/dev/null &
    job=$!
    sleep 0.3
    [[ ! -e $index ]] || { print -u2 "    --reindex crawled while the lock was held"; return 1 }
  } always {
    kill $HOLDER 2>/dev/null; wait $HOLDER 2>/dev/null
  }
  wait $job; rc=$?
  assert_eq 0 $rc "--reindex should finish once the lock is free" || return 1
  assert_eq 1 "$(grep -c alpha $index)" "--reindex should have written the index" || return 1
}

relative_root_follows_pwd() {
  real_bin
  mkdir -p $TMP/w1/src/one/.git $TMP/w2/src/two/.git
  export GOTO_ROOT=src
  cd $TMP/w1; gt one
  assert_eq $TMP/w1/src/one $PWD "should find w1's repo" || return 1
  cd $TMP/w2; gt two 2>/dev/null
  assert_eq $TMP/w2/src/two $PWD "w1's cache must not answer for w2" || return 1
}

tests=(
  toggle_bounces_between_two_repos
  toggle_ignores_manual_cd
  repeat_jump_preserves_previous
  repeat_jump_succeeds_quietly
  jump_from_subdirectory_records_subdirectory
  previous_dir_unset_reports_cleanly
  previous_dir_gone_reports_cleanly
  toggle_to_current_dir_reports_cleanly
  complete_flag_does_not_cd
  unknown_flag_passes_through
  single_candidate_skips_the_picker
  multiple_candidates_use_the_picker
  empty_output_is_an_error
  completion_only_offers_the_first_argument
)
if [[ -n ${GT_BIN-} ]]; then
  [[ -x $GT_BIN ]] || { print -u2 "GT_BIN is not executable: $GT_BIN"; exit 1 }
  tests+=(
    real_binary_jumps
    real_binary_miss_stays_put
    real_binary_typo_jumps_with_a_note
    real_binary_several_matches_use_the_picker
    real_binary_new_clone_found_after_background_reindex
    real_binary_extra_prune_applies_on_the_next_call
    real_binary_rebuilds_a_0_4_1_cache
    real_binary_skips_a_deleted_repo
    real_binary_skips_refresh_while_a_crawl_holds_the_lock
    real_binary_reindex_waits_for_a_running_crawl
    relative_root_follows_pwd
  )
else
  print "  (set GT_BIN to also run the tests against the real binary)"
fi

for t in $tests; do
  run $t
done

print
if (( failures )); then
  print -u2 "$failures of $total goto.zsh tests failed"
  exit 1
fi
print "all $total goto.zsh tests passed"
