#!/usr/bin/env bash
# The fast tour: the program's rules, the committed binary through the
# sequencer's state machine, the IDL check, and the cost of approvals vs M.
# The full on-chain lifecycle is scripts/e2e.sh (docs/LOCAL.md).
set -euo pipefail
cd "$(dirname "$0")"
echo "== program rules (host)";        cargo test -q --manifest-path programs/accord/Cargo.toml -p accord-core 2>&1 | grep "test result" | head -1
echo "== committed binary, V03State";  cargo test -q --release --manifest-path executor-tests/Cargo.toml 2>&1 | grep "test result: ok. [1-9]"
echo "== IDL";                         python3 scripts/check-idl.py
echo "== approval cost vs M";          cargo run -q --release --manifest-path executor-tests/Cargo.toml --bin measure 2>/dev/null | sed -n '/^| M/,$p' | sed -n '1,2p;3p;7p;12p;18p;34,40p'
