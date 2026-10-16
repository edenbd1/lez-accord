# The whole private transaction, measured at M = 1, 3, 5, 10

`docs/BENCHMARKS.md` proves the program's own receipt. This measures the whole
privacy-preserving transaction a member's client builds and sends: the
program's execution composed by the LEZ privacy circuit, with the private
vault spent as a nullifier plus a new commitment.

**What runs.** `accord spend --private-vault` under a spending limit whose
sub-quorum is M: the program verifies M BIP-340 signatures from the limit's
spenders inside the private transaction (`Effect::Spend`, each signature
checked, distinct, a spender of the limit, at least `min_signers`). One
multisig, 10 members, threshold 1, four limits with sub-quorums 1, 3, 5 and 10;
each run spends 10 from the private vault to a public account.
Driver: [`scripts/measure-private-m.sh`](../scripts/measure-private-m.sh).
Raw output: [`docs/raw/`](raw/) (testnet) and [`docs/raw/local/`](raw/local/)
(local v0.3.1 sequencer).

- **Prove** is `prove_and_submit_ms`, from the start of proving to the
  sequencer accepting the transaction (the CLI prints it). Real proving,
  `RISC0_DEV_MODE` unset, CPU.
- **Peak RAM** is `maximum resident set size` from `/usr/bin/time -l` over the
  whole `accord` command (on macOS, the largest single process of the tree,
  `r0vm` included; not a sum).
- **Bytes** is the transaction as the sequencer returns it
  (`getTransaction`, base64-decoded); its first byte is 1, a
  `PrivacyPreserving` transaction.

Machine: Apple M4 Pro (`hw.model` Mac16,8), 14 cores (10 performance +
4 efficiency), 24 GiB (`hw.memsize` 25,769,803,776). Program
`CehtqLan7giiHfVV5qDD9dtXvVi2h7GMdXM87HD2bbQF`, ImageID `24a69044…`, the
committed binary, not redeployed.

## Public testnet (v0.3.1), 2026-10-16

| M | signatures verified | prove (s) | peak RAM | tx bytes | tx hash | block | resolves |
|---:|---:|---:|---:|---:|---|---:|---|
| 1 | 1 | 160.6 | 6.52 GB | 248,012 | `fda6307341be61c353127f1871035c54339ad0225b9543d463748d2c37863b2b` | 25552 | yes |
| 3 | 3 | 188.6 | 7.87 GB | 248,396 | `804ce7d1513c59d57b97d7b2667aa7cbdb4822fead88ec7b5df559bd90e77556` | 25471 | yes |
| 5 | 5 | 165.3 | 7.21 GB | 248,780 | `5e8f7d3ad6f9e4679922c20d55815fc0a6cc5233e36fa15e8dc1f3675dc68cdd` | 25498 | yes |
| 10 | 10 | 165.1 | 6.61 GB | 249,740 | `6b3bad982da1b7cd53d80a74f662f1199eb84b27dc51d6c34a41597d11f013c6` | 25523 | yes |

Re-checked after the runs ([`raw/private-m-testnet-recheck.txt`](raw/private-m-testnet-recheck.txt)):
every hash resolves, a random hash does not, the four limits' counters moved,
the destination holds what the spends sent, and the vault's public balance is 0.

## Local v0.3.1 sequencer, same binary

| M | prove (s) | peak RAM | tx bytes |
|---:|---:|---:|---:|
| 1 | 179.4 | 6.82 GB | 248,012 |
| 3 | 164.3 | 6.01 GB | 248,396 |
| 5 | 165.1 | 7.63 GB | 248,780 |
| 10 | 177.4 | 7.17 GB | 249,740 |

## Reading it

- **Proving time is flat in M**, 160 to 190 s from M = 1 to M = 10. The
  privacy circuit dominates, and the spread between runs is bigger than the
  effect of nine more signatures. The receipt-only bench's ~20 s per approval
  does not add up linearly inside the composed transaction.
- **Size grows by 192 bytes per approval** (twice the 96 bytes of a key
  and a signature), on a base of about 247.8 kB. M = 10 is 249,740
  bytes, so a 1 MiB block holds four.
- **Peak RAM is 6 to 8 GB**, with no trend in M, well under the 24 GiB machine.
- **The window.** The program's 10-minute window starts at the client's `at`.
  These runs set `ACCORD_AT_DELAY_MS=60000` on the testnet (120000 for local
  M = 10), a new CLI option, so slow inclusion does not run the window out.
  It changes no on-chain rule.

## Load and caveats

Other work shared the machine. During the four testnet runs, no other
prover was running (the `# load during` lines in each raw file count `r0vm`
processes; the only one was this run's). Local M = 3 and M = 10 overlapped
another prover for part of the run. A first local M = 1 run, under heavier
load, took 236.4 s ([`raw/local/private-m1-run1.txt`](raw/local/private-m1-run1.txt)).

What went wrong on the testnet, kept in `docs/raw/`:

- The first setup built the M = 1 limit as `k1+k2+k1` (`seq 2 1` counts down).
  `create` reverted on chain with E7116 and was charged its full gas reserve,
  twice (two attempts). Fixed in the script; the multisig was then created
  under the same id.
- The first M = 1 spend (`30fdfcaf…`) was seen in block 25254, was missing
  minutes later, and now sits in block 25263: a block rewrite moved it. It
  looked dropped at the time, so the spend was sent again.
  The second attempt (`0b959b88…`) was never included, most likely because its
  nullifier was already spent. The one after that (`fda63073…`) is the row in
  the table. Proving time for the first attempt (465.9 s) was taken while
  another prover was running, and is not used.
- One M = 3 attempt was refused by the wallet before proving
  (`MissingMembershipProof`) while its view of the vault was stale.
