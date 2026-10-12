# lez_multisig: design

An M-of-N multisig program for the Logos Execution Zone v0.3 (tag `v0.3.0`,
commit `db66590a`), written against `lee_core` directly. This is the design
the prototype in this repository implements, and the design it is extended to
under RFP-005. Where the prototype does not yet implement a part, the section
says so under **Prototype status**.

Requirement ids refer to RFP-005: F.n (Functionality), U.n (Usability), R.n
(Reliability), P.n (Performance), S.n (Supportability), Pr.n (Privacy), and
the Coordination and Privacy Architecture sections.

## 1. The v0.3 constraints that shape everything

A v0.3 program runs twice per call. `plan` sees account ids, signer flags and
which shard each row selects, never data. It decides the effects, the chained
calls, the validity windows and the events. Each effect is then applied by the
same program against one shard's current bytes; `apply` may replace that
shard only if it is the program's own, and may not make calls.

So:

- **Nothing stored can be read while a call is planned.** An instruction
  carries what the caller claims (the configuration epoch, the digest, the
  time); the plan acts on it; `apply` checks every claim against the stored
  configuration and panics on a mismatch, which fails the whole transaction,
  the chained calls included.
- **Time comes from the block.** A plan cannot read a clock, but it can bound
  the block timestamp at which the transaction is valid, and the sequencer
  enforces the bound for public and private transactions alike. Every
  instruction that names a time `at` is valid only in `[at, at + 2 min)`, so
  a recorded time is never more than two minutes from the chain's own.
- **There are no program-owned accounts.** An account carries shards keyed by
  program id; native balance is the shard at `[0; 32]`. The vault is a PDA of
  this program whose native and token shards hold the funds. Nobody has its
  key; the only way to debit it is a chained call to which this program
  attaches the vault's PDA seed.
- **Signer authority propagates into chained calls.** A public account that
  signs the transaction is authorised in every call the transaction makes
  (`ExecutionState::authorize`). An action whose call named the fee payer's
  account would spend the payer's funds with the payer's own signature. The
  plan therefore refuses any action whose calls name an account that is
  authorised at the root (error 7106): the only authority an action carries
  is the vault's seed.

## 2. Keys and roles (F.6, Pr.1)

A member is an entry in the configuration:

```rust
pub struct Member {
    pub key: [u8; 32],   // BIP-340 x-only public key, an "approval key"
    pub roles: u8,       // PROPOSE = 1, APPROVE = 2, EXECUTE = 4
    pub weight: u32,     // 1 by default (soft F.1: weighted approvals)
}
```

The approval key is a secp256k1 key used only for this multisig. It is the
same key type as a LEZ account key, so the wallet's tooling makes and stores
it, but it **never signs a LEZ transaction**: approvals, proposals and the
execute authorisation are BIP-340 signatures over digests, verified by the
program inside `apply`. A member's LEZ account therefore never appears in a
multisig transaction; the only LEZ signer is a fee payer, who can be anyone
(Pr.1). Under the public posture the approval keys are visible in the
configuration; under the private posture they are inside a private account.

What each role guarantees, enforced by the program:

| Role | The program refuses | It cannot prevent |
|---|---|---|
| propose | a `queue` whose proposer signature is not from a current member with PROPOSE (7104) | a proposer sharing their key |
| approve | counting a signature from a key without APPROVE, from a non-member, twice from one key, or over a different digest or epoch (7104, 7105) | approvers colluding |
| execute | an `execute` without a signature from a current member with EXECUTE over `("execute", digest)` (7104) | an executor choosing *when* to execute inside the window |

A proposing key need not be an approving key: the roles are independent bits.

## 3. Accounts (all PDAs of the program's header account)

| Account | Seeds | This program's shard holds |
|---|---|---|
| configuration | `[multisig_id]` | `Multisig`: members, threshold, time lock, spending limits, `epoch`, `initialised`, counters |
| vault | `[multisig_id, "vault"]` | nothing; its native and token shards hold the funds |
| queued action | `[multisig_id, digest]` | `Queued`: the digest, the epoch it was approved under, `quorum_at`, expiry, time lock snapshot, approvers, status |
| registry entry | `["registry", program, idl_sha256]` | `RegistryEntry`: program, ImageID, IDL hash, name, registrant |

Multi-seed PDAs follow SPEL's rule (one seed direct, several hashed with
SHA-256), as in our vesting program.

## 4. The action and its digest (F.2, F.3, U.6)

```rust
pub enum Action {
    Calls(Vec<Call>),                 // F.3: any program, vault seed attached
    Config(ConfigChange),             // F.5: full replacement of members, threshold, time lock, limits
    CancelQueued { digest: [u8; 32] } // veto a queued action
}
pub struct Call { pub program: AccountId, pub accounts: Vec<CallAccount>, pub data: Vec<u8> }
```

`digest = SHA-256("lez_multisig/v1/action" ‖ configuration account ‖ epoch
(u32 LE) ‖ expires_at (u64 LE) ‖ borsh(action))`.

Everything a signer approves is in those bytes: the multisig, the
configuration it was approved under, the expiry, the target programs, every
account each call touches and which shard it selects, and the raw instruction
data. The client decodes the action **from these bytes** before a member signs
(U.6) and shows the proposer's free-text description separately, marked as
the proposer's. The program recomputes the digest from the action it is about
to run; it never trusts a digest a caller supplies.

## 5. Lifecycle (F.1, F.2, F.7, R.1)

Approvals are **collected off-chain**, in the coordination room (F.2, F.9), so
a pending proposal and its approvals are never on chain in either posture, and
reaching quorum costs one transaction rather than M + 1.

1. **Propose.** A PROPOSE member builds the action, computes the digest
   under the current epoch and an expiry, signs `("propose", digest)`, and
   posts the approval package (action bytes, epoch, expiry, signatures so far)
   to the room.
2. **Approve or reject.** Each APPROVE member's client decodes the action
   from the package bytes, checks the digest, shows it, and signs
   `("approve", digest)`. A rejection is a signed message the room tallies;
   the program never needs it.
3. **Queue.** Anyone holding the proposer's signature and approvals summing
   to at least the threshold weight submits `Queue`. `apply` on the
   configuration checks: the multisig is initialised; the epoch named is the
   current one; the proposer is a current PROPOSE member; every approval is a
   valid BIP-340 signature by a distinct current APPROVE member over this
   digest; their weights sum to the threshold. `apply` on the queued-action
   PDA writes the record once (a digest is queued once, 7103) with
   `quorum_at = at`.
4. **Execute.** After the time lock, an EXECUTE member signs
   `("execute", digest)` and anyone submits `Execute` with the action.
   `apply` checks the executor and that `quorum_at + time_lock <= at <
   expires_at`; the plan makes the action's chained calls with the vault's
   seed attached. The record moves to `Executed`; a digest executes once.

With a time lock of zero, `Queue` and `Execute` can be sent back to back; a
combined instruction is a later optimisation, not a protocol change.

**R.1, precisely.** A signature counts only for the digest it signs, and the
digest commits to the multisig, the epoch and the expiry, so it cannot be
moved to another proposal, another multisig or another configuration.
Distinctness is checked on member keys, so one member's two signatures count
once. The count is checked against the threshold stored in the configuration,
never one the caller names.

**R.2, the cutoff.** A configuration change increments `epoch`. An approval
signed under an older epoch never counts after it: `Queue` refuses a digest
whose epoch is not current (7102). **The cutoff is the `Queue` transaction**,
which is the moment an action enters its time-lock delay: a queued action
keeps the quorum that was verified when it was a valid quorum and may still
execute after its delay. A member removed while an action they approved sits
in its delay does not unqueue it; the remaining members can veto it with a
`CancelQueued` action at the new threshold, and `CancelQueued` is exempt from
the time lock so a veto can always land first. Configuration changes are
serialised: an `Action::Config` executes only if its epoch is still current,
so two changes queued under one configuration cannot both apply.

**Expiry (F.2).** `Queue` and `Execute` are refused at or after `expires_at`
(7108); expiry is bounded by the chain's timestamp through the `[at, at + 2
min)` window.

**Time lock (F.7).** `time_lock_ms` is per multisig and changed only through
`Action::Config`. The queued record snapshots it, so lowering the time lock
does not shorten a delay already running.

## 6. Initialisation (S.7: a vault cannot be drained before it is fully initialised)

`Create` writes the configuration with `initialised = false`. The vault
address exists from that moment (anyone may send to it), but every debit path
(`Queue`, `Execute`, `Spend`) refuses an uninitialised multisig (7101).
`Activate` sets `initialised` and requires approvals from current APPROVE
members summing to the threshold over `("activate", configuration, epoch)`, so
the members confirm the configuration they are bound by before it can move
funds. A multisig id that was never created has no configuration, and every
instruction refuses it (7100).

## 7. Spending limits (F.8)

A limit names a set of spender keys, a sub-quorum `min_signers`, an asset, an
amount per period and a period length. `Spend` pays from the vault to a
destination with `min_signers` spender signatures over `("spend",
configuration, limit index, uses, destination, asset, amount)`. `uses` is a
per-limit counter in the configuration, so a spend signature cannot be
replayed. The period restarts when `at` passes `period_start + period_ms`.
A spend can only move value out of the vault to one destination; it cannot
call another program or touch the configuration. Limits are part of the
configuration and change only through `Action::Config`.

## 8. The registry (F.4, U.6)

`Register { program, image_id, idl_sha256, name }` writes an append-only entry
at `["registry", program, idl_sha256]` naming who registered it. A client:

1. reads the target's header account and checks that its `program_loader`
   record holds the ImageID the entry names;
2. fetches the IDL from anywhere and checks it hashes to `idl_sha256`;
3. shows which registrants vouch for the entry, and whether more than one
   independent registrant agrees.

No single server is trusted for either the bytes or the IDL. **When a target
is not registered**, the client displays the target header, the ImageID read
from the chain, the raw instruction bytes in hex with their length, every
account the call touches and the shard it selects, and the warning *"This
program is not in the registry: the action cannot be decoded"*; the mini-app
will not let a member sign it from the default view. The native token program,
the token program and this program itself are decoded by built-in IDLs.

## 9. Coordination room (F.9, Coordination Architecture, Pr.2, Pr.3)

One end-to-end-encrypted room per multisig per epoch on the Logos chat module,
over Logos Delivery. The room carries deliberation and the approval packages.

**Binding a LEZ identity to a chat identity.** Each member signs, with the
approval key, `("bind", configuration, epoch, chat identity)`. The room admits
a chat identity only with a binding signed by a key in the current
configuration, which any member's client verifies against the chain (public
posture) or against the private configuration it can decrypt (private
posture). The binding never goes on chain.

**What a room participant can do, and how it is detected or tolerated:**

| Attack | Effect | Defence |
|---|---|---|
| withhold an approval or a package | delays quorum | liveness only; any member can re-post; the program needs any M, not specific ones |
| reorder | none | packages are independent; each digest stands alone |
| replay an approval | none | the digest binds multisig, epoch and expiry; the queued PDA is written once |
| inject a forged approval | none | the program verifies every signature against the configuration (R.1); the room carries no trust for validity |
| show different members different packages | a member may sign a digest the others did not see | the signer's client decodes the exact bytes it signs (U.6); the room is only a transport |

**Reconciliation on a configuration change.** A new epoch opens a new room
from the new member set; the old room is closed by its members' clients when
they see the epoch move. Until then, a removed member can still read
deliberation in the old room but cannot approve anything that counts: their
key is not in the configuration the program checks.

## 10. Privacy architecture (F.10, Pr.1–Pr.3)

Two postures, one program:

- **Public** (prototype): configuration, vault and queued actions are public
  PDAs. Anyone can inspect configuration, holdings and activity at any time.
  Even here, no member's LEZ account appears in a transaction (Pr.1 holds by
  the key design), and pending proposals stay in the room.
- **Private** (RFP-005 deliverable): the configuration, vault and queued
  records are private PDAs of this program, bound to a group nullifier key
  (`AccountId::for_private_pda`), the group-owned shared private account
  feature (logos-docs#321). Members hold the group keys from a Group Master
  Secret sealed to each member at creation; the group key lets a member read
  the multisig and build a transaction, **not spend**, because a debit needs
  the program to attach the vault's seed, which it does only after verifying
  the quorum. Approval verification then runs inside the privacy-preserving
  circuit, and the chain sees commitments, nullifiers and a proof.
- **Selective disclosure**: sharing the account family's viewing key with an
  audience (one auditor, or every DAO member) lets it decrypt configuration,
  vault and activity and follow every later change, with no spending power.
  Ongoing assurance follows from re-reading current state with the same key.

| Data item | Private posture | Public posture |
|---|---|---|
| existence of the multisig | commitment only | public account |
| member set, threshold, roles | private configuration | public configuration |
| per-signer approval attribution | private input and the room | queued record, public |
| pending-proposal metadata | room only | room only |
| action payload | private input; the target's own effects follow the target's accounts | public in `Execute` |
| vault balance | private | public |
| execution linkage | a privacy-preserving transaction with no member account | public `Execute` with a fee payer |
| coordination content | E2EE room | E2EE room |
| co-signing social graph | not readable from chain | readable from the configuration |

**Correlations left observable (Pr.3).** Timing between room traffic and
`Queue`/`Execute` transactions; the fee payer of public transactions (use a
fresh payer); the size of a transaction, which grows with M; and the
LEZ-to-chat binding, which lives only inside the room.

## 11. Approval verification cost (P.1, P.2)

Every approval is one BIP-340 verification in `apply`. The executor tests
measure the cycles `Queue` uses as a function of M through `lee::V03State`,
the state machine the sequencer applies, against the 10,000,000 gas cap per
transaction, and report the largest M that fits; `docs/BENCHMARKS.md` holds
the table. Real-proving time for the private posture is the first thing the
grant measures.

## 12. Refusal codes

| Code | Name | Meaning |
|---|---|---|
| 7100 | NoMultisig | no configuration at this id |
| 7101 | NotInitialised | the multisig has not been activated |
| 7102 | WrongEpoch | the epoch named is not the configuration's current epoch |
| 7103 | AlreadyExists | a configuration, queued action or registry entry already exists |
| 7104 | BadRole | the key is not a current member with the role this step needs |
| 7105 | BelowThreshold | fewer than the threshold weight of distinct valid approvals |
| 7106 | ForbiddenAccount | an action names a root-authorised account or this program |
| 7107 | TimeLocked | executed before `quorum_at + time_lock` |
| 7108 | Expired | queued or executed at or after the expiry |
| 7109 | NotQueued | no queued action for this digest, or it is not pending |
| 7110 | BadSignature | a signature does not verify |
| 7111 | BadConfig | members, threshold, roles or limits are invalid |
| 7112 | LimitExceeded | a spend exceeds the limit's remaining allowance |
| 7113 | BadLimit | no such limit, or the asset or counter does not match |
| 7114 | Accounts | wrong number or order of account rows |
| 7115 | NotSigned | a row that must sign does not |
| 7116 | Duplicate | the same key appears twice in one approval set |

## 13. Requirement traceability (prototype)

| Req. | Where | Test |
|---|---|---|
| F.1 M-of-N, configurable at creation | `Create`, `Multisig` | `executor-tests`: `create_and_activate`, `below_threshold_refused` |
| F.2 lifecycle, expiry, off-chain collection | `Queue`, `Execute`, CLI `propose`/`approve`/`queue` | `expired_refused`, `e2e` |
| F.3 any program | `Action::Calls` | `executes_token_transfer`, `executes_native_transfer` |
| F.4 registry | `Register` | `registry_append_only` |
| F.5 config through M-of-N | `Action::Config` | `config_change_bumps_epoch` |
| F.6 roles | `Member.roles` | `roles_are_independent` |
| F.7 time lock | `time_lock_ms`, `Queued.time_lock_ms` | `time_lock_enforced` |
| F.8 spending limit | `Spend` | `spend_within_limit`, `spend_replay_refused` |
| R.1 distinct, current, exact | `verify_quorum` | `duplicate_approver_refused`, `wrong_digest_refused`, `non_member_refused` |
| R.2 epoch cutoff | `epoch`, `Queue` | `stale_approvals_refused`, `queued_survives_change`, `cancel_queued_vetoes` |
| S.7 no drain before initialised | `initialised` | `uninitialised_vault_refused` |
| Pr.1 no member account in a tx | key design | `no_member_account_in_transactions` |
| P.2 cost vs M | `executor-tests/src/measure.rs` | `docs/BENCHMARKS.md` |
