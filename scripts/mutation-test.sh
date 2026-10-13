#!/usr/bin/env bash
# Break each guard the program's safety rests on, one at a time, and check
# that the host tests notice. A guard no test has seen fail is not a guard.
#
# Each mutant is an exact text replacement in programs/accord/core/src/lib.rs
# that must match exactly once; the file is restored after every mutant (and
# on exit). A mutant SURVIVES if `cargo test -p accord-core` still passes:
# that is a missing test, and the script exits non-zero.
#
#   scripts/mutation-test.sh            # all mutants
#   scripts/mutation-test.sh threshold  # those whose name contains "threshold"
set -uo pipefail
cd "$(dirname "$0")/.."
SRC=programs/accord/core/src/lib.rs
BACKUP="$(mktemp)"; cp "$SRC" "$BACKUP"
trap 'cp "$BACKUP" "$SRC"; rm -f "$BACKUP"' EXIT

# Mutants, one per line in the here-document closing the loop: name ||| find ||| replace
base="$(cargo test -q --manifest-path programs/accord/Cargo.toml -p accord-core 2>&1)"
printf '%s' "$base" | grep -q "test result: ok" && ! printf '%s' "$base" | grep -q "test result: FAILED" \
  || { echo "the unmutated tests do not pass: fix them first" >&2; exit 2; }
echo "baseline: the unmutated host tests pass"
killed=0; survived=0; skipped=0
while IFS= read -r line <&3; do
  [ -n "$line" ] || continue
  name="${line%% ||| *}"; rest="${line#* ||| }"; find="${rest%% ||| *}"; repl="${rest#* ||| }"
  case "$name" in *"${1:-}"*) ;; *) continue ;; esac
  cp "$BACKUP" "$SRC"
  if ! python3 - "$SRC" "$find" "$repl" <<'PY'
import sys
p, f, r = sys.argv[1:4]
s = open(p).read()
n = s.count(f)
if n != 1:
    print(f"  find text matches {n} times", file=sys.stderr); sys.exit(1)
open(p, "w").write(s.replace(f, r))
PY
  then echo "SKIP     $name"; skipped=$((skipped + 1)); continue; fi
  out="$(cargo test -q --manifest-path programs/accord/Cargo.toml -p accord-core 2>&1)"
  if printf '%s' "$out" | grep -q "test result: FAILED"; then
    failing="$(printf '%s\n' "$out" | sed -n 's/^    \(tests::[a-z_0-9]*\)$/\1/p' | sed 's/^tests:://' | sort -u | tr '\n' ' ')"
    echo "killed   $name  <- ${failing:-a test}"
    killed=$((killed + 1))
  elif printf '%s' "$out" | grep -q "error\[E"; then
    echo "SKIP     $name (does not compile)"; skipped=$((skipped + 1))
  else
    echo "SURVIVED $name"; survived=$((survived + 1))
  fi
done 3<<'EOF'
threshold: queue accepts one weight short ||| refuse_unless(w >= m.threshold, E_BELOW_THRESHOLD, "approvals are below the threshold"); ||| refuse_unless(w + 1 >= m.threshold, E_BELOW_THRESHOLD, "approvals are below the threshold");
threshold: activation accepts one weight short ||| refuse_unless(w >= m.threshold, E_BELOW_THRESHOLD, "the activation is below the threshold"); ||| refuse_unless(w + 1 >= m.threshold, E_BELOW_THRESHOLD, "the activation is below the threshold");
threshold: spend accepts one signer short ||| refuse_unless(approvals.len() >= usize::from(l.spec.min_signers), ||| refuse_unless(approvals.len() + 1 >= usize::from(l.spec.min_signers),
replay: a digest can be queued twice ||| refuse_unless(pre.is_empty(), E_EXISTS, "this action is already queued"); ||| refuse_unless(true, E_EXISTS, "this action is already queued");
replay: an executed action can execute again ||| refuse_unless(q.digest == digest && q.status == STATUS_QUEUED, E_NOT_QUEUED, "the action is not pending"); ||| refuse_unless(q.digest == digest, E_NOT_QUEUED, "the action is not pending");
replay: a spend signature is accepted at any counter ||| refuse_unless(l.uses == uses, E_BAD_LIMIT, ||| refuse_unless(true, E_BAD_LIMIT,
replay: the spend counter does not advance ||| slot.uses += 1; ||| slot.uses += 0;
replay: approvals from another epoch count ||| refuse_unless(epoch == m.epoch, E_WRONG_EPOCH, "the approvals were made under another configuration"); ||| refuse_unless(true, E_WRONG_EPOCH, "the approvals were made under another configuration");
expiry: queue ignores the window's end ||| refuse_unless(at.saturating_add(WINDOW_MS) <= expires_at, E_EXPIRED, "the proposal expires before this transaction's window closes"); ||| refuse_unless(at <= expires_at.saturating_add(WINDOW_MS), E_EXPIRED, "the proposal expires before this transaction's window closes");
expiry: execute ignores expiry ||| refuse_unless(at.saturating_add(WINDOW_MS) <= q.expires_at, E_EXPIRED, ||| refuse_unless(true, E_EXPIRED,
expiry: time lock not enforced ||| exempt_time_lock || q.time_lock_ms == 0 || at >= executable_at(&q), ||| true,
expiry: time lock counted from the window's start ||| q.quorum_at.saturating_add(WINDOW_MS).saturating_add(q.time_lock_ms) ||| q.quorum_at.saturating_add(q.time_lock_ms)
membership: a non-member's signature counts ||| .unwrap_or_else(|| refuse(E_BAD_ROLE, "a signing key is not a current member")); ||| .unwrap_or(&m.members[0]);
membership: the role bit is not checked ||| refuse_unless(member.roles & role != 0, E_BAD_ROLE, ||| refuse_unless(true, E_BAD_ROLE,
membership: one member's two signatures count twice ||| refuse_unless(!approvals[..i].iter().any(|o| o.key == a.key), E_DUPLICATE, "a key signed twice"); ||| refuse_unless(true, E_DUPLICATE, "a key signed twice");
membership: a non-spender's signature counts ||| refuse_unless(l.spec.spenders.contains(&a.key), E_BAD_ROLE, ||| refuse_unless(true, E_BAD_ROLE,
signature: an invalid signature counts ||| refuse_unless(verify(&a.key, message, &a.sig), E_BAD_SIGNATURE, ||| refuse_unless(true, E_BAD_SIGNATURE,
EOF
cp "$BACKUP" "$SRC"
echo "summary: $killed killed, $survived survived, $skipped skipped"
[ "$survived" = 0 ] && [ "$skipped" = 0 ]
