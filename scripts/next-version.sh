#!/bin/bash
# What the next release is called, decided by the commits since the last one.
#
# The release workflow runs this on every push to main. A `feat:` commit since
# the last tag moves the second number, a `fix:` moves the third, and anything
# else - documentation, a change to a script, a rewritten comment - releases
# nothing. That is the whole policy, chosen by the operator on 2026-09-01, and
# it lives here rather than inside the workflow so it can be run and argued
# with on a laptop.
#
# usage: next-version.sh [--self-test]
#
# Prints the version to release, "0.1.2", and prints nothing at all when the
# commits since the last tag ask for no release. Exits 1 with a sentence on
# stderr when the repository is in a state it will not guess about: no version
# tag to count from, a tag that is not a version, or a `Cargo.toml` that
# disagrees with the tag about what is already released.

set -u

# The type is read from the subject only. A `fix:` mentioned in the body of a
# `docs:` commit is prose about a fix, not one.
bump_for() {
  local subjects
  subjects=$(git log --format='%s' "$1") || return 1
  if printf '%s\n' "$subjects" | grep -qE '^feat(\([^)]*\))?!?: '; then
    echo minor
  elif printf '%s\n' "$subjects" | grep -qE '^fix(\([^)]*\))?!?: '; then
    echo patch
  fi
}

next_version() {
  local last base crate bump ma mi pa

  last=$(git describe --tags --abbrev=0 --match 'v*' 2>/dev/null)
  if [ -z "$last" ]; then
    echo "no v tag to count from: the first release is made by hand" >&2
    return 1
  fi

  base=${last#v}
  if ! printf '%s' "$base" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "the last tag is $last, which is not a version this can add to" >&2
    return 1
  fi

  # The two have to agree before anything is decided: `--version` prints what
  # `Cargo.toml` says, so a manifest left behind at the previous number would
  # ship a binary that misreports itself. This is the failure the tag guard in
  # the workflow used to catch after the fact.
  crate=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
  if [ "$crate" != "$base" ]; then
    echo "the last tag says $base and Cargo.toml says $crate" >&2
    return 1
  fi

  bump=$(bump_for "$last..HEAD") || return 1
  [ -z "$bump" ] && return 0

  IFS=. read -r ma mi pa <<<"$base"
  case "$bump" in
    minor)
      mi=$((mi + 1))
      pa=0
      ;;
    patch) pa=$((pa + 1)) ;;
  esac
  echo "$ma.$mi.$pa"
}

# A temporary repository with a tag and a few commits, so the policy above is
# checked where it is cheap rather than by cutting a release to see. The
# workflow runs this before it trusts the script.
self_test() {
  local me tmp out rc fails=0
  me=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN

  check() { # name, expected stdout, expected exit
    out=$("$me" 2>/dev/null)
    rc=$?
    if [ "$out" = "$2" ] && [ "$rc" = "$3" ]; then
      echo "  ok    $1"
    else
      echo "  FAIL  $1: got [$out] exit $rc, wanted [$2] exit $3"
      fails=$((fails + 1))
    fi
  }

  (
    cd "$tmp" || exit 1
    git init -q .
    git config user.email t@t
    git config user.name t
    printf 'version = "0.1.1"\n' >Cargo.toml
    git add Cargo.toml
    git commit -qm "feat: the first"
  ) || return 1

  cd "$tmp" || return 1

  check "no tag at all refuses" "" 1

  git tag -a v0.1.1 -m "hostscope 0.1.1"
  check "a tag and nothing after it releases nothing" "" 0

  git commit -q --allow-empty -m "docs: a paragraph"
  check "documentation alone releases nothing" "" 0

  git commit -q --allow-empty -m "fix: a defect"
  check "a fix moves the third number" "0.1.2" 0

  git commit -q --allow-empty -m "feat(cli): a flag"
  check "a feature moves the second and zeroes the third" "0.2.0" 0

  git commit -q --allow-empty -m "fix: another defect"
  check "a feature outranks a fix in the same range" "0.2.0" 0

  printf 'version = "0.9.9"\n' >Cargo.toml
  git commit -qam "chore: a manifest that ran ahead"
  check "a manifest disagreeing with the tag refuses" "" 1

  [ "$fails" = 0 ] || return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  "") next_version ;;
  *)
    echo "usage: next-version.sh [--self-test]" >&2
    exit 1
    ;;
esac
