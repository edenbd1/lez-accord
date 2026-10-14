#!/usr/bin/env bash
# The public-testnet step on its own: wait for (or check) the payer's balance,
# run scripts/testnet.sh (public lifecycle, private vault, verifier on both),
# and write evidence/testnet/links.md with an explorer link per transaction.
#
#   WALLET=<v0.3 wallet> LEE_WALLET_HOME_DIR=<wallet home> PAYER=<account id> \
#     scripts/testnet-when-funded.sh [--wait]
#
# Without --wait it refuses to start below MIN (default 500,000,000) and exits 3.
# With --wait it polls every 5 minutes, read-only, until the balance is there.
# It never moves funds other than the payer's own fees and the run's transfers.
set -uo pipefail
cd "$(dirname "$0")/.."
: "${LEE_WALLET_HOME_DIR:?}"; : "${PAYER:?}"; : "${WALLET:?the v0.3 wallet binary}"
RPC=https://testnet.lez.logos.co
EXPLORER="${EXPLORER:-https://explorer.testnet.lez.logos.co}"
MIN="${MIN:-500000000}"
rpc() { curl -s -m 20 -X POST "$RPC" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":$2}"; }
balance() { rpc getAccountBalance "[\"$PAYER\"]" | python3 -c 'import json,sys
try: print(int(json.load(sys.stdin)["result"]))
except Exception: print(-1)'; }

grep -q 'testnet.lez.logos.co' "$LEE_WALLET_HOME_DIR/wallet_config.json" \
  || { echo "the wallet home does not point at $RPC" >&2; exit 2; }
"$WALLET" --version >/dev/null 2>&1 || { echo "WALLET=$WALLET does not run" >&2; exit 2; }
echo "testnet height $(rpc getLastBlockId '[]' | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"])'), channel $(rpc getChannelId '[]' | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"])')"

b="$(balance)"
if [ "${1:-}" = --wait ]; then
  until [ "$b" -ge "$MIN" ]; do
    echo "$(date -u +%FT%TZ) payer $PAYER holds $b, waiting for $MIN"
    sleep 300; b="$(balance)"
  done
fi
echo "payer $PAYER holds $b"
[ "$b" -ge "$MIN" ] || { echo "below $MIN: not running (use --wait to poll)" >&2; exit 3; }

./scripts/testnet.sh; rc=$?

# Explorer links, from the manifests the run wrote.
python3 scripts/links.py "$EXPLORER" evidence/testnet/public.tsv evidence/testnet/private.tsv > evidence/testnet/links.md
echo "explorer links: evidence/testnet/links.md"
exit $rc
