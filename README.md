# Accord

An M-of-N multisig for the Logos Execution Zone v0.3, with an end-to-end
encrypted coordination room over Logos Delivery. Prototype for RFP-005
(Multisig and Coordination).

- **Approvals are signatures, checked by the program.** A member is a BIP-340
  approval key with a role mask (propose, approve, execute) and a weight.
  Proposals, approvals, the execute authorisation and spends are signatures
  over domain-separated digests that commit to the multisig, the
  configuration epoch, the expiry and the exact action bytes; the program
  verifies them inside its own execution. No member's LEZ account ever signs
  or appears in a multisig transaction: the only LEZ signer is a fee payer.
- **Approvals are collected off-chain**, as approval packages carried by the
  room (or any transport). Reaching quorum costs one `queue` transaction.
- **Any program.** An approved action is a list of calls to any program in
  the LEZ, each authorised as the vault by a PDA seed only this program can
  attach, and by nothing else: an action naming an account that signs the
  transaction is refused.
- **Configuration through the quorum**, time lock, veto, expiry, spending
  limits with sub-quorums, an on-chain registry for decoding actions, and a
  private vault: a private PDA of this program keyed by a Group Master Secret
  the members share, spendable only through the program.

| | |
| --- | --- |
| LEZ | program and CLI built on v0.3.0 (`db66590a`); run against v0.3.1 (`1a003bfb`), as the public testnet is, with v0.3.1's block format patched into the CLI (`cli/vendor/lez-common`) |
| Program | `programs/accord`, guest `accord_multisig`, written against `lee_core` |
| Public testnet | header `CehtqLan7giiHfVV5qDD9dtXvVi2h7GMdXM87HD2bbQF`; the public lifecycle and the private vault run there, 34 and 9 checks, 0 failures ([`evidence/testnet/`](evidence/testnet/README.md)) |
| Binary | `artifacts/programs/v0.3/accord_multisig.bin` |
| IDL | `idl/accord_multisig.idl.json` (SPEL's shape, drift-checked) |
| CLI | `cli/`, binary `accord` |
| Design | [`DESIGN.md`](DESIGN.md), decision log [`docs/DECISIONS.md`](docs/DECISIONS.md) |
| Benchmarks | [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) |

## What is tested, and where

| Layer | What runs | Count |
| --- | --- | --- |
| `programs/accord/core` host tests | `plan` and `apply` directly, every rule | 28 |
| `executor-tests` | the committed binary through `lee::V03State`, the state machine the sequencer applies, real native and token programs, real timestamp windows, the 10M gas cap | 20 |
| `cli` unit tests | room invites and frames: sealing, signatures, epoch separation | 2 |
| `scripts/e2e.sh` | the whole public lifecycle on a local v0.3 sequencer through the CLI, with a manifest | `evidence/v03/local-e2e.tsv` |
| `scripts/e2e-private.sh` | the private vault: funded privately, refused to a holder of the group keys, spent and executed through the program in privacy-preserving transactions | `evidence/v03/local-private.tsv` |
| `scripts/verify-onchain.sh` | re-checks a manifest against the chain: every transaction in its block, every refusal unresolved or reverted, final state hashes, the program header's ImageID | |
| `scripts/check-idl.py` | decodes borsh vectors of every instruction, account and event with the IDL alone | 15 vectors |

One executor test, `every_refusal_code_fires_on_chain`, fires each of the 17
refusal codes through the real binary and fails if any code is missing.

**The tests are themselves tested.** `scripts/mutation-test.sh` breaks each
guard the program's safety rests on (threshold, replay and counters, epoch,
expiry and time lock, membership, roles, duplicate signers, signature
validity), one at a time, and requires a host test to fail: 17 of 17 mutants
are killed, each by a named test ([`evidence/mutation/host.txt`](evidence/mutation/host.txt)).
`scripts/mutation-test-guest.sh` builds 6 of them into the real guest and
runs the executor tests against it: 6 of 6 killed, and the rebuilt guest is
the committed binary again afterwards ([`evidence/mutation/guest.txt`](evidence/mutation/guest.txt)).

## Quick start (local sequencer)

```bash
# LEZ: the v0.3.0 wallet, and the v0.3.1 standalone sequencer the testnet runs
git clone https://github.com/logos-blockchain/logos-execution-zone lez && cd lez
git checkout db66590ab821a4e142c211017a3866d007f6fa77
cargo build --release -p wallet
git checkout 1a003bfbfb89fcf987f62975cd08a401169e1b8c
cargo build --release --features standalone -p sequencer_service
export PATH="$PWD/target/release:$PATH"; cd -

# the guest (local RISC Zero toolchain; DOCKER=1 for the pinned builder), the CLI
scripts/build-guest.sh
cargo build --release --manifest-path cli/Cargo.toml

# a sequencer and a wallet pointed at it (docs/LOCAL.md)
mkdir -p /tmp/seq && cp lez/lez/sequencer/service/configs/debug/sequencer_config.json /tmp/seq/
(cd /tmp/seq && sequencer_service sequencer_config.json --listen-address 127.0.0.1 &)
export LEE_WALLET_HOME_DIR=/tmp/wallet-local
wallet check-health            # writes wallet_config.json; point it at http://127.0.0.1:3040
wallet account import public --private-key 10a26a9aec7d34b82364eeae45c5294dbb0a764b000b94eeb9b58511dc487c4d

# the whole lifecycle, then the independent check
DEPLOY=1 PAYER=6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV ./scripts/e2e.sh
./scripts/verify-onchain.sh --manifest evidence/v03/local-e2e.tsv --rpc http://127.0.0.1:3040
```

## The operator journey (CLI)

```bash
export ACCORD_PROGRAM=<header> ACCORD_PAYER=<fee payer>
accord key new alice; accord key new bob; accord key new carol      # approval keys, local
accord create treasury --member alice:pa --member bob:a --member carol:ae \
  --threshold 2 --time-lock 1h --limit bob:1:100:1d --payer $ACCORD_PAYER
accord activate treasury --key alice --key carol
accord propose treasury --transfer <to>:40 --key alice --out pay.json   # an approval package
accord decode pay.json                     # the action decoded from the bytes approvers sign
accord approve pay.json --key bob          # each approver, on their own machine
accord queue pay.json                      # quorum verified on chain; the time lock starts
accord execute pay.json --key carol        # after the time lock
accord spend treasury --to <to> --amount 60 --key bob   # within the spending limit
accord show treasury
```

The room: `accord room open|join|post|listen` (docs/ROOM.md). The private
vault: `accord vault id|fund|steal` and `accord spend --private-vault <group>`
(DESIGN.md §10).

## Licence

MIT or Apache-2.0, at your option.
