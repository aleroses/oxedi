#!/usr/bin/env bash
# Checks complexity.py's comparison with a baseline on hand-written measurements (no lizard).
# Run: tools/complexity/tests/complexity.sh
set -uo pipefail

script="$(cd "$(dirname "$0")/.." && pwd)/complexity.py"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failed=0

baseline() {
  printf '# header\n' > "$tmp/baseline"
  printf '%s\n' "$@" | tr ' ' '\t' >> "$tmp/baseline"
}

measurements() {
  printf '%s\n' "$@" > "$tmp/measured.csv"
}

# expect STATUS PATTERN [ARGS...]: runs the check and wants that exit status and output line.
expect() {
  local status=$1 pattern=$2 output actual
  shift 2
  output=$(python3 "$script" "$tmp/baseline" --measurements "$tmp/measured.csv" "$@" 2>&1)
  actual=$?
  if [ "$actual" != "$status" ] || ! grep -qE -- "$pattern" <<< "$output"; then
    echo "FAIL: wanted status $status and /$pattern/, got $actual:"
    echo "$output"
    failed=1
  fi
}

baseline 'a.rs big 12 2' 'a.rs fmt 20 0' 'a.rs fmt 9 1'
measurements 'a.rs,big,12,2' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0' 'a.rs,small,7,3'
expect 0 'ok, 4 functions measured'

measurements 'a.rs,big,12,2' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0' 'a.rs,small,8,3'
expect 1 'FAIL a.rs: small: CCN 8, nesting 3 is over the targets'

measurements 'a.rs,big,12,2' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0' 'a.rs,small,2,4'
expect 1 'FAIL a.rs: small: CCN 2, nesting 4 is over the targets'

measurements 'a.rs,big,13,2' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0'
expect 1 "FAIL a.rs: big: CCN 13, nesting 2 is worse than the baseline's CCN 12, nesting 2"

measurements 'a.rs,big,12,4' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0'
expect 1 "FAIL a.rs: big: CCN 12, nesting 4 is worse"

# Nesting within the target is not a regression of a function kept for its CCN.
measurements 'a.rs,big,12,3' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0'
expect 0 'ok'

measurements 'a.rs,big,11,2' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0'
expect 1 "STALE a.rs: big: CCN 11, nesting 2 is better than the baseline's CCN 12, nesting 2"

measurements 'a.rs,big,7,2' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0'
expect 1 'STALE a.rs: big: baseline entry CCN 12, nesting 2 is now within the targets'

measurements 'a.rs,fmt,9,1' 'a.rs,fmt,20,0'
expect 1 'STALE a.rs: big: baseline entry .* or the function is gone'

# Functions sharing a name are compared worst with worst: a third one over the target is new.
measurements 'a.rs,big,12,2' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0' 'a.rs,fmt,8,0'
expect 1 'FAIL a.rs: fmt: CCN 8, nesting 0 is over the targets'

measurements 'a.rs,big,13,2' 'a.rs,fmt,9,1' 'a.rs,fmt,20,0'
expect 1 'baseline was not updated' --update
grep -q 'big	12	2' "$tmp/baseline" || { echo "FAIL: a refused update changed the baseline"; failed=1; }

measurements 'a.rs,big,10,2' 'a.rs,fmt,20,0'
expect 0 'wrote .*rs 2' --update
if ! grep -q 'big	10	2' "$tmp/baseline" || grep -q 'fmt	9' "$tmp/baseline"; then
  echo "FAIL: the update did not lower big and drop the second fmt:"
  cat "$tmp/baseline"
  failed=1
fi

baseline 'a.rs big twelve 2'
expect 1 'expected path, function, ccn and nesting separated by tabs'

# A function whose file moved matches its gone entry; --update records the move.
baseline 'a.rs big 12 2' 'a.rs fmt 20 0'
measurements 'b.rs,big,11,2' 'a.rs,fmt,20,0'
expect 1 "MOVED b.rs: big: CCN 11, nesting 2 matches the gone baseline entry a.rs: big \(CCN 12, nesting 2\)"
expect 0 'wrote' --update
if ! grep -q 'b.rs	big	11	2' "$tmp/baseline" || grep -q 'a.rs	big' "$tmp/baseline"; then
  echo "FAIL: the update did not record the move:"
  cat "$tmp/baseline"
  failed=1
fi

# A moved function that got worse is not a move: it is new, and its old entry is stale.
baseline 'a.rs big 12 2'
measurements 'b.rs,big,13,2'
expect 1 'FAIL b.rs: big: CCN 13, nesting 2 is over the targets'
expect 1 'STALE a.rs: big: baseline entry CCN 12, nesting 2 is now within the targets'

# A function simplified below the targets still exists, so its entry cannot move to a new
# function of the same name elsewhere: the entry is stale and the new function fails.
baseline 'a.rs fmt 20 0'
measurements 'a.rs,fmt,5,0' 'b.rs,fmt,15,0'
expect 1 'STALE a.rs: fmt: baseline entry CCN 20, nesting 0 is now within the targets'
expect 1 'FAIL b.rs: fmt: CCN 15, nesting 0 is over the targets'
if grep -q MOVED <<< "$(python3 "$script" "$tmp/baseline" --measurements "$tmp/measured.csv")"; then
  echo "FAIL: an entry whose function still exists was taken as moved"
  failed=1
fi

# A reviewed exception is recorded on purpose with --accept.
baseline 'a.rs fmt 20 0'
measurements 'a.rs,fmt,21,0' 'a.rs,other,9,0'
expect 1 "make complexity-update ACCEPT='a.rs::fmt'"
expect 1 'baseline was not updated: 1 FAIL' --update --accept 'a.rs::fmt'
expect 0 'ACCEPTED FAIL a.rs: fmt: CCN 21' --update --accept 'a.rs::fmt' --accept 'a.rs::other'
grep -q 'a.rs	fmt	21	0' "$tmp/baseline" || { echo "FAIL: --accept did not raise fmt"; failed=1; }
expect 1 "--accept 'a.rs::nothing' names no function" --update --accept 'a.rs::nothing'
expect 2 'needs --update' --accept 'a.rs::fmt'

# The pull request report lists added and raised entries, counts moves and never fails.
printf '# old\na.rs\tfmt\t20\t0\na.rs\tbig\t12\t2\n' > "$tmp/old"
baseline 'a.rs fmt 22 0' 'b.rs big 12 2' 'c.rs fresh 9 1'
output=$(GITHUB_STEP_SUMMARY="$tmp/summary" python3 "$script" "$tmp/baseline" --compare-with "$tmp/old")
status=$?
for want in '::warning title=Complexity baseline::2 entry' \
  'a.rs. .fmt.: CCN 22, nesting 0, raised from CCN 20, nesting 0' \
  'c.rs. .fresh.: CCN 9, nesting 1, added' '1 entry\(ies\) moved' \
  '- .big.: .a\.rs. → .b\.rs., CCN 12, nesting 2'; do
  if [ "$status" != 0 ] || ! grep -qE -- "$want" <<< "$output"; then
    echo "FAIL: the report wanted /$want/ and status 0, got $status:"
    echo "$output"
    failed=1
  fi
done
grep -q 'entries added or raised' "$tmp/summary" || { echo "FAIL: no job summary written"; failed=1; }
output=$(python3 "$script" "$tmp/old" --compare-with "$tmp/old")
grep -q 'no entry added or raised' <<< "$output" || { echo "FAIL: unchanged report: $output"; failed=1; }

# Python nesting comes from the ast module: an elif sits at its if's level, a nested function
# and a comprehension do not deepen the enclosing one, try and with do.
cat > "$tmp/sample.py" <<'EOF'
def outer(x):
    if x:
        pass
    elif x > 1:
        for i in x:
            if i:
                pass
    else:
        with x:
            pass
    def inner():
        try:
            while x:
                pass
        except ValueError:
            pass
    return [y for y in x if y]


def flat(x):
    return [y for y in x if y]
EOF
depths=$(python3 -B -c "import sys; sys.path.insert(0, sys.argv[1]); import complexity; \
print(sorted(complexity.python_nesting(sys.argv[2]).items()))" "$(dirname "$script")" "$tmp/sample.py")
if [ "$depths" != "[(1, 3), (11, 2), (20, 0)]" ]; then
  echo "FAIL: Python nesting: wanted [(1, 3), (11, 2), (20, 0)], got $depths"
  failed=1
fi

# ---- the touch rule: a change that modifies a baseline function must leave it within the targets.
# These cases build a throwaway git repository, so the changed lines are real diffs.

repo="$tmp/repo"

# new_repo: a repository whose branch `base` holds a.rs of 20 lines.
new_repo() {
  rm -rf "$repo"
  mkdir "$repo"
  git -C "$repo" init -q -b main
  git -C "$repo" config user.email t@example.com
  git -C "$repo" config user.name t
  seq 1 20 | sed 's/^/line /' > "$repo/a.rs"
  git -C "$repo" add a.rs
  git -C "$repo" commit -q -m base
  git -C "$repo" branch base
}

# in_repo STATUS PATTERN [ARGS...]: like expect, run inside the repository against `base`.
in_repo() {
  local status=$1 pattern=$2 output actual
  shift 2
  output=$(cd "$repo" && python3 "$script" "$tmp/baseline" --measurements "$tmp/measured.csv" \
    --base base "$@" 2>&1)
  actual=$?
  if [ "$actual" != "$status" ] || ! grep -qE -- "$pattern" <<< "$output"; then
    echo "FAIL: wanted status $status and /$pattern/, got $actual:"
    echo "$output"
    failed=1
  fi
  last_output=$output
}

# no_line PATTERN: the last in_repo output must not hold that line.
no_line() {
  if grep -qE -- "$1" <<< "$last_output"; then
    echo "FAIL: output holds /$1/:"
    echo "$last_output"
    failed=1
  fi
}

# An uncommitted change inside big (lines 3 to 9), which stays over the targets.
new_repo
sed -i '5s/.*/changed/' "$repo/a.rs"
baseline 'a.rs big 12 2'
measurements 'a.rs,big,12,2,3,9'
in_repo 1 "TOUCHED a.rs::big ccn 12 nesting 2 \(targets: ccn 7, nesting 3\): this change modifies it; bring it within the targets by fixing its cause, or record a flat dispatch with make complexity-update ACCEPT='a.rs::big'"

# The same change, committed.
git -C "$repo" commit -q -am change
in_repo 1 'TOUCHED a.rs::big'

# A reviewed flat dispatch is exempt.
baseline 'a.rs big 12 2 flat'
in_repo 0 'ok, 1 functions measured'

# Brought within the targets, the entry is stale and nothing is touched.
baseline 'a.rs big 12 2'
measurements 'a.rs,big,7,2,3,9'
in_repo 1 'STALE a.rs: big: baseline entry CCN 12, nesting 2 is now within the targets'
no_line TOUCHED

# A baseline function in a modified file but outside the changed lines.
measurements 'a.rs,big,12,2,10,18'
in_repo 0 'ok'

# A deletion between two lines of a function modifies it; one between two functions does not.
new_repo
sed -i '5d' "$repo/a.rs"
measurements 'a.rs,big,12,2,3,9' 'a.rs,other,9,1,12,18'
baseline 'a.rs big 12 2' 'a.rs other 9 1'
in_repo 1 'TOUCHED a.rs::big'
no_line 'TOUCHED a.rs::other'
git -C "$repo" checkout -q a.rs
sed -i '10d' "$repo/a.rs"
measurements 'a.rs,big,12,2,3,9' 'a.rs,other,9,1,11,18'
in_repo 0 'ok'

# A renamed file whose functions are unchanged: MOVED, not TOUCHED.
new_repo
git -C "$repo" mv a.rs b.rs
baseline 'a.rs big 12 2'
measurements 'b.rs,big,12,2,3,9'
in_repo 1 'MOVED b.rs: big'
no_line TOUCHED

# --update --accept records the flat marker, and a later --update keeps it.
new_repo
sed -i '5s/.*/changed/' "$repo/a.rs"
baseline 'a.rs big 12 2'
measurements 'a.rs,big,12,2,3,9'
in_repo 1 'baseline was not updated' --update
in_repo 0 'wrote' --update --accept 'a.rs::big'
grep -qP 'a.rs\tbig\t12\t2\tflat$' "$tmp/baseline" || {
  echo "FAIL: --accept did not write the flat marker:"; cat "$tmp/baseline"; failed=1; }
in_repo 0 'ok'
measurements 'a.rs,big,12,2,3,9' 'a.rs,other,9,1,12,18'
in_repo 0 'wrote' --update --accept 'a.rs::other'
in_repo 0 'wrote' --update
grep -qP 'a.rs\tbig\t12\t2\tflat$' "$tmp/baseline" || {
  echo "FAIL: a later --update lost the flat marker:"; cat "$tmp/baseline"; failed=1; }

# A flat entry follows its function when the file moves.
new_repo
git -C "$repo" mv a.rs b.rs
baseline 'a.rs big 12 2 flat'
measurements 'b.rs,big,12,2,3,9'
in_repo 0 'wrote' --update
grep -qP 'b.rs\tbig\t12\t2\tflat$' "$tmp/baseline" || {
  echo "FAIL: the move lost the flat marker:"; cat "$tmp/baseline"; failed=1; }

# A fifth field other than flat is refused.
baseline 'a.rs big 12 2 other'
in_repo 1 'expected path, function, ccn and nesting'

# A base ref that cannot be found skips the check with one line and does not fail.
new_repo
sed -i '5s/.*/changed/' "$repo/a.rs"
baseline 'a.rs big 12 2'
measurements 'a.rs,big,12,2,3,9'
output=$(cd "$repo" && python3 "$script" "$tmp/baseline" --measurements "$tmp/measured.csv" \
  --base nosuchref 2>&1)
status=$?
if [ "$status" != 0 ] || [ "$(grep -c 'touched check skipped' <<< "$output")" != 1 ] \
  || ! grep -q 'nosuchref' <<< "$output"; then
  echo "FAIL: a missing base ref should skip the touched check with one line, got $status:"
  echo "$output"
  failed=1
fi

# A file renamed and edited inside a baseline function in one change: TOUCHED, not a quiet move.
new_repo
git -C "$repo" mv a.rs b.rs
sed -i '5s/.*/changed/' "$repo/b.rs"
baseline 'a.rs big 12 2'
measurements 'b.rs,big,12,2,3,9'
in_repo 1 'TOUCHED b.rs::big'
baseline 'a.rs big 12 2 flat'
in_repo 0 'wrote' --update
grep -qP 'b.rs\tbig\t12\t2\tflat$' "$tmp/baseline" || { echo "FAIL: flat lost"; failed=1; }
baseline 'a.rs big 12 2'
in_repo 0 'wrote' --update --accept 'b.rs::big'

# A content line that looks like a file header does not end the file's ranges.
new_repo
sed -i '5s/.*/-- x/' "$repo/a.rs"
git -C "$repo" commit -q -am dashes
git -C "$repo" branch -f base
sed -i -e '5s/.*/++ y/' -e '15s/.*/changed/' "$repo/a.rs"
baseline 'a.rs big 12 2' 'a.rs other 9 1'
measurements 'a.rs,big,12,2,3,9' 'a.rs,other,9,1,12,18'
in_repo 1 'TOUCHED a.rs::big'
if ! grep -q 'TOUCHED a.rs::other' <<< "$last_output"; then
  echo "FAIL: a content line starting with +++ hid the next hunk:"; echo "$last_output"; failed=1
fi

# The report names the entries newly marked flat and those no longer flat.
printf '# old\na.rs\tfmt\t20\t0\na.rs\tbig\t12\t2\tflat\nc.rs\tkept\t9\t1\tflat\nd.rs\tmv\t9\t1\tflat\n' > "$tmp/old"
baseline 'a.rs fmt 20 0 flat' 'a.rs big 12 2' 'c.rs kept 9 1 flat' 'e.rs mv 9 1 flat'
output=$(GITHUB_STEP_SUMMARY="$tmp/summary" python3 "$script" "$tmp/baseline" --compare-with "$tmp/old")
status=$?
for want in 'marked flat' 'a.rs. .fmt.' 'no longer flat' 'a.rs. .big.'; do
  if [ "$status" != 0 ] || ! grep -qE -- "$want" <<< "$output"; then
    echo "FAIL: the report wanted /$want/ and status 0, got $status:"; echo "$output"; failed=1
  fi
done
if grep -qE 'kept|e.rs' <<< "$(grep -E 'flat' <<< "$output")"; then
  echo "FAIL: an unchanged or moved flat entry was reported:"; echo "$output"; failed=1
fi
grep -q 'marked flat' "$tmp/summary" || { echo "FAIL: flat marks missing from the summary"; failed=1; }

if [ "$failed" = 0 ]; then echo "complexity: ok"; fi
exit "$failed"
