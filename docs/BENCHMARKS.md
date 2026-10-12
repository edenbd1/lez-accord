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
budget of one transaction (gas is cycles one for one). Raw output:
[`bench-accelerated.txt`](bench-accelerated.txt).

| op | cycles | share of the 10M gas cap |
|---|---:|---:|
| create (3 members) | 124,073 | 1.2% |
| queue, M = 2 (3 signatures) | 867,409 | 8.7% |
| execute, native transfer (1 signature) | 329,527 | 3.3% |
| spend within a limit (1 signature) | 311,850 | 3.1% |

| M | queue cycles | share of the cap |
|---:|---:|---:|
| 1 | 599,411 | 6.0% |
| 3 | 1,147,522 | 11.5% |
| 5 | 1,705,954 | 17.1% |
| 7 | 2,253,008 | 22.5% |
| 10 | 3,074,907 | 30.7% |
| 16 | 4,731,766 | 47.3% |
| 24 | 6,952,355 | 69.5% |
| 32 | 9,136,004 | 91.4% |

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

`RISC0_DEV_MODE=0 cargo run --release --bin prove -- 1 3 5 7 10 16` proves the
guest's `apply` of `VerifyQueue`, the step that verifies the approvals, with
the RISC Zero prover (`r0vm` 3.0.5) on this machine, and reports the succinct
receipt. This is the receipt a member's client produces under the private
posture, where the privacy-preserving circuit composes it. Raw output:
[`bench-real-proving.txt`](bench-real-proving.txt).

Machine: Apple M4 Pro, 14 cores, CPU proving.

| M | signatures verified | user cycles | segments | prove (s, wall) | succinct receipt (bytes) |
|---:|---:|---:|---:|---:|---:|
| 1 | 2 | 556,046 | 1 | 71.0 | 224,513 |
| 3 | 4 | 1,107,971 | 2 | 108.0 | 225,193 |
| 5 | 6 | 1,644,837 | 2 | 290.6 | 225,873 |
| 7 | 8 | 2,192,650 | 3 | 378.7 | 226,553 |
| 10 | 11 | 2,995,783 | 4 | 256.0 | 227,573 |
| 16 | 17 | 4,655,878 | 5 | 554.7 | 229,613 |

The succinct receipt is nearly constant in M (about +340 bytes per approval, the
signatures themselves in the journal). Proving time grows with segments. The
M = 5 and M = 7 runs overlapped a full LEZ build on the same machine; the
figures are wall clock as measured, and a quiet rerun is the first item of
the grant's benchmark milestone.

### Transaction size

A `queue` instruction carries, per approval, a 32-byte key and a 64-byte
signature, 96 bytes: M = 5 carries 480 bytes of approvals, M = 32 carries 3,072, far under
the 1 MiB block size of the local sequencer configuration.
