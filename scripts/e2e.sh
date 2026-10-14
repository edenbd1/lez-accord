#!/usr/bin/env bash
# Drive the whole accord_multisig lifecycle against a LEZ v0.3 sequencer.
#
# Written for a local standalone sequencer (see docs/LOCAL.md), where the
# genesis accounts are funded; pointed at the public testnet it is the same run
# once PAYER holds LGO.
#
# Every step prints the CLI's JSON line and is checked against what it should
# do: `applied` where the program must accept, `reverted` (included, and
# charged if public, with no effect) or `rejected`/`not-included` where it
# must refuse. State is read back from the chain around the steps. Approvals
# travel as approval packages (files here; the room carries the same bytes).
#
# Needs: the v0.3 `wallet` and `accord` binaries, LEE_WALLET_HOME_DIR with
# PAYER imported, and the program deployed (ACCORD_PROGRAM) or DEPLOY=1.
#
# Writes $OUT (default evidence/v03/local-e2e.tsv): label expected verdict block tx
set -uo pipefail
cd "$(dirname "$0")/.."

: "${LEE_WALLET_HOME_DIR:?set LEE_WALLET_HOME_DIR to the v0.3 wallet home}"
: "${PAYER:?base58 id of a funded key that pays every fee}"
RPC="${RPC:-http://127.0.0.1:3040}"
export ACCORD_RPC="$RPC"
W="${WALLET:-wallet}"
CLI="${CLI:-cli/target/release/accord}"
OUT="${OUT:-evidence/v03/local-e2e.tsv}"
TAG="${TAG:-e2e}"
LOCK="${LOCK:-45s}"
LOCK_S="${LOCK_S:-665}"
export ACCORD_ELF="${ACCORD_ELF:-artifacts/programs/v0.3/accord_multisig.bin}"
export ACCORD_KEYS="${ACCORD_KEYS:-$(mktemp -d)/keys}"
PK="$(mktemp -d)"
mkdir -p "$(dirname "$OUT")"

if [ "${DEPLOY:-0}" = 1 ]; then
  line="$("$CLI" deploy --payer "$PAYER" | grep '^{' | tail -1)"
  echo "$line"
  ACCORD_PROGRAM="$(printf '%s' "$line" | python3 -c 'import json,sys;print(json.load(sys.stdin)["header"])')"
fi
: "${ACCORD_PROGRAM:?deploy first (DEPLOY=1) or set ACCORD_PROGRAM}"
export ACCORD_PROGRAM
echo "# run against $RPC, $(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "$OUT"
echo "# program $ACCORD_PROGRAM $("$CLI" image-id)" | tee -a "$OUT"

new_pub() { "$W" account new public 2>/dev/null | grep -oE 'Public/[A-Za-z0-9]+' | head -1 | cut -d/ -f2; }
rpc() { curl -s -m 20 -X POST "$RPC" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":[\"$2\"]}"; }
shard() { # account program -> hex of that shard
  rpc getAccount "$1" | python3 -c '
import json,sys
r=json.load(sys.stdin).get("result") or {}
print(bytes((r.get("data") or {}).get("shards",{}).get(sys.argv[1],[])).hex())' "$2"; }
NATIVE=11111111111111111111111111111111
bal() { python3 -c 'import sys;h=sys.argv[1];print(int.from_bytes(bytes.fromhex(h),"little") if h else 0)' "$(shard "$1" $NATIVE)"; }
jget() { python3 -c 'import json,sys
v=json.loads(sys.argv[1])
for k in sys.argv[2].split("."): v=v[int(k)] if k.isdigit() else v[k]
print(v)' "$1" "$2"; }
show() { "$CLI" show "$MS"; }
FINAL=()

fail=0
check() { # label got want
  if [ "$2" = "$3" ]; then echo "  ok  $1: $2"; echo "# check ok: $1 = $2" >> "$OUT"
  else echo "  FAIL $1: got $2, want $3"; echo "# check FAIL: $1: $2 != $3" >> "$OUT"; fail=1; fi
}
# step label expected(applied|refused) -- cli args...
step() {
  local label="$1" expect="$2"; shift 3
  local line verdict block tx
  line="$("$CLI" --label "$label" "$@" 2>/tmp/accord-e2e.err | grep '^{' | tail -1)"
  [ -n "$line" ] || line="{\"verdict\":\"error\",\"error\":\"$(tail -1 /tmp/accord-e2e.err | tr '"' "'")\"}"
  echo "$line"
  verdict="$(jget "$line" verdict)"; block="$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("block"))' "$line")"
  tx="$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("tx","-"))' "$line")"
  printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$expect" "$verdict" "$block" "$tx" >> "$OUT"
  case "$expect:$verdict" in
    applied:applied|refused:reverted|refused:rejected|refused:not-included) echo "  ok  $label: $verdict" ;;
    *) echo "  FAIL $label: $verdict, expected $expect"; fail=1 ;;
  esac
}
# A client-side step that must succeed (propose/approve: no transaction).
off() { "$CLI" "$@" | grep '^{' | tail -1; }

P=(--payer "$PAYER")
MS="$TAG"
DEST=$(new_pub); DEST2=$(new_pub)
for k in alice bob carol dave eve; do "$CLI" key new "$k" >/dev/null; done
echo "# payer $PAYER dest $DEST dest2 $DEST2 multisig $MS" >> "$OUT"
J="$("$CLI" ids "$MS")"; CFG="$(jget "$J" configuration)"; VAULT="$(jget "$J" vault)"
FINAL+=("$CFG $ACCORD_PROGRAM" "$VAULT $NATIVE")

echo "== 1. create a 2-of-3 with a $LOCK time lock and a spending limit; fund its vault"
step create applied -- "${P[@]}" create "$MS" --member alice:pa --member bob:a --member carol:ae \
  --threshold 2 --time-lock "$LOCK" --limit bob:1:100:1d
"$W" auth-transfer send --from "Public/$PAYER" --to "Public/$VAULT" --amount 1000 2>&1 | grep -E "hash" | sed 's/^/  fund vault: /'
check "vault funded before activation" "$(bal "$VAULT")" 1000
check "not initialised yet" "$(jget "$(show)" initialised)" False

echo "== 2. a vault cannot be drained before it is fully initialised"
off propose "$MS" --transfer "$DEST:10" --key alice --out "$PK/early.json" >/dev/null
off approve "$PK/early.json" --key bob >/dev/null; off approve "$PK/early.json" --key carol >/dev/null
step queue-before-activation refused -- "${P[@]}" --force queue "$PK/early.json"
step spend-before-activation refused -- "${P[@]}" spend "$MS" --to "$DEST" --amount 10 --key bob
step activate-below-threshold refused -- "${P[@]}" activate "$MS" --key alice
step activate applied -- "${P[@]}" activate "$MS" --key alice --key carol
check "vault untouched" "$(bal "$VAULT")" 1000

echo "== 3. propose, approve off-chain, queue; the time lock holds"
off propose "$MS" --transfer "$DEST:40" --key alice --description "pay the auditor's deposit" --out "$PK/pay40.json" >/dev/null
check "the package decodes from its bytes" "$(jget "$(off decode "$PK/pay40.json")" action.calls.0.decoded.amount)" 40
off approve "$PK/pay40.json" --key bob >/dev/null
step queue-one-approval refused -- "${P[@]}" --force queue "$PK/pay40.json"
off approve "$PK/pay40.json" --key carol >/dev/null
D40="$(jget "$(cat "$PK/pay40.json")" digest)"
step queue applied -- "${P[@]}" queue "$PK/pay40.json"
FINAL+=("$(jget "$("$CLI" ids "$MS" --digest "$D40")" queued) $ACCORD_PROGRAM")
step execute-in-time-lock refused -- "${P[@]}" --force execute "$PK/pay40.json" --key carol
step execute-by-non-executor refused -- "${P[@]}" --force execute "$PK/pay40.json" --key alice

echo "== 4. spending limit: small transfers without the quorum"
step spend-60 applied -- "${P[@]}" spend "$MS" --to "$DEST2" --amount 60 --key bob
step spend-over-limit refused -- "${P[@]}" spend "$MS" --to "$DEST2" --amount 50 --key bob
step spend-by-non-spender refused -- "${P[@]}" spend "$MS" --to "$DEST2" --amount 1 --key alice
check "spend paid 60" "$(bal "$DEST2")" 60

echo "== 5. an action naming the fee payer is queued (it will be refused at execution)"
python3 - "$PAYER" "$DEST" > "$PK/steal-calls.json" <<'PY'
import json, sys
# native Transfer { amount: 5 } from the fee payer: borsh = variant 0, u128 LE
data = "00" + (5).to_bytes(16, "little").hex()
N = "11111111111111111111111111111111"
print(json.dumps([{"program": N, "accounts": [[sys.argv[1], N], [sys.argv[2], N]], "data": data}]))
PY
off propose "$MS" --calls "$PK/steal-calls.json" --key alice --out "$PK/steal.json" >/dev/null
off approve "$PK/steal.json" --key bob >/dev/null; off approve "$PK/steal.json" --key carol >/dev/null
step queue-steal applied -- "${P[@]}" queue "$PK/steal.json"

echo "== 6. a configuration change, queued; then the wait, the executions, and its effect"
# a package approved under epoch 0, held back
off propose "$MS" --transfer "$DEST:5" --key alice --out "$PK/stale.json" >/dev/null
off approve "$PK/stale.json" --key bob >/dev/null; off approve "$PK/stale.json" --key carol >/dev/null
cat > "$PK/rotate.json" <<EOF
{"members": ["alice:pa", "carol:ae", "dave:a"], "threshold": 2, "time_lock": "$LOCK"}
EOF
off propose "$MS" --config "$PK/rotate.json" --key alice --out "$PK/rotate-pkg.json" >/dev/null
off approve "$PK/rotate-pkg.json" --key alice >/dev/null
step queue-config-one-approval refused -- "${P[@]}" --force queue "$PK/rotate-pkg.json"
off approve "$PK/rotate-pkg.json" --key carol >/dev/null
step queue-config applied -- "${P[@]}" queue "$PK/rotate-pkg.json"

# One wait covers the three queued actions above. The lock counts from the
# end of the queue's window (DESIGN.md §5), so it is never shorter than set.
echo "  waiting ${LOCK_S}s for the time lock"; sleep "$LOCK_S"
step execute applied -- "${P[@]}" execute "$PK/pay40.json" --key carol
check "destination received 40" "$(bal "$DEST")" 40
check "vault paid 60 (spend) and 40" "$(bal "$VAULT")" 900
check "queued record executed" "$(jget "$(off queued "$MS" "$D40")" status)" executed
step queue-again refused -- "${P[@]}" --force queue "$PK/pay40.json"
step execute-again refused -- "${P[@]}" --force execute "$PK/pay40.json" --key carol
step execute-steal refused -- "${P[@]}" --force execute "$PK/steal.json" --key carol
step execute-config applied -- "${P[@]}" execute "$PK/rotate-pkg.json" --key carol
check "epoch moved" "$(jget "$(show)" epoch)" 1
check "bob is no longer a member" "$(show | python3 -c 'import json,sys;print(len(json.load(sys.stdin)["members"]))')" 3
step queue-stale-approvals refused -- "${P[@]}" --force queue "$PK/stale.json"
off propose "$MS" --transfer "$DEST:5" --key alice --out "$PK/removed.json" >/dev/null
off approve "$PK/removed.json" --key alice >/dev/null; off approve "$PK/removed.json" --key bob >/dev/null
step queue-removed-member refused -- "${P[@]}" --force queue "$PK/removed.json"

echo "== 7. veto: the new quorum cancels a queued action, skipping the time lock"
off propose "$MS" --transfer "$DEST:300" --key alice --out "$PK/big.json" >/dev/null
off approve "$PK/big.json" --key alice >/dev/null; off approve "$PK/big.json" --key dave >/dev/null
step queue-big applied -- "${P[@]}" queue "$PK/big.json"
DBIG="$(jget "$(cat "$PK/big.json")" digest)"
off propose "$MS" --cancel "$DBIG" --key alice --out "$PK/veto.json" >/dev/null
off approve "$PK/veto.json" --key alice >/dev/null; off approve "$PK/veto.json" --key dave >/dev/null
step queue-veto applied -- "${P[@]}" queue "$PK/veto.json"
step execute-veto applied -- "${P[@]}" execute "$PK/veto.json" --key carol
check "big transfer cancelled" "$(jget "$(off queued "$MS" "$DBIG")" status)" cancelled
step execute-vetoed refused -- "${P[@]}" --force execute "$PK/big.json" --key carol
check "vault after everything" "$(bal "$VAULT")" 900

echo "== 8. registry: append-only"
echo "{\"name\":\"token\",\"version\":\"v0.3.0\",\"run\":\"$TAG\"}" > "$PK/token.idl.json"
TOKEN=AxDdLwqkWgR9qaSctvABV1ZtvWJJyzXB8199xZueifPj
IMG=0000000000000000000000000000000000000000000000000000000000000000
step register applied -- "${P[@]}" register "$TOKEN" --idl "$PK/token.idl.json" --name token --image-id "$IMG"
step register-again refused -- "${P[@]}" register "$TOKEN" --idl "$PK/token.idl.json" --name token --image-id "$IMG"

echo "== 9. Pr.1: no transaction of this run was signed by anyone but the fee payer"
check "signers across the run" "$(python3 scripts/signers.py "$RPC" "$OUT")" "$PAYER"

FINAL+=("$(jget "$("$CLI" ids "$MS" --digest "$DBIG")" queued) $ACCORD_PROGRAM")
for f in "${FINAL[@]}"; do
  set -- $f
  echo "# final $1 $2 $(shard "$1" "$2" | python3 -c 'import hashlib,sys;print(hashlib.sha256(bytes.fromhex(sys.stdin.read().strip())).hexdigest())')" >> "$OUT"
done

python3 scripts/reresolve.py "$RPC" "$OUT"
applied=$(awk -F'\t' '$3=="applied"' "$OUT" | wc -l | tr -d ' ')
refused=$(awk -F'\t' '$2=="refused" && ($3=="reverted"||$3=="rejected"||$3=="not-included")' "$OUT" | wc -l | tr -d ' ')
checks=$(grep -c '^# check ok' "$OUT")
echo "summary: $applied applied, $refused refused as expected, $checks state checks ok, failures=$fail"
exit $fail
