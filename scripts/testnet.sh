#!/usr/bin/env bash
# The public LEZ testnet run: the public lifecycle, then the private vault,
# then the independent check, each with run-unique TAGs so nothing collides
# with an earlier run or anyone else's multisig ids.
#
#   WALLET=<v0.3 wallet binary> LEE_WALLET_HOME_DIR=<this prototype's wallet home> \
#     PAYER=<funded account> scripts/testnet.sh
#
# Writes evidence/testnet/{public,private}.tsv, their transcripts and the
# verifier's output for each. scripts/testnet-when-funded.sh wraps this with a
# balance gate and the explorer links.
set -uo pipefail
cd "$(dirname "$0")/.."
: "${LEE_WALLET_HOME_DIR:?the prototype's own wallet home}"; : "${PAYER:?a funded account in it}"
# The wallet must be a v0.3 build: an older `wallet` on PATH fails on v0.3 accounts.
: "${WALLET:?the v0.3 wallet binary (LEZ v0.3.0 db66590a: target/release/wallet)}"
export WALLET RPC=https://testnet.lez.logos.co SUPPRESS_VERBOSE_PRINTS=1
STAMP="$(date -u +%m%d%H%M)"
mkdir -p evidence/testnet
fail=0
DEPLOY=1 TAG="acc-pub-$STAMP" OUT=evidence/testnet/public.tsv ./scripts/e2e.sh 2>&1 | tee evidence/testnet/public-transcript.txt || fail=1
./scripts/verify-onchain.sh --manifest evidence/testnet/public.tsv --rpc "$RPC" 2>&1 | tee evidence/testnet/public-verify.txt || fail=1
TAG="acc-priv-$STAMP" OUT=evidence/testnet/private.tsv ./scripts/e2e-private.sh 2>&1 | grep -v -E "^(INF|DBG|WRN|TRC|NTC|ERR) " | tee evidence/testnet/private-transcript.txt || fail=1
./scripts/verify-onchain.sh --manifest evidence/testnet/private.tsv --rpc "$RPC" 2>&1 | tee evidence/testnet/private-verify.txt || fail=1
exit $fail
