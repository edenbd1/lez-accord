//! accord_multisig on LEZ v0.3: the wire types, the digests and the whole
//! program logic.
//!
//! The guest binary hands [`plan`] and [`apply`] to
//! `lee_core::program::run_program`. Everything else lives here so the CLI,
//! the host tests and the executor tests use the same types, seeds, digests
//! and checks as the program. DESIGN.md explains the model; the short version:
//!
//! - A member is a BIP-340 key with a role mask and a weight. Proposals,
//!   approvals, the execute authorisation, activation and spends are
//!   signatures over domain-separated digests, verified in `apply`. No
//!   member's LEZ account ever signs a multisig transaction; the only LEZ
//!   signer is whoever pays the fee.
//! - `plan` cannot read state, so it computes the digest from the action it
//!   carries and hands `apply` what it must check; `apply` checks the epoch,
//!   the members, the roles, the signatures and the threshold against the
//!   stored configuration and panics on any mismatch, which fails the whole
//!   transaction, chained calls included.
//! - The vault is a PDA nobody holds a key for. An approved action's calls
//!   carry the vault's PDA seed, which only this program can attach.

use borsh::{BorshDeserialize, BorshSerialize};
use k256::schnorr::{signature::hazmat::PrehashVerifier as _, Signature, VerifyingKey};
use lee_core::{
    account::{AccountId, ProgramShardSelector},
    native_token::{self, NATIVE_TOKEN_PROGRAM_ID},
    program::{AccountMeta, ChainedCall, PdaSeed, Plan, PlanInput, ProgramEvent, TimestampValidityWindow},
};
use risc0_zkvm::sha::{Impl, Sha256 as _};
use token_core::{TokenDescriptor, TokenKind};

pub mod errors;
use errors::*;

/// How far past the time it names an instruction may still land.
pub const WINDOW_MS: u64 = 120_000;
pub const MAX_MEMBERS: usize = 32;
pub const MAX_LIMITS: usize = 8;
/// `lee_core::program::MAX_NUMBER_CHAINED_CALLS`.
pub const MAX_CALLS: usize = 10;

pub const ROLE_PROPOSE: u8 = 1;
pub const ROLE_APPROVE: u8 = 2;
pub const ROLE_EXECUTE: u8 = 4;
pub const ROLE_ALL: u8 = ROLE_PROPOSE | ROLE_APPROVE | ROLE_EXECUTE;

pub const STATUS_QUEUED: u8 = 1;
pub const STATUS_EXECUTED: u8 = 2;
pub const STATUS_CANCELLED: u8 = 3;

pub const KIND_CALLS: u8 = 0;
pub const KIND_CONFIG: u8 = 1;
pub const KIND_CANCEL: u8 = 2;

/// The token program's account id on every LEZ v0.3 chain.
#[must_use]
pub fn token_program() -> AccountId {
    token_core::token_account_id()
}

// ------------------------------------------------------------------- types

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Asset {
    Native,
    Token { definition_id: AccountId },
}

impl Asset {
    #[must_use]
    pub fn shard_program(&self) -> AccountId {
        match self {
            Self::Native => NATIVE_TOKEN_PROGRAM_ID,
            Self::Token { .. } => token_program(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Member {
    /// BIP-340 x-only public key: the member's approval key.
    pub key: [u8; 32],
    /// [`ROLE_PROPOSE`] | [`ROLE_APPROVE`] | [`ROLE_EXECUTE`], independently.
    pub roles: u8,
    pub weight: u32,
}

/// A spending limit as configured: who may spend, how many of them must
/// sign, what, and how much per period.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct LimitSpec {
    pub spenders: Vec<[u8; 32]>,
    pub min_signers: u8,
    pub asset: Asset,
    pub amount: u128,
    pub period_ms: u64,
}

/// A spending limit as stored, with its running counters.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Limit {
    pub spec: LimitSpec,
    pub period_start: u64,
    pub spent: u128,
    /// Spends made under this limit; part of every spend digest, so a spend
    /// signature is good once.
    pub uses: u64,
}

/// A configuration. `Create` writes one, `Action::Config` replaces one.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Config {
    pub members: Vec<Member>,
    /// Minimum cumulative approval weight.
    pub threshold: u64,
    pub time_lock_ms: u64,
    pub limits: Vec<LimitSpec>,
}

/// The configuration account's shard.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Multisig {
    pub version: u8,
    pub initialised: u8,
    /// Incremented by every configuration change; approvals sign it.
    pub epoch: u32,
    pub members: Vec<Member>,
    pub threshold: u64,
    pub time_lock_ms: u64,
    pub limits: Vec<Limit>,
    pub vault: AccountId,
    pub creator: AccountId,
    pub queued: u64,
    pub executed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CallAccount {
    pub account: AccountId,
    /// Which shard of the account the call selects (a program's account id).
    pub shard_program: AccountId,
}

/// One call an action makes, with the vault's seed attached.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Call {
    pub program: AccountId,
    pub accounts: Vec<CallAccount>,
    /// The callee's instruction bytes, exactly as the callee decodes them.
    pub data: Vec<u8>,
}

/// What a proposal does. The variant order is the ABI.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Action {
    /// Calls to any program in this LEZ, each authorised as the vault (F.3).
    Calls(Vec<Call>),
    /// Replace members, threshold, time lock and limits (F.5).
    Config(Config),
    /// Veto a queued action; exempt from the time lock.
    CancelQueued { digest: [u8; 32] },
}

impl Action {
    #[must_use]
    pub fn kind(&self) -> u8 {
        match self {
            Self::Calls(_) => KIND_CALLS,
            Self::Config(_) => KIND_CONFIG,
            Self::CancelQueued { .. } => KIND_CANCEL,
        }
    }
}

/// A signature by a member's approval key.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Sig {
    pub key: [u8; 32],
    pub sig: [u8; 64],
}

/// A queued action's shard: written once, at quorum.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Queued {
    pub digest: [u8; 32],
    pub kind: u8,
    pub epoch: u32,
    pub quorum_at: u64,
    pub expires_at: u64,
    /// The time lock when quorum was reached; a later change does not shorten it.
    pub time_lock_ms: u64,
    pub proposer: [u8; 32],
    /// Attribution (public posture): who approved.
    pub approvers: Vec<[u8; 32]>,
    pub status: u8,
    pub executed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct RegistryEntry {
    pub program: AccountId,
    pub image_id: [u32; 8],
    pub idl_sha256: [u8; 32],
    pub name: String,
    pub registrant: AccountId,
}

/// The program's instruction. The variant order is the ABI.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Accounts: `[configuration, creator (signer handle)]`.
    Create { multisig_id: [u8; 32], config: Config },
    /// Accounts: `[configuration]`. Approvals over [`activate_message`].
    Activate { multisig_id: [u8; 32], epoch: u32, approvals: Vec<Sig> },
    /// Accounts: `[configuration, queued]`. Valid in `[at, at + WINDOW_MS)`.
    Queue {
        multisig_id: [u8; 32],
        epoch: u32,
        expires_at: u64,
        action: Action,
        time_lock_ms: u64,
        proposer: Sig,
        approvals: Vec<Sig>,
        at: u64,
    },
    /// Accounts: `[configuration, queued, ...]`: for `CancelQueued` the target
    /// queued record next, for `Calls` every account the calls name.
    Execute {
        multisig_id: [u8; 32],
        epoch: u32,
        expires_at: u64,
        action: Action,
        executor: Sig,
        at: u64,
    },
    /// Accounts: `[configuration, vault (asset shard), destination (asset shard)]`.
    Spend {
        multisig_id: [u8; 32],
        limit: u32,
        uses: u64,
        asset: Asset,
        amount: u128,
        approvals: Vec<Sig>,
        at: u64,
    },
    /// Accounts: `[entry, registrant (signer handle)]`.
    Register {
        program: AccountId,
        image_id: [u32; 8],
        idl_sha256: [u8; 32],
        name: String,
    },
}

/// What `plan` asks `apply` to do. Values computed by the plan (digests,
/// messages, row ids) are trusted: a program only applies effects it planned.
/// Values the caller claimed (epoch, time lock, uses, times) are checked.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Effect {
    Create(Multisig),
    Activate { epoch: u32, message: [u8; 32], approvals: Vec<Sig> },
    VerifyQueue {
        epoch: u32,
        digest: [u8; 32],
        time_lock_ms: u64,
        proposer: Sig,
        approvals: Vec<Sig>,
        at: u64,
        expires_at: u64,
    },
    WriteQueued(Queued),
    AuthorizeExecute { epoch: u32, digest: [u8; 32], executor: Sig, change: Option<Config> },
    MarkExecuted { digest: [u8; 32], at: u64, exempt_time_lock: bool },
    MarkCancelled { digest: [u8; 32] },
    Spend {
        limit: u32,
        uses: u64,
        asset: Asset,
        amount: u128,
        message: [u8; 32],
        approvals: Vec<Sig>,
        at: u64,
    },
    Register(RegistryEntry),
}

// --------------------------------------------------------------- addresses

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        buf.extend_from_slice(p);
    }
    Impl::hash_bytes(&buf)
        .as_bytes()
        .try_into()
        .expect("a SHA-256 digest is 32 bytes")
}

#[must_use]
pub fn seed_from_str(s: &str) -> [u8; 32] {
    let src = s.as_bytes();
    assert!(src.len() <= 32, "seed string exceeds 32 bytes");
    let mut out = [0u8; 32];
    out[..src.len()].copy_from_slice(src);
    out
}

/// SPEL's multi-seed rule: one seed as is, several hashed with SHA-256.
#[must_use]
pub fn combine_seeds(seeds: &[&[u8; 32]]) -> PdaSeed {
    match seeds {
        [one] => PdaSeed::new(**one),
        many => {
            let parts: Vec<&[u8]> = many.iter().map(|s| s.as_slice()).collect();
            PdaSeed::new(sha256(&parts))
        }
    }
}

#[must_use]
pub fn config_seed(multisig_id: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[multisig_id])
}

#[must_use]
pub fn vault_seed(multisig_id: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[multisig_id, &seed_from_str("vault")])
}

#[must_use]
pub fn queued_seed(multisig_id: &[u8; 32], digest: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[multisig_id, digest])
}

#[must_use]
pub fn registry_seed(program: &AccountId, idl_sha256: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[&seed_from_str("registry"), program.value(), idl_sha256])
}

#[must_use]
pub fn config_account(me: &AccountId, multisig_id: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(me, &config_seed(multisig_id))
}

#[must_use]
pub fn vault_account(me: &AccountId, multisig_id: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(me, &vault_seed(multisig_id))
}

#[must_use]
pub fn queued_account(me: &AccountId, multisig_id: &[u8; 32], digest: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(me, &queued_seed(multisig_id, digest))
}

#[must_use]
pub fn registry_account(me: &AccountId, program: &AccountId, idl_sha256: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(me, &registry_seed(program, idl_sha256))
}

// ----------------------------------------------------------------- digests

pub const TAG_ACTION: &[u8] = b"accord_multisig/v1/action";
pub const TAG_PROPOSE: &[u8] = b"accord_multisig/v1/propose";
pub const TAG_APPROVE: &[u8] = b"accord_multisig/v1/approve";
pub const TAG_EXECUTE: &[u8] = b"accord_multisig/v1/execute";
pub const TAG_ACTIVATE: &[u8] = b"accord_multisig/v1/activate";
pub const TAG_SPEND: &[u8] = b"accord_multisig/v1/spend";
pub const TAG_BIND: &[u8] = b"accord_multisig/v1/bind";

/// The digest every approval of `action` signs, under `epoch`, until `expires_at`.
#[must_use]
pub fn action_digest(config: &AccountId, epoch: u32, expires_at: u64, action: &Action) -> [u8; 32] {
    let body = borsh::to_vec(action).expect("borsh serialization is infallible");
    sha256(&[TAG_ACTION, config.value(), &epoch.to_le_bytes(), &expires_at.to_le_bytes(), &body])
}

#[must_use]
pub fn propose_message(digest: &[u8; 32]) -> [u8; 32] {
    sha256(&[TAG_PROPOSE, digest])
}

#[must_use]
pub fn approve_message(digest: &[u8; 32]) -> [u8; 32] {
    sha256(&[TAG_APPROVE, digest])
}

#[must_use]
pub fn execute_message(digest: &[u8; 32]) -> [u8; 32] {
    sha256(&[TAG_EXECUTE, digest])
}

#[must_use]
pub fn activate_message(config: &AccountId, epoch: u32) -> [u8; 32] {
    sha256(&[TAG_ACTIVATE, config.value(), &epoch.to_le_bytes()])
}

fn asset_bytes(asset: &Asset) -> [u8; 33] {
    let mut out = [0u8; 33];
    if let Asset::Token { definition_id } = asset {
        out[0] = 1;
        out[1..].copy_from_slice(definition_id.value());
    }
    out
}

#[must_use]
pub fn spend_message(
    config: &AccountId,
    limit: u32,
    uses: u64,
    destination: &AccountId,
    asset: &Asset,
    amount: u128,
) -> [u8; 32] {
    sha256(&[
        TAG_SPEND,
        config.value(),
        &limit.to_le_bytes(),
        &uses.to_le_bytes(),
        destination.value(),
        &asset_bytes(asset),
        &amount.to_le_bytes(),
    ])
}

/// What a member signs to bind a chat identity to their approval key, for the
/// room (never on chain).
#[must_use]
pub fn bind_message(config: &AccountId, epoch: u32, chat_identity: &[u8]) -> [u8; 32] {
    sha256(&[TAG_BIND, config.value(), &epoch.to_le_bytes(), chat_identity])
}

/// BIP-340 verification of `sig` by `key` over the 32-byte `message`, as LEZ
/// verifies its own witness sets (`k256` Schnorr over a prehash).
#[must_use]
pub fn verify(key: &[u8; 32], message: &[u8; 32], sig: &[u8; 64]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(key) else {
        return false;
    };
    let Ok(sig) = Signature::try_from(sig.as_slice()) else {
        return false;
    };
    vk.verify_prehash(message, &sig).is_ok()
}

// ------------------------------------------------------------------ events

/// `sha256("accord_multisig::<Name>")[..8]`.
#[must_use]
pub fn event_selector(name: &str) -> [u8; 8] {
    let full = sha256(&[b"accord_multisig::", name.as_bytes()]);
    full[..8].try_into().expect("8 of 32 bytes")
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Created {
    pub multisig_id: [u8; 32],
    pub vault: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Activated {
    pub multisig_id: [u8; 32],
    pub epoch: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ActionQueued {
    pub multisig_id: [u8; 32],
    pub digest: [u8; 32],
    pub kind: u8,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ActionExecuted {
    pub multisig_id: [u8; 32],
    pub digest: [u8; 32],
    pub kind: u8,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Spent {
    pub multisig_id: [u8; 32],
    pub limit: u32,
    pub amount: u128,
    pub destination: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Registered {
    pub program: AccountId,
    pub idl_sha256: [u8; 32],
}

pub const EVENT_NAMES: [&str; 6] = ["Created", "Activated", "ActionQueued", "ActionExecuted", "Spent", "Registered"];

fn event<T: BorshSerialize>(name: &str, body: &T) -> ProgramEvent {
    ProgramEvent {
        selector: event_selector(name),
        data: borsh::to_vec(body).expect("borsh serialization is infallible"),
    }
}

// -------------------------------------------------------------- validation

/// Refuse an invalid configuration (7111), or one whose keys repeat (7116).
pub fn validate_config(c: &Config) {
    refuse_unless(
        !c.members.is_empty() && c.members.len() <= MAX_MEMBERS,
        E_BAD_CONFIG,
        "a multisig has between 1 and 32 members",
    );
    let mut approve_weight: u64 = 0;
    let (mut proposers, mut executors) = (0, 0);
    for (i, m) in c.members.iter().enumerate() {
        refuse_unless(
            m.roles != 0 && m.roles & !ROLE_ALL == 0,
            E_BAD_CONFIG,
            "a member's roles are a non-empty subset of propose, approve, execute",
        );
        refuse_unless(m.weight >= 1, E_BAD_CONFIG, "a member's weight is at least 1");
        refuse_unless(VerifyingKey::from_bytes(&m.key).is_ok(), E_BAD_CONFIG, "a member key is not a valid BIP-340 key");
        refuse_unless(
            !c.members[..i].iter().any(|o| o.key == m.key),
            E_DUPLICATE,
            "a key appears twice among the members",
        );
        if m.roles & ROLE_APPROVE != 0 {
            approve_weight += u64::from(m.weight);
        }
        proposers += usize::from(m.roles & ROLE_PROPOSE != 0);
        executors += usize::from(m.roles & ROLE_EXECUTE != 0);
    }
    refuse_unless(proposers > 0 && executors > 0, E_BAD_CONFIG, "at least one member can propose and one can execute");
    refuse_unless(
        c.threshold >= 1 && c.threshold <= approve_weight,
        E_BAD_CONFIG,
        "the threshold is between 1 and the approvers' total weight",
    );
    refuse_unless(c.limits.len() <= MAX_LIMITS, E_BAD_CONFIG, "at most 8 spending limits");
    for l in &c.limits {
        refuse_unless(
            !l.spenders.is_empty() && l.min_signers >= 1 && usize::from(l.min_signers) <= l.spenders.len(),
            E_BAD_CONFIG,
            "a limit needs spenders and a sub-quorum between 1 and their number",
        );
        refuse_unless(l.amount > 0 && l.period_ms > 0, E_BAD_CONFIG, "a limit has a positive amount and period");
        for (i, s) in l.spenders.iter().enumerate() {
            refuse_unless(c.members.iter().any(|m| m.key == *s), E_BAD_CONFIG, "a spender is not a member");
            refuse_unless(!l.spenders[..i].contains(s), E_DUPLICATE, "a spender appears twice");
        }
    }
}

fn fresh_limits(specs: &[LimitSpec]) -> Vec<Limit> {
    specs
        .iter()
        .map(|spec| Limit {
            spec: spec.clone(),
            period_start: 0,
            spent: 0,
            uses: 0,
        })
        .collect()
}

/// The state `Create` writes.
#[must_use]
pub fn new_state(creator: AccountId, vault: AccountId, config: &Config) -> Multisig {
    validate_config(config);
    Multisig {
        version: 1,
        initialised: 0,
        epoch: 0,
        members: config.members.clone(),
        threshold: config.threshold,
        time_lock_ms: config.time_lock_ms,
        limits: fresh_limits(&config.limits),
        vault,
        creator,
        queued: 0,
        executed: 0,
    }
}

/// The weight of `approvals` over `message` from distinct current members
/// holding `role`. Refuses a non-member or wrong role (7104), a repeated key
/// (7116) or a bad signature (7110).
pub fn approval_weight(m: &Multisig, role: u8, message: &[u8; 32], approvals: &[Sig]) -> u64 {
    let mut weight = 0u64;
    for (i, a) in approvals.iter().enumerate() {
        refuse_unless(!approvals[..i].iter().any(|o| o.key == a.key), E_DUPLICATE, "a key signed twice");
        let member = m
            .members
            .iter()
            .find(|x| x.key == a.key)
            .unwrap_or_else(|| refuse(E_BAD_ROLE, "a signing key is not a current member"));
        refuse_unless(member.roles & role != 0, E_BAD_ROLE, "a signing member lacks the role this step needs");
        refuse_unless(verify(&a.key, message, &a.sig), E_BAD_SIGNATURE, "a signature does not verify");
        weight += u64::from(member.weight);
    }
    weight
}

// -------------------------------------------------------------------- plan

fn rows<const N: usize>(input: &PlanInput) -> [AccountMeta; N] {
    <[AccountMeta; N]>::try_from(input.accounts.clone())
        .unwrap_or_else(|_| refuse(E_ACCOUNTS, "wrong number of accounts for this instruction"))
}

fn expect_own(row: &AccountMeta, me: &AccountId, id: AccountId, what: &str) {
    refuse_unless(row.account_id == id && row.program_account_id == *me, E_ACCOUNTS, what);
}

fn expect_signer_handle(row: &AccountMeta, me: &AccountId) {
    refuse_unless(row.is_authorized, E_NOT_SIGNED, "the signer row carries no signature");
    refuse_unless(row.program_account_id == *me, E_ACCOUNTS, "a signer handle selects this program's shard");
}

fn window(plan: &mut Plan, at: u64) {
    let until = at.checked_add(WINDOW_MS).unwrap_or_else(|| refuse(E_EXPIRED, "time overflows"));
    plan.timestamp_window(TimestampValidityWindow::try_from(at..until).expect("a positive window is not empty"));
}

/// The chained calls of an approved `Calls` action, each carrying the
/// vault's seed. Refuses a call to this program, or naming any account that
/// is authorised at the root of this transaction (7106).
#[must_use]
pub fn action_calls(me: &AccountId, multisig_id: &[u8; 32], calls: &[Call], root: &[AccountMeta]) -> Vec<ChainedCall> {
    refuse_unless(!calls.is_empty() && calls.len() <= MAX_CALLS, E_ACCOUNTS, "an action makes between 1 and 10 calls");
    calls
        .iter()
        .map(|c| {
            refuse_unless(c.program != *me, E_FORBIDDEN_ACCOUNT, "an action may not call this program");
            for a in &c.accounts {
                refuse_unless(
                    !root.iter().any(|r| r.account_id == a.account && r.is_authorized),
                    E_FORBIDDEN_ACCOUNT,
                    "an action names an account that signs this transaction",
                );
            }
            ChainedCall {
                program_account_id: c.program,
                shard_selectors: c
                    .accounts
                    .iter()
                    .map(|a| ProgramShardSelector::new(a.account, a.shard_program))
                    .collect(),
                instruction_data: c.data.clone(),
                pda_seeds: vec![vault_seed(multisig_id)],
            }
        })
        .collect()
}

fn transfer_call(asset: &Asset, from: AccountId, to: AccountId, amount: u128) -> ChainedCall {
    let p = asset.shard_program();
    match asset {
        Asset::Native => ChainedCall::new(
            NATIVE_TOKEN_PROGRAM_ID,
            vec![ProgramShardSelector::new(from, p), ProgramShardSelector::new(to, p)],
            &native_token::Instruction::Transfer { amount },
        ),
        Asset::Token { definition_id } => ChainedCall::new(
            token_program(),
            vec![ProgramShardSelector::new(from, p), ProgramShardSelector::new(to, p)],
            &token_core::Instruction::Transfer {
                amount_to_transfer: amount,
                descriptor: TokenDescriptor {
                    definition_id: *definition_id,
                    kind: TokenKind::Fungible,
                },
            },
        ),
    }
}

/// The `Call` that moves `amount` of `asset` out of the vault, for clients
/// building transfer actions.
#[must_use]
pub fn vault_transfer(me: &AccountId, multisig_id: &[u8; 32], asset: &Asset, to: AccountId, amount: u128) -> Call {
    let c = transfer_call(asset, vault_account(me, multisig_id), to, amount);
    Call {
        program: c.program_account_id,
        accounts: c
            .shard_selectors
            .iter()
            .map(|s| CallAccount {
                account: s.account_id,
                shard_program: s.program_account_id,
            })
            .collect(),
        data: c.instruction_data,
    }
}

/// The plan entry point.
#[must_use]
pub fn plan(input: &PlanInput, ix: Instruction) -> Plan {
    let me = input.self_account_id;
    let mut plan = Plan::new(input);
    match ix {
        Instruction::Create { multisig_id, config } => {
            let [cfg, creator] = rows::<2>(input);
            expect_own(&cfg, &me, config_account(&me, &multisig_id), "the first row is the configuration PDA");
            expect_signer_handle(&creator, &me);
            let vault = vault_account(&me, &multisig_id);
            plan.effect(&cfg, &Effect::Create(new_state(creator.account_id, vault, &config)));
            plan.event(event("Created", &Created { multisig_id, vault }));
        }
        Instruction::Activate {
            multisig_id,
            epoch,
            approvals,
        } => {
            let [cfg] = rows::<1>(input);
            let config = config_account(&me, &multisig_id);
            expect_own(&cfg, &me, config, "the row is the configuration PDA");
            plan.effect(
                &cfg,
                &Effect::Activate {
                    epoch,
                    message: activate_message(&config, epoch),
                    approvals,
                },
            );
            plan.event(event("Activated", &Activated { multisig_id, epoch }));
        }
        Instruction::Queue {
            multisig_id,
            epoch,
            expires_at,
            action,
            time_lock_ms,
            proposer,
            approvals,
            at,
        } => {
            let [cfg, queued] = rows::<2>(input);
            let config = config_account(&me, &multisig_id);
            let digest = action_digest(&config, epoch, expires_at, &action);
            expect_own(&cfg, &me, config, "the first row is the configuration PDA");
            expect_own(&queued, &me, queued_account(&me, &multisig_id, &digest), "the second row is this action's queued PDA");
            window(&mut plan, at);
            let approvers = approvals.iter().map(|a| a.key).collect();
            plan.effect(
                &cfg,
                &Effect::VerifyQueue {
                    epoch,
                    digest,
                    time_lock_ms,
                    proposer: proposer.clone(),
                    approvals,
                    at,
                    expires_at,
                },
            );
            plan.effect(
                &queued,
                &Effect::WriteQueued(Queued {
                    digest,
                    kind: action.kind(),
                    epoch,
                    quorum_at: at,
                    expires_at,
                    time_lock_ms,
                    proposer: proposer.key,
                    approvers,
                    status: STATUS_QUEUED,
                    executed_at: 0,
                }),
            );
            plan.event(event(
                "ActionQueued",
                &ActionQueued {
                    multisig_id,
                    digest,
                    kind: action.kind(),
                    at,
                },
            ));
        }
        Instruction::Execute {
            multisig_id,
            epoch,
            expires_at,
            action,
            executor,
            at,
        } => {
            refuse_unless(input.accounts.len() >= 2, E_ACCOUNTS, "execute takes the configuration and the queued record first");
            let (cfg, queued) = (input.accounts[0].clone(), input.accounts[1].clone());
            let config = config_account(&me, &multisig_id);
            let digest = action_digest(&config, epoch, expires_at, &action);
            expect_own(&cfg, &me, config, "the first row is the configuration PDA");
            expect_own(&queued, &me, queued_account(&me, &multisig_id, &digest), "the second row is this action's queued PDA");
            window(&mut plan, at);
            let change = match &action {
                Action::Config(c) => Some(c.clone()),
                _ => None,
            };
            plan.effect(
                &cfg,
                &Effect::AuthorizeExecute {
                    epoch,
                    digest,
                    executor,
                    change,
                },
            );
            plan.effect(
                &queued,
                &Effect::MarkExecuted {
                    digest,
                    at,
                    exempt_time_lock: action.kind() == KIND_CANCEL,
                },
            );
            match &action {
                Action::Calls(calls) => {
                    for call in action_calls(&me, &multisig_id, calls, &input.accounts) {
                        plan.call(call);
                    }
                }
                Action::Config(_) => {
                    refuse_unless(input.accounts.len() == 2, E_ACCOUNTS, "a configuration change takes two rows");
                }
                Action::CancelQueued { digest: target } => {
                    refuse_unless(input.accounts.len() == 3, E_ACCOUNTS, "a cancellation takes the target queued record third");
                    refuse_unless(*target != digest, E_ACCOUNTS, "an action cannot cancel itself");
                    let row = &input.accounts[2];
                    expect_own(row, &me, queued_account(&me, &multisig_id, target), "the third row is the target's queued PDA");
                    plan.effect(row, &Effect::MarkCancelled { digest: *target });
                }
            }
            plan.event(event(
                "ActionExecuted",
                &ActionExecuted {
                    multisig_id,
                    digest,
                    kind: action.kind(),
                    at,
                },
            ));
        }
        Instruction::Spend {
            multisig_id,
            limit,
            uses,
            asset,
            amount,
            approvals,
            at,
        } => {
            let [cfg, vault, destination] = rows::<3>(input);
            let config = config_account(&me, &multisig_id);
            expect_own(&cfg, &me, config, "the first row is the configuration PDA");
            refuse_unless(
                vault.account_id == vault_account(&me, &multisig_id) && vault.program_account_id == asset.shard_program(),
                E_ACCOUNTS,
                "the second row is the vault's shard of this asset",
            );
            refuse_unless(destination.program_account_id == asset.shard_program(), E_ACCOUNTS, "the destination row selects the asset's shard");
            refuse_unless(!destination.is_authorized, E_FORBIDDEN_ACCOUNT, "a spend destination must not sign");
            refuse_unless(amount > 0, E_LIMIT_EXCEEDED, "a spend names a positive amount");
            window(&mut plan, at);
            plan.effect(
                &cfg,
                &Effect::Spend {
                    limit,
                    uses,
                    asset,
                    amount,
                    message: spend_message(&config, limit, uses, &destination.account_id, &asset, amount),
                    approvals,
                    at,
                },
            );
            let payout = match asset {
                Asset::Native => native_token::custody_transfer(vault.account_id, vault_seed(&multisig_id), destination.account_id, amount),
                Asset::Token { .. } => transfer_call(&asset, vault.account_id, destination.account_id, amount)
                    .with_pda_seeds(vec![vault_seed(&multisig_id)]),
            };
            plan.call(payout);
            plan.event(event(
                "Spent",
                &Spent {
                    multisig_id,
                    limit,
                    amount,
                    destination: destination.account_id,
                },
            ));
        }
        Instruction::Register {
            program,
            image_id,
            idl_sha256,
            name,
        } => {
            let [entry, registrant] = rows::<2>(input);
            expect_own(&entry, &me, registry_account(&me, &program, &idl_sha256), "the first row is the registry entry PDA");
            expect_signer_handle(&registrant, &me);
            refuse_unless(name.len() <= 64, E_BAD_CONFIG, "a registry name is at most 64 bytes");
            plan.effect(
                &entry,
                &Effect::Register(RegistryEntry {
                    program,
                    image_id,
                    idl_sha256,
                    name,
                    registrant: registrant.account_id,
                }),
            );
            plan.event(event("Registered", &Registered { program, idl_sha256 }));
        }
    }
    plan
}

// ------------------------------------------------------------------- apply

/// Decode a stored configuration, refusing an empty shard (7100).
#[must_use]
pub fn load(pre: &[u8]) -> Multisig {
    refuse_unless(!pre.is_empty(), E_NO_MULTISIG, "no multisig is configured at this id");
    Multisig::try_from_slice(pre).unwrap_or_else(|_| refuse(E_NO_MULTISIG, "configuration failed to deserialize"))
}

#[must_use]
pub fn load_queued(pre: &[u8]) -> Queued {
    refuse_unless(!pre.is_empty(), E_NOT_QUEUED, "no action is queued at this digest");
    Queued::try_from_slice(pre).unwrap_or_else(|_| refuse(E_NOT_QUEUED, "queued record failed to deserialize"))
}

fn store<T: BorshSerialize>(s: &T) -> Vec<u8> {
    borsh::to_vec(s).expect("borsh serialization is infallible")
}

/// The apply entry point: one effect against one shard.
#[must_use]
pub fn apply(effect: Effect, pre: &[u8]) -> Option<Vec<u8>> {
    match effect {
        Effect::Create(state) => {
            refuse_unless(pre.is_empty(), E_EXISTS, "a multisig already exists at this id");
            Some(store(&state))
        }
        Effect::Activate {
            epoch,
            message,
            approvals,
        } => {
            let mut m = load(pre);
            refuse_unless(m.initialised == 0, E_EXISTS, "the multisig is already active");
            refuse_unless(epoch == m.epoch, E_WRONG_EPOCH, "the activation names another epoch");
            let w = approval_weight(&m, ROLE_APPROVE, &message, &approvals);
            refuse_unless(w >= m.threshold, E_BELOW_THRESHOLD, "the activation is below the threshold");
            m.initialised = 1;
            Some(store(&m))
        }
        Effect::VerifyQueue {
            epoch,
            digest,
            time_lock_ms,
            proposer,
            approvals,
            at,
            expires_at,
        } => {
            let mut m = load(pre);
            refuse_unless(m.initialised != 0, E_NOT_INITIALISED, "the multisig is not active");
            refuse_unless(epoch == m.epoch, E_WRONG_EPOCH, "the approvals were made under another configuration");
            refuse_unless(time_lock_ms == m.time_lock_ms, E_WRONG_EPOCH, "the time lock named is not the configuration's");
            refuse_unless(at < expires_at, E_EXPIRED, "the proposal has expired");
            approval_weight(&m, ROLE_PROPOSE, &propose_message(&digest), core::slice::from_ref(&proposer));
            let w = approval_weight(&m, ROLE_APPROVE, &approve_message(&digest), &approvals);
            refuse_unless(w >= m.threshold, E_BELOW_THRESHOLD, "approvals are below the threshold");
            m.queued += 1;
            Some(store(&m))
        }
        Effect::WriteQueued(q) => {
            refuse_unless(pre.is_empty(), E_EXISTS, "this action is already queued");
            Some(store(&q))
        }
        Effect::AuthorizeExecute {
            epoch,
            digest,
            executor,
            change,
        } => {
            let mut m = load(pre);
            refuse_unless(m.initialised != 0, E_NOT_INITIALISED, "the multisig is not active");
            approval_weight(&m, ROLE_EXECUTE, &execute_message(&digest), core::slice::from_ref(&executor));
            if let Some(c) = change {
                // Configuration changes are serialised: one approved under an
                // epoch that has since moved does not apply.
                refuse_unless(epoch == m.epoch, E_WRONG_EPOCH, "the configuration changed since this change was approved");
                validate_config(&c);
                m.members = c.members;
                m.threshold = c.threshold;
                m.time_lock_ms = c.time_lock_ms;
                m.limits = fresh_limits(&c.limits);
                m.epoch = m.epoch.checked_add(1).unwrap_or_else(|| refuse(E_BAD_CONFIG, "epoch overflows"));
            }
            m.executed += 1;
            Some(store(&m))
        }
        Effect::MarkExecuted {
            digest,
            at,
            exempt_time_lock,
        } => {
            let mut q = load_queued(pre);
            refuse_unless(q.digest == digest && q.status == STATUS_QUEUED, E_NOT_QUEUED, "the action is not pending");
            refuse_unless(at < q.expires_at, E_EXPIRED, "the proposal has expired");
            refuse_unless(
                exempt_time_lock || at >= q.quorum_at.saturating_add(q.time_lock_ms),
                E_TIME_LOCKED,
                "the time lock has not elapsed",
            );
            q.status = STATUS_EXECUTED;
            q.executed_at = at;
            Some(store(&q))
        }
        Effect::MarkCancelled { digest } => {
            let mut q = load_queued(pre);
            refuse_unless(q.digest == digest && q.status == STATUS_QUEUED, E_NOT_QUEUED, "the target is not pending");
            q.status = STATUS_CANCELLED;
            Some(store(&q))
        }
        Effect::Spend {
            limit,
            uses,
            asset,
            amount,
            message,
            approvals,
            at,
        } => {
            let mut m = load(pre);
            refuse_unless(m.initialised != 0, E_NOT_INITIALISED, "the multisig is not active");
            let idx = usize::try_from(limit).unwrap_or(usize::MAX);
            refuse_unless(idx < m.limits.len(), E_BAD_LIMIT, "no such spending limit");
            let l = m.limits[idx].clone();
            refuse_unless(l.spec.asset == asset, E_BAD_LIMIT, "the limit is for another asset");
            refuse_unless(l.uses == uses, E_BAD_LIMIT, "the spend counter does not match: re-read the configuration");
            // Count distinct valid signatures from this limit's spenders.
            for (i, a) in approvals.iter().enumerate() {
                refuse_unless(!approvals[..i].iter().any(|o| o.key == a.key), E_DUPLICATE, "a spender signed twice");
                refuse_unless(l.spec.spenders.contains(&a.key), E_BAD_ROLE, "a signing key is not a spender of this limit");
                refuse_unless(verify(&a.key, &message, &a.sig), E_BAD_SIGNATURE, "a spend signature does not verify");
            }
            refuse_unless(approvals.len() >= usize::from(l.spec.min_signers), E_BELOW_THRESHOLD, "too few spender signatures");
            let (mut start, mut spent) = (l.period_start, l.spent);
            if at >= start.saturating_add(l.spec.period_ms) {
                start = at;
                spent = 0;
            }
            let after = spent.checked_add(amount).unwrap_or(u128::MAX);
            refuse_unless(after <= l.spec.amount, E_LIMIT_EXCEEDED, "the spend exceeds the limit for this period");
            let slot = &mut m.limits[idx];
            slot.period_start = start;
            slot.spent = after;
            slot.uses += 1;
            Some(store(&m))
        }
        Effect::Register(entry) => {
            refuse_unless(pre.is_empty(), E_EXISTS, "this registry entry exists");
            Some(store(&entry))
        }
    }
}

/// The account rows each instruction takes, in order, for clients. The plan
/// checks exactly these.
pub mod rows {
    use super::*;

    fn own(me: &AccountId, id: AccountId) -> ProgramShardSelector {
        ProgramShardSelector::new(id, *me)
    }

    #[must_use]
    pub fn create(me: &AccountId, multisig_id: &[u8; 32], creator: AccountId) -> Vec<ProgramShardSelector> {
        vec![own(me, config_account(me, multisig_id)), own(me, creator)]
    }

    #[must_use]
    pub fn activate(me: &AccountId, multisig_id: &[u8; 32]) -> Vec<ProgramShardSelector> {
        vec![own(me, config_account(me, multisig_id))]
    }

    #[must_use]
    pub fn queue(me: &AccountId, multisig_id: &[u8; 32], digest: &[u8; 32]) -> Vec<ProgramShardSelector> {
        vec![own(me, config_account(me, multisig_id)), own(me, queued_account(me, multisig_id, digest))]
    }

    /// `execute`'s rows: the configuration, the queued record, then what the
    /// action needs: the target record of a cancellation, or every account
    /// the calls name (each once, with the shard its first use selects).
    #[must_use]
    pub fn execute(me: &AccountId, multisig_id: &[u8; 32], digest: &[u8; 32], action: &Action) -> Vec<ProgramShardSelector> {
        let mut out = queue(me, multisig_id, digest);
        match action {
            Action::Calls(calls) => {
                for a in calls.iter().flat_map(|c| &c.accounts) {
                    let sel = ProgramShardSelector::new(a.account, a.shard_program);
                    if !out.contains(&sel) {
                        out.push(sel);
                    }
                }
            }
            Action::Config(_) => {}
            Action::CancelQueued { digest: target } => out.push(own(me, queued_account(me, multisig_id, target))),
        }
        out
    }

    #[must_use]
    pub fn spend(me: &AccountId, multisig_id: &[u8; 32], asset: &Asset, destination: AccountId) -> Vec<ProgramShardSelector> {
        vec![
            own(me, config_account(me, multisig_id)),
            ProgramShardSelector::new(vault_account(me, multisig_id), asset.shard_program()),
            ProgramShardSelector::new(destination, asset.shard_program()),
        ]
    }

    #[must_use]
    pub fn register(me: &AccountId, program: &AccountId, idl_sha256: &[u8; 32], registrant: AccountId) -> Vec<ProgramShardSelector> {
        vec![own(me, registry_account(me, program, idl_sha256)), own(me, registrant)]
    }
}

#[cfg(test)]
mod tests;
