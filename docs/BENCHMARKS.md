# Benchmarks (P.1, P.2)

Every figure here is produced by code in this repository, from the committed
guest binary `artifacts/programs/v0.3/accord_multisig.bin`.

## Approval verification is the program's cost

Accord's approvals are BIP-340 signatures by members' approval keys,
verified inside the program's `apply` (DESIGN.md §2, docs/DECISIONS.md D1).
A `queue` with M approvals verifies M + 1 signatures: the approvals and the
proposer's.

### Cycles, through the sequencer's own state machine

`cargo run --release --bin measure` in `executor-tests/` runs each
instruction through `lee::V03State` (`ValidatedStateDiff::from_public_transaction_with_cycle_budget`),
the call the v0.3 sequencer's settlement makes, with the 10,000,000-cycle
budget of one transaction (gas is cycles one for one). Raw output, on the
committed binary (`24a69044…`): [`bench-accelerated.txt`](bench-accelerated.txt).

The keys are fixed, but each BIP-340 signature draws fresh auxiliary
randomness, so a rerun moves every row slightly: a rerun on 2026-10-13
([`bench-accelerated-rerun-2026-10-13.txt`](bench-accelerated-rerun-2026-10-13.txt))
differs from the table below by at most 1.32% on any of its 36 rows, and by at
most 0.5 points of the cap. Expect that spread when you reproduce it.

| op | cycles | share of the 10M gas cap |
|---|---:|---:|
| create (3 members) | 124,125 | 1.2% |
| queue, M = 2 (3 signatures) | 880,503 | 8.8% |
| execute, native transfer (1 signature) | 329,044 | 3.3% |
| spend within a limit (1 signature) | 311,751 | 3.1% |

| M | queue cycles | share of the cap |
|---:|---:|---:|
| 1 | 601,700 | 6.0% |
| 3 | 1,148,829 | 11.5% |
| 5 | 1,704,538 | 17.0% |
| 7 | 2,271,522 | 22.7% |
| 10 | 3,103,377 | 31.0% |
| 16 | 4,755,001 | 47.6% |
| 24 | 6,970,611 | 69.7% |
| 32 | 9,181,398 | 91.8% |

About **275,000 cycles per approval**, linear in M. **M = 32**, the most
members a configuration may hold, fits one transaction. The RFP's line,
M ≥ 5, is passed with a factor of six to spare.

### The same program with the stock `k256`

`ACCORD_BIN=artifacts/bench/accord_multisig.k256-unaccelerated.bin cargo run --release --bin measure`
([`bench-unaccelerated.txt`](bench-unaccelerated.txt)): **no M is viable**.
Activation with two signatures, and `queue` with one approval (two
verifications), both exceed the cycle budget. RISC Zero's accelerated fork
(bigint2 curve arithmetic) is what makes in-program approval verification
possible at all on LEZ v0.3 (D2).

### Real proving (P.1: never development mode)

`RISC0_DEV_MODE=0 cargo run --release --bin prove -- 1 3 5 10 16` proves the
guest's `apply` of `VerifyQueue`, the step that verifies the approvals, with
the RISC Zero prover (`r0vm` 3.0.5) on this machine, and reports the succinct
receipt. This is the receipt a member's client produces under the private
posture, where the privacy-preserving circuit composes it. Raw output:
[`bench-real-proving.txt`](bench-real-proving.txt).

Machine: Apple M4 Pro, 14 cores, CPU proving, final binary (`24a69044…`, the reproducible build),
no other heavy work running.

| M | signatures verified | user cycles | segments | prove (s, wall) | succinct receipt (bytes) |
|---:|---:|---:|---:|---:|---:|
| 1 | 2 | 559,965 | 1 | 71.5 | 224,513 |
| 3 | 4 | 1,108,793 | 2 | 93.4 | 225,193 |
| 5 | 6 | 1,653,649 | 2 | 151.8 | 225,873 |
| 10 | 11 | 3,017,893 | 4 | 251.7 | 227,573 |
| 16 | 17 | 4,656,025 | 5 | 380.6 | 229,613 |

The succinct receipt is nearly constant in M (about +340 bytes per approval, the
signatures themselves in the journal). Proving time grows with the number of segments, about 20 s per
approval on this machine; every M up to 16 proves in under seven minutes, inside the
ten-minute window a private transaction has to land in.

### Transaction size

A `queue` instruction carries, per approval, a 32-byte key and a 64-byte
signature, 96 bytes: M = 5 carries 480 bytes of approvals, M = 32 carries 3,072, far under
the 1 MiB block size of the local sequencer configuration.
