# Design decision log

Each entry: the context, the options considered, the option chosen, and the
rationale and trade-offs (RFP-005 S.14). Newest last; an entry is superseded
by a later one, never edited away.

## D1. Approvals are BIP-340 signatures verified inside `apply`

**Context.** R.1 requires the program to verify, at execution, M distinct
valid approvals from current members on exactly the action. v0.3 offers two
ways to authenticate a party: a LEZ signer (the transaction's witness set,
verified by the sequencer) or a signature the program verifies itself.

**Options.**
1. Each approver signs the LEZ transaction (multi-signer witness set). Free
   for the program; but every approver's LEZ account then appears in the
   transaction, which Pr.1 forbids under the private posture, and every
   approver's nonce is frozen into the message, so approvals collected
   off-chain go stale whenever an approver transacts.
2. One on-chain transaction per approval, recorded in a proposal account.
   M + 1 transactions per action, and pending-proposal metadata on chain.
3. Approval keys dedicated to the multisig, signing domain-separated digests;
   the program verifies the signatures in `apply`.

**Chosen.** 3. The only LEZ signer of any multisig transaction is a fee
payer, who can be anyone; approvals are collected off-chain and cost one
`queue` transaction; the digest commits to everything the approval means.

**Trade-off.** Verification is now the program's cost, on the critical path.
D2 sizes it.

## D2. RISC Zero's accelerated `k256`, not the stock crate

**Context.** With the stock `k256`, verifying one BIP-340 signature in the
guest costs several million cycles. Measured through `lee::V03State` against
the 10,000,000-cycle gas cap of one transaction: activation with two
signatures, and `queue` with a single approval (two verifications, the
proposer's and the approver's), both exceed the cap
(`docs/bench-unaccelerated.txt`). M = 1 is not viable.

**Options.** Stock `k256`; RISC Zero's fork (`k256/v0.13.4-risczero.1`, with
the matching `crypto-bigint` and `sha2` forks), which routes field and curve
arithmetic through the zkVM's bigint2 accelerator; a different scheme
(Ed25519, P-256).

**Chosen.** The accelerated fork. About 275,000 cycles per verification;
`queue` with M = 32 approvals (33 verifications) uses 9.14M cycles, 91.4% of
the cap (`docs/BENCHMARKS.md`). The scheme stays BIP-340, the one LEZ uses
for its own witness sets, so any LEZ wallet key works as an approval key.

**Trade-off.** The guest depends on three forked crates, pinned by tag in
`programs/accord/Cargo.toml` and by commit in `Cargo.lock`.

## D3. The cutoff of R.2 is the `queue` transaction

**Context.** R.2: a configuration change must invalidate approvals gathered
under the old configuration, unless the action had already reached quorum and
entered its time-lock delay or been submitted.

**Chosen.** Approvals sign the configuration `epoch`; every change increments
it; `queue` refuses a digest whose epoch is not current (7102). `queue` is the
transaction in which the program verifies the quorum and the delay starts, so
it is the cutoff: a queued action keeps its verified quorum. A member removed
during the delay does not unqueue it; the new quorum can veto it with
`CancelQueued`, which is exempt from the time lock so a veto always lands
first. Configuration changes are serialised (one queued under an epoch that
has since moved does not apply).

**Trade-off.** A removed member's approval can still count toward an action
that was queued before the removal. The time lock exists to make the veto
possible; a multisig that wants no such window sets a zero time lock and
loses the veto window instead.

## D4. Time from the block timestamp window, not a clock account

**Context.** A plan cannot read state, so it cannot read a clock account.

**Chosen.** Every instruction that names a time `at` sets the validity
window `[at, at + 2 min)`, enforced by the sequencer for public and private
transactions. `quorum_at`, the time lock, expiry and spending-limit periods
are all measured against times the chain accepted.

## D5. An action carries only the vault's authority

**Context.** In v0.3 a public account that signs a transaction is authorised
in every chained call the transaction makes. An approved action naming the
fee payer's account would move the payer's funds with the payer's own
signature, though nobody approved spending them.

**Chosen.** The plan refuses any action whose calls name an account that is
authorised at the root of the transaction (7106), and any call to this
program itself. The vault's PDA seed is the only authority the calls carry.

## D6. One configuration account, append-once queued records

**Context.** `apply` sees one shard. A check that needs both the
configuration and a proposal must be split across two effects.

**Chosen.** `queue` emits two effects: the configuration's `apply` verifies
epoch, roles, signatures and threshold; the queued record's `apply` writes
the record once. Both are planned by the same program code from the same
digest, so they cannot disagree; either refusal fails the whole transaction.

## D7. Spending limits are separate signatures with a counter

**Context.** F.8 lets a member or sub-quorum move value up to a limit
without the full quorum.

**Chosen.** A limit names its spender keys and sub-quorum; a spend is signed
over `(configuration, limit, uses, destination, asset, amount)`. `uses` is a
per-limit counter, so a spend signature is good once. A spend can only move
value from the vault to one destination.
