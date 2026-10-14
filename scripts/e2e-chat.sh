#!/usr/bin/env bash
# The room on the Logos Chat module: three members coordinate in a chat-module
# group conversation (MLS, end-to-end encrypted, over Logos Delivery), then one
# of them queues and executes on chain what the room carried.
#
# Each member is a separate headless Logos Core daemon (`logosctl`, its own
# session directory) with chat_module 0.3.0 and its delivery_module installed
# from the official module catalog, as the chat module's own group doctest runs
# them; each member holds its own approval key in its own key directory. Alice proposes and posts the package; Bob and Carol each
# take it from the room, decode it from its bytes, approve it and post it back;
# Bob also posts a copy carrying a forged approval. Alice merges what she heard
# (the forged approval is dropped), and queues and executes it.
#
# Needs: a local v0.3 sequencer and wallet (docs/LOCAL.md), ACCORD_PROGRAM or
# DEPLOY=1, CHAT_TOOLS: a directory holding the released logosctl
# (scripts/chat-tools.sh fetches it), and network access to the module catalog
# and a Delivery fleet (logos.dev by default; ACCORD_CHAT_PRESET picks another).
# Writes $OUT (evidence/v03/local-chat.tsv) and prints a transcript.
set -uo pipefail
cd "$(dirname "$0")/.."
: "${LEE_WALLET_HOME_DIR:?}"; : "${PAYER:?}"; : "${CHAT_TOOLS:?a directory made by scripts/chat-tools.sh}"
RPC="${RPC:-http://127.0.0.1:3040}"; export ACCORD_RPC="$RPC"
W="${WALLET:-wallet}"; CLI="${CLI:-cli/target/release/accord}"
OUT="${OUT:-evidence/v03/local-chat.tsv}"; TAG="${TAG:-chat-$(date +%s)}"
PRESET="${ACCORD_CHAT_PRESET:-logos.dev}"
LC="$CHAT_TOOLS/logosctl-bin"
export ACCORD_ELF="${ACCORD_ELF:-artifacts/programs/v0.3/accord_multisig.bin}"
D="$(mktemp -d)"; mkdir -p "$(dirname "$OUT")"
[ -x "$LC" ] || { echo "CHAT_TOOLS=$CHAT_TOOLS lacks logosctl-bin (scripts/chat-tools.sh)" >&2; exit 2; }

if [ "${DEPLOY:-0}" = 1 ]; then
  line="$("$CLI" deploy --payer "$PAYER" | grep '^{' | tail -1)"; echo "$line"
  ACCORD_PROGRAM="$(printf '%s' "$line" | python3 -c 'import json,sys;print(json.load(sys.stdin)["header"])')"
fi
: "${ACCORD_PROGRAM:?deploy first (DEPLOY=1) or set ACCORD_PROGRAM}"; export ACCORD_PROGRAM

echo "# run against $RPC, room on chat_module ($PRESET), $(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "$OUT"
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
# chat <member> <method> args...  (one logoscore call on that member's daemon)
chat() { local who="$1"; shift; "$LC" --config-dir "$D/lc-$who" call chat_module "$@" 2>/dev/null; }
stop_all() { for w in alice bob carol; do "$LC" --config-dir "$D/lc-$w" daemon stop >/dev/null 2>&1 || true; done; }
trap stop_all EXIT
# send <member> <group> <file>: retried, a group refuses sends while it commits a membership change
send() { local i out; for i in $(seq 1 30); do
  out="$(chat "$1" send_message "$2" "@$3")"
  if printf '%s' "$out" | python3 -c 'import json,sys;sys.exit(0 if json.load(sys.stdin)["result"]["success"] else 1)' 2>/dev/null; then
    echo "  sent by $1: $(wc -c < "$3" | tr -d ' ') bytes"; return 0; fi
  sleep 5; done; echo "  FAIL $1 could not send: $out"; fail=1; return 1; }
# hear <member> <group> <sender address> <python predicate on content> <out file>: poll get_messages
hear() { local i; for i in $(seq 1 60); do
  chat "$1" get_messages "$2" | python3 -c '
import json,sys
sender, pred, out = sys.argv[1:4]
try: msgs = json.load(sys.stdin)["result"]
except Exception: sys.exit(1)
for m in msgs:
    c = m.get("content", "")
    if m.get("sender") == sender and eval(pred, {"c": c, "json": json}):
        open(out, "w").write(c); sys.exit(0)
sys.exit(1)' "$3" "$4" "$5" && return 0
  sleep 4; done; return 1; }

P=(--payer "$PAYER"); MS="$TAG"
DEST=$("$W" account new public 2>/dev/null | grep -oE 'Public/[A-Za-z0-9]+' | head -1 | cut -d/ -f2)
for k in alice bob carol; do mkdir -p "$D/$k"; ACCORD_KEYS="$D/$k" "$CLI" key new "$k" >/dev/null; done
KA=$(jget "$(as alice key show alice)" approval_key); KB=$(jget "$(as bob key show bob)" approval_key); KC=$(jget "$(as carol key show carol)" approval_key)
echo "# payer $PAYER dest $DEST multisig $MS alice $KA bob $KB carol $KC" >> "$OUT"

echo "== 1. a 2-of-3 on chain: Alice proposes and executes, Bob and Carol approve"
step create applied alice -- "${P[@]}" create "$MS" --member "$KA:pe" --member "$KB:a" --member "$KC:a" --threshold 2
VAULT=$(jget "$(as alice ids "$MS")" vault)
"$W" auth-transfer send --from "Public/$PAYER" --to "Public/$VAULT" --amount 1000 >/dev/null 2>&1
ACT=$(mktemp -d); cp "$D/bob/bob.key" "$D/carol/carol.key" "$ACT/"
step activate applied alice -- "${P[@]}" --keys "$ACT" activate "$MS" --key bob --key carol
rm -rf "$ACT"
EPOCH=$(jget "$(as alice show "$MS")" epoch 2>/dev/null || echo 0)

echo "== 2. three chat_module instances, one per member"
echo "# logosctl $("$LC" --version 2>&1 | head -1)" >> "$OUT"
for w in alice bob carol; do
  mkdir -p "$D/lc-$w"
  "$LC" --config-dir "$D/lc-$w" daemon start --detach >/dev/null
  # the session's package manager comes up a moment after the daemon does
  ok=""; for i in $(seq 1 20); do
    out="$("$LC" --human --config-dir "$D/lc-$w" install chat_module -y 2>&1)" && { ok=yes; break; }; sleep 3; done
  printf '%s\n' "$out" | grep -E "install +(chat|delivery)_module" | sed "s/^/  $w: /"
  check "$w installed chat_module from the catalog" "$ok" yes
  "$LC" --config-dir "$D/lc-$w" module load chat_module >/dev/null
  chat "$w" init "json:{\"delivery_preset\":\"$PRESET\"}" >/dev/null
done
echo "# chat_module $("$LC" --human --config-dir "$D/lc-alice" package show chat_module | grep -i '^installed' | tr -s ' ')" >> "$OUT"
for w in alice bob carol; do
  st=""; for i in $(seq 1 90); do st="$(chat "$w" status)"; case "$st" in *'"delivery_state":"online"'*) break ;; esac; sleep 2; done
  check "$w's chat node is online" "$(printf '%s' "$st" | grep -o '"delivery_state":"[a-z]*"')" '"delivery_state":"online"'
  chat "$w" get_address | python3 -c 'import json,sys;sys.stdout.write(json.load(sys.stdin)["result"])' > "$D/$w.addr"
  echo "# chat address $w $(cat "$D/$w.addr")" >> "$OUT"
done

echo "== 3. Alice opens the room for epoch $EPOCH and invites Bob and Carol"
out="$(chat alice create_group_conversation "accord $MS e$EPOCH" "Accord coordination room")"
GID="$(printf '%s' "$out" | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"]["value"])')"
echo "# chat group $GID" >> "$OUT"; echo "  room $GID"
for w in bob carol; do
  chat alice add_group_member "$GID" "@$D/$w.addr" >/dev/null
  joined=""; for i in $(seq 1 150); do
    chat "$w" list_conversations | grep -q "$GID" && { joined=yes; break; }; sleep 4; done
  check "$w joined the room" "$joined" yes
done

echo "== 4. Alice proposes in the room"
as alice propose "$MS" --transfer "$DEST:70" --key alice --description "pay the auditor" --out "$D/pay.json"
python3 -c 'import json,sys;json.dump(json.load(open(sys.argv[1])),open(sys.argv[2],"w"),separators=(",",":"))' "$D/pay.json" "$D/pay.min.json"
DIG=$(jget "$(cat "$D/pay.json")" digest)
send alice "$GID" "$D/pay.min.json"
IS_PKG="c.startswith('{') and json.loads(c).get('digest') == '$DIG'"

echo "== 5. Bob and Carol each take the package from the room, decode it, approve it, post it back"
for w in bob carol; do
  hear "$w" "$GID" "$(cat "$D/alice.addr")" "$IS_PKG" "$D/$w-heard.json"
  check "$w heard Alice's package" "$(test -s "$D/$w-heard.json" && echo yes)" yes
  check "$w decodes it, digest matches its bytes" "$(jget "$(as "$w" decode "$D/$w-heard.json")" digest_matches_bytes 2>/dev/null)" True
  as "$w" approve "$D/$w-heard.json" --key "$w"
  send "$w" "$GID" "$D/$w-heard.json"
done
printf '%s' "checked the invoice, approving" > "$D/say.txt"; send bob "$GID" "$D/say.txt"
# Bob also posts a copy with a forged approval: Carol's key, a signature that does not verify.
python3 - "$D/bob-heard.json" "$KC" "$D/forged.json" <<'PY'
import json, sys
p = json.load(open(sys.argv[1])); p["approvals"] = [{"key": sys.argv[2], "sig": "11" * 64}]
json.dump(p, open(sys.argv[3], "w"), separators=(",", ":"))
PY
send bob "$GID" "$D/forged.json"

echo "== 6. Alice merges what she heard and queues it"
HAS1="$IS_PKG and len(json.loads(c)['approvals']) == 1"
hear alice "$GID" "$(cat "$D/bob.addr")" "$HAS1 and json.loads(c)['approvals'][0]['key'] == '$KB'" "$D/from-bob.json"
hear alice "$GID" "$(cat "$D/carol.addr")" "$HAS1" "$D/from-carol.json"
hear alice "$GID" "$(cat "$D/bob.addr")" "$HAS1 and json.loads(c)['approvals'][0]['key'] == '$KC'" "$D/forged-heard.json"
hear alice "$GID" "$(cat "$D/bob.addr")" "c == 'checked the invoice, approving'" "$D/said.txt"
check "Alice heard Bob's approval" "$(test -s "$D/from-bob.json" && echo yes)" yes
check "Alice heard Carol's approval" "$(test -s "$D/from-carol.json" && echo yes)" yes
check "Alice heard the forged copy" "$(test -s "$D/forged-heard.json" && echo yes)" yes
check "Alice heard Bob's deliberation" "$(cat "$D/said.txt" 2>/dev/null)" "checked the invoice, approving"
check "the forged approval is dropped" "$(jget "$(as alice merge "$D/inbox.json" "$D/forged-heard.json")" approvals)" 0
check "the two real approvals merge" "$(jget "$(as alice merge "$D/inbox.json" "$D/from-bob.json" "$D/from-carol.json" "$D/forged-heard.json")" approvals)" 2
step queue applied alice -- "${P[@]}" queue "$D/inbox.json"
step execute applied alice -- "${P[@]}" execute "$D/inbox.json" --key alice
check "destination received 70" "$(bal "$DEST")" 70

echo "== 7. the room as each member sees it"
for w in alice bob carol; do
  chat "$w" get_messages "$GID" | python3 -c '
import json,sys
names = dict(l.split() for l in sys.argv[2:])
for m in json.load(sys.stdin)["result"]:
    c = m.get("content", ""); s = names.get(m.get("sender"), m.get("sender", "")[:12])
    try: p = json.loads(c); c = "package %s… approvals %d" % (p["digest"][:12], len(p["approvals"]))
    except Exception: pass
    print("  %s sees  %-6s %s" % (sys.argv[1], s, c))' "$w" "$(cat "$D/alice.addr") alice" "$(cat "$D/bob.addr") bob" "$(cat "$D/carol.addr") carol" | tee -a /dev/stderr | sed 's/^/# room: /' >> "$OUT"
done
echo "summary: $(awk -F'\t' '$3=="applied"' "$OUT" | wc -l | tr -d ' ') applied, $(grep -c '^# check ok' "$OUT") checks ok, failures=$fail"
exit $fail
