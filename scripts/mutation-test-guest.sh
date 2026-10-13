#!/usr/bin/env bash
# One mutant per guard class, built into the real guest binary and run through
# the executor tests (lee::V03State). Restores the source and rebuilds the guest
# at the end, and checks the rebuild is the committed binary again.
#   scripts/mutation-test-guest.sh   (needs the local RISC Zero toolchain, see build-guest.sh)
set -uo pipefail
cd "$(dirname "$0")/.."
SRC=programs/accord/core/src/lib.rs
ORIG="$(mktemp)"; COMMITTED="$(mktemp)"
cp artifacts/programs/v0.3/accord_multisig.bin "$COMMITTED"
trap 'cp "$ORIG" "$SRC"; cp "$COMMITTED" artifacts/programs/v0.3/accord_multisig.bin' EXIT
cp $SRC "$ORIG"
mut() { python3 -c '
import sys;p,f,r=sys.argv[1:4];s=open(p).read();assert s.count(f)==1,f;open(p,"w").write(s.replace(f,r))' $SRC "$2" "$3"
  scripts/build-guest.sh >/dev/null 2>&1 || { echo "BUILD FAIL $1"; return; }
  out=$(cargo test --release --manifest-path executor-tests/Cargo.toml 2>&1)
  res=$(printf '%s\n' "$out" | grep -E "^test result: (ok|FAILED). [1-9]" | head -1)
  fails=$(printf '%s\n' "$out" | sed -n 's/^test \([a-z_0-9]*\) \.\.\. FAILED$/\1/p' | tr '\n' ' ')
  case "$res" in *FAILED*) echo "killed   $1  <- $fails";; *) echo "SURVIVED $1 ($res)";; esac
  cp "$ORIG" $SRC
}
mut "threshold: queue accepts one weight short" 'refuse_unless(w >= m.threshold, E_BELOW_THRESHOLD, "approvals are below the threshold");' 'refuse_unless(w + 1 >= m.threshold, E_BELOW_THRESHOLD, "approvals are below the threshold");'
mut "replay: an executed action can execute again" 'refuse_unless(q.digest == digest && q.status == STATUS_QUEUED, E_NOT_QUEUED, "the action is not pending");' 'refuse_unless(q.digest == digest, E_NOT_QUEUED, "the action is not pending");'
mut "replay: a spend signature is accepted at any counter" 'refuse_unless(l.uses == uses, E_BAD_LIMIT,' 'refuse_unless(true, E_BAD_LIMIT,'
mut "expiry: queue ignores the window's end" 'refuse_unless(at.saturating_add(WINDOW_MS) <= expires_at, E_EXPIRED, "the proposal expires before this transaction'"'"'s window closes");' 'refuse_unless(at <= expires_at.saturating_add(WINDOW_MS), E_EXPIRED, "x");'
mut "membership: a non-member's signature counts" '.unwrap_or_else(|| refuse(E_BAD_ROLE, "a signing key is not a current member"));' '.unwrap_or(&m.members[0]);'
mut "membership: one member's two signatures count twice" 'refuse_unless(!approvals[..i].iter().any(|o| o.key == a.key), E_DUPLICATE, "a key signed twice");' 'refuse_unless(true, E_DUPLICATE, "a key signed twice");'
scripts/build-guest.sh >/dev/null 2>&1
cmp "$COMMITTED" artifacts/programs/v0.3/accord_multisig.bin && echo "restored: the rebuilt guest is the committed binary again"
res=$(cargo test --release --manifest-path executor-tests/Cargo.toml 2>&1 | grep -E "^test result: (ok|FAILED). [1-9]")
echo "baseline after restore: $res"
