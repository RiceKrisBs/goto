# Tests for the `gt` shell function in goto.zsh.
#
#   zsh -f tests/goto_zsh_test.zsh
#
# The -f matters: without it an alias named `gt-bin` or `fzf` in ~/.zshrc is
# expanded into the stub definitions below at parse time.

emulate -R zsh

GOTO_ZSH=${GOTO_ZSH:-${0:A:h}/../goto.zsh}
[[ -r $GOTO_ZSH ]] || { print -u2 "cannot read $GOTO_ZSH"; exit 1 }
source $GOTO_ZSH

TMP=$(mktemp -d)
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

# ---- toggle ----

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

# ---- repeat jump (A3) ----

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

# ---- no previous jump (A4) ----

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

# ---- flag passthrough (A2/C4) ----

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

# ---- candidate counting (B5) ----

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

# ---- empty output (D7) ----

empty_output_is_an_error() {
  fzf_stub
  gt-bin() { return 0 }
  cd $TMP/start
  gt whatever 2>/dev/null
  assert_eq 1 $? "empty binary output should exit 1" || return 1
  assert_eq $TMP/start $PWD "empty binary output must not move the shell" || return 1
}

for t in \
  toggle_bounces_between_two_repos \
  toggle_ignores_manual_cd \
  repeat_jump_preserves_previous \
  repeat_jump_succeeds_quietly \
  jump_from_subdirectory_records_subdirectory \
  previous_dir_unset_reports_cleanly \
  previous_dir_gone_reports_cleanly \
  complete_flag_does_not_cd \
  unknown_flag_passes_through \
  single_candidate_skips_the_picker \
  multiple_candidates_use_the_picker \
  empty_output_is_an_error
do
  run $t
done

print
if (( failures )); then
  print -u2 "$failures of $total goto.zsh tests failed"
  exit 1
fi
print "all $total goto.zsh tests passed"
