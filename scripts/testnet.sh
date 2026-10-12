#!/usr/bin/env bash
# The public LEZ testnet run: the public lifecycle, then the private vault,
# then the independent check, each with run-unique TAGs so nothing collides
# with an earlier run or anyone else's multisig ids.
#
#   LEE_WALLET_HOME_DIR=<this prototype's wallet home> PAYER=<funded account> scripts/testnet.sh
#
# Writes evidence/testnet/{public,private}.tsv and their transcripts.
set -uo pipefail
cd "$(dirname "$0")/.."
: "${LEE_WALLET_HOME_DIR:?the prototype's own wallet home}"; : "${PAYER:?a funded account in it}"
export RPC=https://testnet.lez.logos.co SUPPRESS_VERBOSE_PRINTS=1
STAMP="$(date -u +%m%d%H%M)"
mkdir -p evidence/testnet
fail=0
DEPLOY=1 TAG="acc-pub-$STAMP" OUT=evidence/testnet/public.tsv ./scripts/e2e.sh 2>&1 | tee evidence/testnet/public-transcript.txt || fail=1
./scripts/verify-onchain.sh --manifest evidence/testnet/public.tsv --rpc "$RPC" 2>&1 | tee evidence/testnet/public-verify.txt || fail=1
TAG="acc-priv-$STAMP" OUT=evidence/testnet/private.tsv ./scripts/e2e-private.sh 2>&1 | grep -v -E "^(INF|DBG|WRN|TRC|NTC|ERR) " | tee evidence/testnet/private-transcript.txt || fail=1
exit $fail
