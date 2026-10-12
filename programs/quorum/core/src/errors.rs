//! Refusal codes. A v0.3 program refuses by panicking, which fails the whole
//! transaction; the code opens the panic message (`E7105: ...`), so a refusal
//! reads the same in the sequencer log, the executor tests and the CLI.

pub const E_NO_MULTISIG: u32 = 7100;
pub const E_NOT_INITIALISED: u32 = 7101;
pub const E_WRONG_EPOCH: u32 = 7102;
pub const E_EXISTS: u32 = 7103;
pub const E_BAD_ROLE: u32 = 7104;
pub const E_BELOW_THRESHOLD: u32 = 7105;
pub const E_FORBIDDEN_ACCOUNT: u32 = 7106;
pub const E_TIME_LOCKED: u32 = 7107;
pub const E_EXPIRED: u32 = 7108;
pub const E_NOT_QUEUED: u32 = 7109;
pub const E_BAD_SIGNATURE: u32 = 7110;
pub const E_BAD_CONFIG: u32 = 7111;
pub const E_LIMIT_EXCEEDED: u32 = 7112;
pub const E_BAD_LIMIT: u32 = 7113;
pub const E_ACCOUNTS: u32 = 7114;
pub const E_NOT_SIGNED: u32 = 7115;
pub const E_DUPLICATE: u32 = 7116;

/// Every code with its name, in the order the IDL lists them.
pub const ALL: [(u32, &str); 17] = [
    (E_NO_MULTISIG, "NoMultisig"),
    (E_NOT_INITIALISED, "NotInitialised"),
    (E_WRONG_EPOCH, "WrongEpoch"),
    (E_EXISTS, "AlreadyExists"),
    (E_BAD_ROLE, "BadRole"),
    (E_BELOW_THRESHOLD, "BelowThreshold"),
    (E_FORBIDDEN_ACCOUNT, "ForbiddenAccount"),
    (E_TIME_LOCKED, "TimeLocked"),
    (E_EXPIRED, "Expired"),
    (E_NOT_QUEUED, "NotQueued"),
    (E_BAD_SIGNATURE, "BadSignature"),
    (E_BAD_CONFIG, "BadConfig"),
    (E_LIMIT_EXCEEDED, "LimitExceeded"),
    (E_BAD_LIMIT, "BadLimit"),
    (E_ACCOUNTS, "Accounts"),
    (E_NOT_SIGNED, "NotSigned"),
    (E_DUPLICATE, "Duplicate"),
];

/// What a refusal means for the person who hit it, for clients (U.7).
#[must_use]
pub fn advice(code: u32) -> &'static str {
    match code {
        E_NO_MULTISIG => "No multisig exists at this id. Check the id and the program header.",
        E_NOT_INITIALISED => "The multisig has not been activated: collect threshold approvals of the activation and send `activate` first.",
        E_WRONG_EPOCH => "The configuration changed since these signatures were made. Re-propose under the current epoch.",
        E_EXISTS => "This was already done: the action is queued, or the id is taken.",
        E_BAD_ROLE => "A signing key is not a current member with the role this step needs (propose, approve or execute).",
        E_BELOW_THRESHOLD => "Not enough approval weight: collect more approvals from distinct current approvers.",
        E_FORBIDDEN_ACCOUNT => "The action names an account that signs this transaction, or this program itself. Only the vault's authority may reach an action.",
        E_TIME_LOCKED => "The time lock has not elapsed. Execute after the time shown.",
        E_EXPIRED => "The proposal expired. Propose it again.",
        E_NOT_QUEUED => "No pending queued action matches this digest: it was never queued, already executed, or cancelled.",
        E_BAD_SIGNATURE => "A signature does not verify over this action. Re-collect it from the package you are submitting.",
        E_BAD_CONFIG => "The configuration is invalid: check members, roles, weights, threshold and limits.",
        E_LIMIT_EXCEEDED => "The spend exceeds what the limit allows in this period.",
        E_BAD_LIMIT => "No such spending limit, or the asset or counter does not match: re-read the configuration.",
        E_ACCOUNTS => "The transaction's accounts are not the ones this instruction takes.",
        E_NOT_SIGNED => "A row that must sign does not.",
        E_DUPLICATE => "The same key appears twice: each member counts once.",
        _ => "Unknown refusal.",
    }
}

/// Refuse the transaction with `code`.
#[track_caller]
pub fn refuse(code: u32, why: &str) -> ! {
    panic!("E{code}: {why}")
}

#[track_caller]
pub fn refuse_unless(ok: bool, code: u32, why: &str) {
    if !ok {
        refuse(code, why);
    }
}
