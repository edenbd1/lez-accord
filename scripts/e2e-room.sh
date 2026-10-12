#!/usr/bin/env bash
# Two members coordinate over Logos Delivery, then one executes on chain.
#
# Three approval keys live in three separate key directories, as on three
# machines. Carol opens the room and seals an invite to each member; Bob joins
# with his own key. Carol listens on her own Delivery node; Alice and Bob each
# bring up their own node and post: Alice the proposal, Bob his approval of
# it. Carol's node hears both frames, checks each sender against the
# configuration on chain, merges the approvals, and Carol queues and executes
# the package she received, never one she was handed out of band.
#
# Needs: a local v0.3 sequencer and wallet (docs/LOCAL.md), the program
# deployed (ACCORD_PROGRAM), ACCORD_DELIVERY_LIB (docs/ROOM.md), network
# access to the logos.dev Delivery fleet. Writes $OUT (evidence/v03/local-room.tsv).
set -uo pipefail
cd "$(dirname "$0")/.."
: "${LEE_WALLET_HOME_DIR:?}"; : "${PAYER:?}"; : "${ACCORD_PROGRAM:?}"; : "${ACCORD_DELIVERY_LIB:?}"
export ACCORD_PROGRAM ACCORD_DELIVERY_LIB
RPC="${RPC:-http://127.0.0.1:3040}"; export ACCORD_RPC="$RPC"
W="${WALLET:-wallet}"; CLI="${CLI:-cli/target/release/accord}"
OUT="${OUT:-evidence/v03/local-room.tsv}"; TAG="${TAG:-room}"
D="$(mktemp -d)"; mkdir -p "$(dirname "$OUT")"
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
as() { local who="$1"; shift; ACCORD_KEYS="$D/$who" "$CLI" "$@" 2>/dev/null | grep '^{' | tail -1; }
step() {
  local label="$1" expect="$2" who="$3"; shift 4
  local line verdict block tx
  line="$(ACCORD_KEYS="$D/$who" "$CLI" --label "$label" "$@" 2>/dev/null | grep '^{' | tail -1)"
  [ -n "$line" ] || line='{"verdict":"error"}'
  echo "$line"
  verdict="$(jget "$line" verdict)"; block="$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("block"))' "$line")"
  tx="$(python3 -c 'import json,sys;print(json.loads(sys.argv[1]).get("tx","-"))' "$line")"
  printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$expect" "$verdict" "$block" "$tx" >> "$OUT"
  case "$expect:$verdict" in applied:applied) echo "  ok  $label: $verdict" ;; *) echo "  FAIL $label: $verdict"; fail=1 ;; esac
}
P=(--payer "$PAYER")
MS="$TAG"
DEST=$("$W" account new public 2>/dev/null | grep -oE 'Public/[A-Za-z0-9]+' | head -1 | cut -d/ -f2)
for k in alice bob carol; do mkdir -p "$D/$k"; ACCORD_KEYS="$D/$k" "$CLI" key new "$k" >/dev/null; done
KA=$(jget "$(as alice key show alice)" approval_key); KB=$(jget "$(as bob key show bob)" approval_key); KC=$(jget "$(as carol key show carol)" approval_key)
echo "# payer $PAYER dest $DEST multisig $MS alice $KA bob $KB carol $KC" >> "$OUT"

echo "== 1. a 2-of-3; three members, three key directories"
step create applied carol -- "${P[@]}" create "$MS" --member "$KA:pa" --member "$KB:a" --member "$KC:ae" --threshold 2
VAULT=$(jget "$(as carol ids "$MS")" vault)
"$W" auth-transfer send --from "Public/$PAYER" --to "Public/$VAULT" --amount 1000 >/dev/null 2>&1
# activation signatures are collected like any approval; here two members sign locally
ACT=$(mktemp -d); cp "$D/alice/alice.key" "$D/carol/carol.key" "$ACT/"
step activate applied carol -- "${P[@]}" --keys "$ACT" activate "$MS" --key alice --key carol
rm -rf "$ACT"

echo "== 2. the room: Carol opens it, Bob joins with his own key"
as carol room open "$MS" --key carol --out "$D/carol.room" --invites "$D/invites.json" > "$D/open.json"; cat "$D/open.json"
as bob room join "$MS" --key bob --out "$D/bob.room" --invites "$D/invites.json"
as alice room join "$MS" --key alice --out "$D/alice.room" --invites "$D/invites.json" >/dev/null
check "Bob's room is Carol's room" "$(jget "$(cat "$D/bob.room")" secret)" "$(jget "$(cat "$D/carol.room")" secret)"

echo "== 3. Carol listens on her own Delivery node"
ACCORD_KEYS="$D/carol" "$CLI" room listen "$D/carol.room" --into "$D/carol-inbox" --seconds 420 --until-approvals 1 \
  > "$D/listen.out" 2>/dev/null &
LISTENER=$!
sleep 45

echo "== 4. Alice proposes over her node; Bob approves over his"
as alice propose "$MS" --transfer "$DEST:70" --key alice --description "pay the auditor" --out "$D/pay.json"
as alice room post "$D/alice.room" --key alice --package "$D/pay.json" --linger 20
cp "$D/pay.json" "$D/bob-copy.json"
as bob decode "$D/bob-copy.json" >/dev/null
as bob approve "$D/bob-copy.json" --key bob
as bob room post "$D/bob.room" --key bob --say "checked the invoice, approving" --linger 5
as bob room post "$D/bob.room" --key bob --package "$D/bob-copy.json" --linger 20
wait "$LISTENER"
grep '^{' "$D/listen.out" | tee -a /dev/stderr | sed 's/^/# heard: /' >> "$OUT"
DIG=$(jget "$(cat "$D/pay.json")" digest)
PKG="$D/carol-inbox/$DIG.json"
check "Carol's node heard the package" "$(test -f "$PKG" && echo yes)" yes
check "Carol's node heard Bob's deliberation" "$(grep -c 'room-chat' "$D/listen.out")" 1
check "the package Carol holds carries Alice's proposal and Bob's approval" \
  "$(python3 -c 'import json,sys;p=json.load(open(sys.argv[1]));print(len(p["approvals"]), p["proposer"] is not None)' "$PKG" 2>/dev/null)" "1 True"

echo "== 5. Carol approves the package she received, queues and executes it"
as carol approve "$PKG" --key carol
step queue applied carol -- "${P[@]}" queue "$PKG"
step execute applied carol -- "${P[@]}" execute "$PKG" --key carol
check "destination received 70" "$(bal "$DEST")" 70
echo "summary: $(awk -F'\t' '$3=="applied"' "$OUT" | wc -l | tr -d ' ') applied, $(grep -c '^# check ok' "$OUT") checks ok, failures=$fail"
exit $fail
