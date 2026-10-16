#!/usr/bin/env bash
# The whole private transaction at M approvals: a spend from the private vault
# under a spending limit whose sub-quorum is M, so the program verifies M
# BIP-340 signatures inside a privacy-preserving transaction (nullifier +
# commitment for the vault, proved locally, composed by the privacy circuit).
#
#   setup:    one multisig with 10 members, threshold 1, four limits whose
#             sub-quorums are 1, 3, 5 and 10; a private vault funded with 1000
#   spend M:  `accord spend --limit i --key k1..kM --private-vault <group>`
#             under /usr/bin/time -l, then the transaction read back from the
#             sequencer (getTransaction) and its bytes counted
#
#   WALLET=<v0.3.0 wallet> LEE_WALLET_HOME_DIR=<wallet home> PAYER=<funded public account> \
#   ACCORD_PROGRAM=<header> scripts/measure-private-m.sh setup
#   ... scripts/measure-private-m.sh spend 1      (then 3, 5, 10)
#
# ACCORD_AT_DELAY_MS opens the spend's window later than the chain clock, for a
# proof expected to take longer than the 10-minute window.
# State between phases lives in $STATE (default docs/raw/private-m.state);
# the approval keys in $ACCORD_KEYS (default a directory outside the repository).
# Raw outputs go to docs/raw/.
set -uo pipefail
cd "$(dirname "$0")/.."
: "${LEE_WALLET_HOME_DIR:?}"; : "${PAYER:?}"; : "${ACCORD_PROGRAM:?}"; : "${WALLET:?}"
RPC="${RPC:-https://testnet.lez.logos.co}"
export ACCORD_RPC="$RPC" ACCORD_PROGRAM SUPPRESS_VERBOSE_PRINTS=1
CLI="${CLI:-cli/target/release/accord}"
export ACCORD_ELF="${ACCORD_ELF:-artifacts/programs/v0.3/accord_multisig.bin}"
export ACCORD_KEYS="${ACCORD_KEYS:-$HOME/.accord-measure-keys}"
RAW="${RAW:-docs/raw}"; mkdir -p "$RAW"
STATE="${STATE:-$RAW/private-m.state}"
off() { "$CLI" "$@" 2>/dev/null | grep '^{' | tail -1; }
jget() { python3 -c 'import json,sys;print(json.loads(sys.argv[1])[sys.argv[2]])' "$1" "$2"; }
quiet() { grep -v -E "^(INF|DBG|WRN|TRC|NTC|ERR) |^\[|^ *$"; }

case "${1:?setup or spend <M>}" in
setup)
  STAMP="$(date -u +%m%d%H%M)"; MS="acc-pm-$STAMP"; GROUP="accord-pm-$STAMP"
  LOG="$RAW/private-m-setup-$STAMP.txt"
  { echo "# run against $RPC, $(date -u +%Y-%m-%dT%H:%M:%SZ)"; echo "# program $ACCORD_PROGRAM $("$CLI" image-id)"; } | tee "$LOG"
  for i in $(seq 1 10); do "$CLI" key new "k$i" >/dev/null 2>&1 || true; done
  members=(--member k1:pae); for i in $(seq 2 10); do members+=(--member "k$i:a"); done
  spenders() { local s=k1 i; for ((i = 2; i <= $1; i++)); do s="$s+k$i"; done; echo "$s"; }   # not seq: `seq 2 1` counts down
  limits=(); for m in 1 3 5 10; do limits+=(--limit "$(spenders $m):$m:100:1d"); done
  "$CLI" --label create --payer "$PAYER" create "$MS" "${members[@]}" --threshold 1 "${limits[@]}" 2>&1 | grep '^{' | tee -a "$LOG"
  "$CLI" --label activate --payer "$PAYER" activate "$MS" --key k1 2>&1 | grep '^{' | tee -a "$LOG"
  "$WALLET" group new "$GROUP" >/dev/null 2>&1
  VAULT="$(jget "$(off --payer "$PAYER" vault id "$MS" --group "$GROUP")" private_vault)"
  DEST=$("$WALLET" account new public 2>/dev/null | grep -oE 'Public/[A-Za-z0-9]+' | head -1 | cut -d/ -f2)
  "$CLI" --label fund --payer "$PAYER" vault fund "$MS" --group "$GROUP" --from "$PAYER" --amount 1000 2>&1 | grep '^{' | tee -a "$LOG"
  off --payer "$PAYER" vault track "$MS" --group "$GROUP" | tee -a "$LOG"
  printf 'MS=%s\nGROUP=%s\nVAULT=%s\nDEST=%s\n' "$MS" "$GROUP" "$VAULT" "$DEST" > "$STATE"
  cat "$STATE" | tee -a "$LOG"
  ;;
spend)
  M="${2:?M}"; . "$STATE"
  case "$M" in 1) L=0 ;; 3) L=1 ;; 5) L=2 ;; 10) L=3 ;; *) echo "M is 1, 3, 5 or 10" >&2; exit 2 ;; esac
  keys=(); for ((i = 1; i <= M; i++)); do keys+=(--key "k$i"); done
  LOG="$RAW/private-m$M.txt"
  {
    echo "# M=$M: spend under limit $L (sub-quorum $M) from private vault $VAULT of $MS, to $DEST"
    echo "# $(date -u +%Y-%m-%dT%H:%M:%SZ) against $RPC; ACCORD_AT_DELAY_MS=${ACCORD_AT_DELAY_MS:-0}"
    echo "# $(sysctl -n hw.model) $(sysctl -n machdep.cpu.brand_string) hw.memsize=$(sysctl -n hw.memsize) hw.ncpu=$(sysctl -n hw.ncpu)"
    echo "# load before: $(uptime | sed 's/.*load/load/'); other provers running: $(pgrep -f 'r0vm --port' | wc -l | tr -d ' ')"
  } > "$LOG"
  ( while sleep 30; do echo "# load during: $(date -u +%H:%M:%S) $(uptime | sed 's/.*load averages: //'); r0vm processes incl. this run's: $(pgrep -f 'r0vm --port' | wc -l | tr -d ' ')"; done ) >> "$LOG.load" &
  LOADER=$!
  /usr/bin/time -l "$CLI" --label "spend-private-m$M" --payer "$PAYER" spend "$MS" --limit "$L" --to "$DEST" --amount 10 \
    "${keys[@]}" --private-vault "$GROUP" 2>&1 | quiet | tee -a "$LOG"
  kill "$LOADER" 2>/dev/null; cat "$LOG.load" >> "$LOG"; rm -f "$LOG.load"
  TX="$(grep '"op":"spend-private"' "$LOG" | tail -1 | python3 -c 'import json,sys;print(json.load(sys.stdin).get("tx",""))')"
  if [ -n "$TX" ]; then
    python3 - "$RPC" "$TX" <<'PY' | tee -a "$LOG"
import base64, json, sys, urllib.request
rpc, tx = sys.argv[1], sys.argv[2]
body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getTransaction", "params": [tx]}).encode()
res = json.load(urllib.request.urlopen(urllib.request.Request(rpc, body, {"Content-Type": "application/json"}), timeout=60)).get("result")
if not res:
    print(json.dumps({"tx": tx, "resolves": False}))
else:
    raw = base64.b64decode(res[0])
    print(json.dumps({"tx": tx, "resolves": True, "block": res[1], "tx_bytes": len(raw), "kind_tag": raw[0]}))
PY
  fi
  ;;
esac
