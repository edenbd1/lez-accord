#!/usr/bin/env bash
# The private vault on a LEZ v0.3 sequencer: a private PDA of accord_multisig
# under the vault seed, keyed by a wallet group (Group Master Secret).
#
#   1. the vault is funded by a privacy-preserving transfer
#   2. a member who holds the group keys tries to move it directly, without
#      the multisig: the circuit refuses (UnauthorizedSender)
#   3. a spend within a limit moves it, through the program
#   4. an M-of-N action moves it: approvals collected off-chain, `queue` in
#      public, `execute` in a privacy-preserving transaction
#
# The program header is deployed immutable: the privacy circuit checks a
# private PDA's binding to its program against the header's mirror.
#
# Needs what scripts/e2e.sh needs. Writes $OUT (default evidence/v03/local-private.tsv).
set -uo pipefail
cd "$(dirname "$0")/.."
: "${LEE_WALLET_HOME_DIR:?}"; : "${PAYER:?}"
RPC="${RPC:-http://127.0.0.1:3040}"
export ACCORD_RPC="$RPC"
W="${WALLET:-wallet}"; CLI="${CLI:-cli/target/release/accord}"
OUT="${OUT:-evidence/v03/local-private.tsv}"; TAG="${TAG:-priv}"
export ACCORD_ELF="${ACCORD_ELF:-artifacts/programs/v0.3/accord_multisig.bin}"
export ACCORD_KEYS="${ACCORD_KEYS:-$(mktemp -d)/keys}"
PK="$(mktemp -d)"; mkdir -p "$(dirname "$OUT")"
GROUP="accord-$TAG-$(date +%s)"

line="$("$CLI" deploy --payer "$PAYER" --immutable 2>/dev/null | grep '^{' | tail -1)"; echo "$line"
ACCORD_PROGRAM="$(printf '%s' "$line" | python3 -c 'import json,sys;print(json.load(sys.stdin)["header"])')"
export ACCORD_PROGRAM
echo "# run against $RPC, $(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "$OUT"
echo "# program $ACCORD_PROGRAM $("$CLI" image-id)" | tee -a "$OUT"

jget() { python3 -c 'import json,sys
v=json.loads(sys.argv[1])
for k in sys.argv[2].split("."): v=v[int(k)] if k.isdigit() else v[k]
print(v)' "$1" "$2"; }
bal() { curl -s -m 20 -X POST "$RPC" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccountBalance\",\"params\":[\"$1\"]}" | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"])'; }
fail=0
check() { if [ "$2" = "$3" ]; then echo "  ok  $1: $2"; echo "# check ok: $1 = $2" >> "$OUT"
  else echo "  FAIL $1: got $2, want $3"; echo "# check FAIL: $1: $2 != $3" >> "$OUT"; fail=1; fi; }
step() {
  local label="$1" expect="$2"; shift 3
  local line verdict block tx
  line="$("$CLI" --label "$label" "$@" 2>/tmp/accord-priv.err | grep '^{' | tail -1)"
  [ -n "$line" ] || line="{\"verdict\":\"error\",\"error\":\"$(tail -1 /tmp/accord-priv.err | tr '"' "'")\"}"
  echo "$line" | cut -c1-400
  verdict="$(jget "$line" verdict)"; block="$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("block"))' "$line")"
  tx="$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("tx","-"))' "$line")"
  printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$expect" "$verdict" "$block" "$tx" >> "$OUT"
  case "$expect:$verdict" in
    applied:applied|refused:reverted|refused:rejected|refused:not-included) echo "  ok  $label: $verdict" ;;
    *) echo "  FAIL $label: $verdict, expected $expect"; fail=1 ;;
  esac
}
off() { "$CLI" "$@" 2>/dev/null | grep '^{' | tail -1; }
P=(--payer "$PAYER")
MS="$TAG"
DEST=$("$W" account new public 2>/dev/null | grep -oE 'Public/[A-Za-z0-9]+' | head -1 | cut -d/ -f2)
for k in alice bob carol; do "$CLI" key new "$k" >/dev/null; done

echo "== 1. a 2-of-3 whose vault is private"
step create applied -- "${P[@]}" create "$MS" --member alice:pa --member bob:a --member carol:ae --threshold 2 --limit bob:1:100:1d
step activate applied -- "${P[@]}" activate "$MS" --key alice --key carol
"$W" group new "$GROUP" >/dev/null 2>&1
VAULT="$(jget "$(off "${P[@]}" vault id "$MS" --group "$GROUP")" private_vault)"
echo "# payer $PAYER dest $DEST multisig $MS private-vault $VAULT group $GROUP" >> "$OUT"
step fund-private-vault applied -- "${P[@]}" vault fund "$MS" --group "$GROUP" --from "$PAYER" --amount 500
check "the group's member sees the private vault's balance" "$(jget "$(off "${P[@]}" vault track "$MS" --group "$GROUP")" balance_seen_by_this_member)" 500
check "the vault's balance is not public" "$(bal "$VAULT")" 0

echo "== 2. the group keys alone cannot spend it"
step steal-with-group-keys refused -- "${P[@]}" vault steal "$MS" --group "$GROUP" --to "$DEST" --amount 100
check "nothing moved" "$(bal "$DEST")" 0

echo "== 3. a spend within the limit, through the program, proved locally"
step spend-private applied -- "${P[@]}" spend "$MS" --to "$DEST" --amount 60 --key bob --private-vault "$GROUP"
check "destination received 60" "$(bal "$DEST")" 60

echo "== 4. an M-of-N action on the private vault"
off propose "$MS" --transfer "$DEST:100" --from "$VAULT" --key alice --out "$PK/pay.json" >/dev/null
off approve "$PK/pay.json" --key bob >/dev/null; off approve "$PK/pay.json" --key carol >/dev/null
step queue applied -- "${P[@]}" queue "$PK/pay.json"
step execute-private applied -- "${P[@]}" execute "$PK/pay.json" --key carol --private-vault "$GROUP"
check "destination received 100 more" "$(bal "$DEST")" 160
check "the members' view of the vault" "$(jget "$(off "${P[@]}" vault track "$MS" --group "$GROUP")" balance_seen_by_this_member)" 340
check "and still nothing public about it" "$(bal "$VAULT")" 0

python3 scripts/reresolve.py "$RPC" "$OUT"
applied=$(awk -F'\t' '$3=="applied"' "$OUT" | wc -l | tr -d ' ')
refused=$(awk -F'\t' '$2=="refused" && ($3=="reverted"||$3=="rejected"||$3=="not-included")' "$OUT" | wc -l | tr -d ' ')
echo "summary: $applied applied, $refused refused as expected, $(grep -c '^# check ok' "$OUT") state checks ok, failures=$fail"
exit $fail
