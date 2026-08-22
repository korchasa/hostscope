#!/bin/bash
# Build, ship and run the live check as one command.
#
# What this replaces: `ssh <host> mkdir`, then `scp` of the binary and the
# scripts, then `ssh <host> bash host-check.sh`, then a read of whatever
# scrolled by. Three round trips with a wait and a decision between each, and
# the transcripts of this project counted 130 of those trips against 11 full
# runs. The work was never the expensive part.
#
# usage: live-check.sh [--bg|--log|--ship-only] [section ...]
#
#   (no flag)     build, ship, run, print the summary, keep the full log
#   --bg          the same, detached on the host; returns as soon as it starts
#   --log         print the summary of the run that is on the host now
#   --ship-only   build and ship, run nothing
#
# HS_HOST and HS_DIR override the host and the directory on it.

set -u
# Every path below is relative to the root of the repository, so the command
# works from wherever it is called.
cd "$(dirname "$0")/.." || exit 1
# No default host: a check that silently goes to the wrong machine is worse
# than one that stops and asks.
HOST=${HS_HOST:?set it: make live HOST=my.host, or HOST = my.host in local.mk}
DIR=${HS_DIR:-/tmp/hostscope}
TARGET=x86_64-unknown-linux-musl
BIN=target/$TARGET/release/hostscope
LOGDIR=target/live
LOG=$LOGDIR/host-check.log
REMOTE_LOG=$DIR/host-check.log

mode=run
case ${1:-} in
  --bg) mode=bg; shift ;;
  --log) mode=log; shift ;;
  --ship-only) mode=ship; shift ;;
esac
SECTIONS="$*"

mkdir -p "$LOGDIR"

# The summary is what a reader needs: the counters, every failure in full, and
# what each section cost. The rest of the log stays in the file - a full run
# prints some 200 lines, and reading them all to find two failures is how a
# four minute check turns into a fifteen minute round.
summarise() {
  local log=$1
  echo
  grep -E '^  FAIL' "$log" && echo
  grep -E '^  SKIP' "$log" && echo
  grep -E '\(.* took .* s\)' "$log" | sed 's/^ *//' | sort -t' ' -k3 -gr | head -8
  echo
  grep -E '^== summary' "$log" || echo "== no summary: the run did not reach the end =="
  echo "full log: $log"
}

if [ "$mode" = log ]; then
  scp -q "$HOST:$REMOTE_LOG" "$LOG" 2>/dev/null || { echo "no log on $HOST:$REMOTE_LOG"; exit 1; }
  if ssh -o BatchMode=yes "$HOST" 'pgrep -f host-check.sh >/dev/null'; then
    echo "== the run is still going on $HOST =="
  fi
  summarise "$LOG"
  exit 0
fi

echo "== build =="
cargo build --release --target "$TARGET" || exit 1

# One trip instead of two: `scp` cannot create the directory it writes into, so
# the pair `ssh mkdir` plus `scp` was always two. A tar over ssh does both.
echo "== ship to $HOST:$DIR =="
ROOT=$(pwd)
# --no-xattrs: without it the tar of macOS carries a provenance attribute that
# the tar on the host does not know, and every ship prints a warning about it.
tar --no-xattrs -cf - -C "$ROOT/target/$TARGET/release" hostscope \
           -C "$ROOT/scripts" host-check.sh oracle.py frame-lint.py model-query.py \
  | ssh -o BatchMode=yes "$HOST" "mkdir -p $DIR && tar -xf - -C $DIR && chmod +x $DIR/hostscope" \
  || { echo "ship failed"; exit 1; }

[ "$mode" = ship ] && { echo "shipped"; exit 0; }

if [ "$mode" = bg ]; then
  # Detached on the host, so the check and the work on the Mac go on at the
  # same time. The four minutes it takes are four minutes of waiting only when
  # the terminal is held open for them.
  ssh -o BatchMode=yes "$HOST" \
    "cd $DIR && nohup bash host-check.sh $SECTIONS > $REMOTE_LOG 2>&1 & echo started"
  echo "running on $HOST; collect it with: make live-log"
  exit 0
fi

echo "== check on $HOST =="
started=$(python3 -c 'import time; print(time.time())')
ssh -o BatchMode=yes "$HOST" "cd $DIR && bash host-check.sh $SECTIONS > $REMOTE_LOG 2>&1"
status=$?
scp -q "$HOST:$REMOTE_LOG" "$LOG" 2>/dev/null
python3 -c "import sys,time; print('the run took %.0f s' % (time.time() - float(sys.argv[1])))" "$started"
summarise "$LOG"
exit $status
