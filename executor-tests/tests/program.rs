//! What the committed v0.3 accord_multisig binary accepts and refuses, run
//! through the LEE state machine the sequencer uses. Every refusal is asserted
//! by its code and checked to have moved nothing; one test fires every
//! refusal code the program defines.

use accord_core::{
    self as core, action_digest, activate_message, approve_message, config_account, errors::*, execute_message,
    propose_message, queued_account, rows, spend_message, vault_account, vault_transfer, Action, Asset, Call,
    CallAccount, Config, Instruction, LimitSpec, Member, Sig, ROLE_APPROVE, ROLE_EXECUTE, ROLE_PROPOSE,
    STATUS_CANCELLED, STATUS_EXECUTED, STATUS_QUEUED,
};
use accord_executor_tests::{code, Chain, Key, MSIG, MSIG2};
use lee::{AccountId, ProgramShardSelector};
use lee_core::native_token::NATIVE_TOKEN_PROGRAM_ID;
use std::collections::BTreeSet;

const ID: [u8; 32] = [0x31; 32];
const HOUR: u64 = 3_600_000;
const DEF: AccountId = AccountId::new([0xDE; 32]);
const DEST: AccountId = AccountId::new([0xD1; 32]);

struct W {
    c: Chain,
    payer: Key,
    /// Approval keys. k[0] proposes and approves, k[1] approves (and spends),
    /// k[2] approves and executes, k[3] and k[4] are outsiders until a change.
    k: Vec<Key>,
    cfg: AccountId,
    vault: AccountId,
}

fn member(k: &Key, roles: u8) -> Member {
    Member {
        key: k.approval_key(),
        roles,
        weight: 1,
    }
}

fn config(k: &[Key], time_lock_ms: u64) -> Config {
    Config {
        members: vec![
            member(&k[0], ROLE_PROPOSE | ROLE_APPROVE),
            member(&k[1], ROLE_APPROVE),
            member(&k[2], ROLE_APPROVE | ROLE_EXECUTE),
        ],
        threshold: 2,
        time_lock_ms,
        limits: vec![LimitSpec {
            spenders: vec![k[1].approval_key()],
            min_signers: 1,
            asset: Asset::Native,
            amount: 100,
            period_ms: 24 * HOUR,
        }],
    }
}

/// A chain with a funded payer and a vault already holding 1,000 native and
/// 5,000 tokens: anyone can send to the vault's address before the multisig
/// even exists.
fn world() -> W {
    let payer = Key::new(1);
    let k: Vec<Key> = (10..15).map(Key::new).collect();
    let vault = vault_account(&MSIG, &ID);
    let c = Chain::new(&[(payer.id, 1_000_000), (vault, 1_000)], &[(vault, DEF, 5_000)]);
    W {
        c,
        payer,
        k,
        cfg: config_account(&MSIG, &ID),
        vault,
    }
}

impl W {
    fn send(&mut self, ix: Instruction, r: Vec<ProgramShardSelector>) -> Result<(), String> {
        let payer = Key::new(1);
        self.c.send(&ix, r, &[&payer])
    }
    fn create(&mut self, cfg: Config) -> Result<(), String> {
        let r = rows::create(&MSIG, &ID, self.payer.id);
        self.send(Instruction::Create { multisig_id: ID, config: cfg }, r)
    }
    fn activate(&mut self, signers: &[usize]) -> Result<(), String> {
        let msg = activate_message(&self.cfg, self.ms().epoch);
        let approvals = signers.iter().map(|&i| self.k[i].sign(&msg)).collect();
        let epoch = self.ms().epoch;
        self.send(
            Instruction::Activate {
                multisig_id: ID,
                epoch,
                approvals,
            },
            rows::activate(&MSIG, &ID),
        )
    }
    fn ready(time_lock_ms: u64) -> Self {
        let mut w = world();
        let cfg = config(&w.k, time_lock_ms);
        w.create(cfg).unwrap();
        w.activate(&[0, 1]).unwrap();
        w
    }
    fn ms(&self) -> core::Multisig {
        self.c.multisig(MSIG, &ID)
    }
    fn digest(&self, epoch: u32, exp: u64, a: &Action) -> [u8; 32] {
        action_digest(&self.cfg, epoch, exp, a)
    }
    fn queue_with(&mut self, epoch: u32, exp: u64, a: &Action, proposer: Sig, approvals: Vec<Sig>) -> Result<[u8; 32], String> {
        let d = self.digest(epoch, exp, a);
        let ix = Instruction::Queue {
            multisig_id: ID,
            epoch,
            expires_at: exp,
            action: a.clone(),
            time_lock_ms: self.ms().time_lock_ms,
            proposer,
            approvals,
            at: self.c.now,
        };
        self.send(ix, rows::queue(&MSIG, &ID, &d)).map(|()| d)
    }
    fn queue(&mut self, exp: u64, a: &Action, approvers: &[usize]) -> Result<[u8; 32], String> {
        let epoch = self.ms().epoch;
        let d = self.digest(epoch, exp, a);
        let p = self.k[0].sign(&propose_message(&d));
        let ap = approvers.iter().map(|&i| self.k[i].sign(&approve_message(&d))).collect();
        self.queue_with(epoch, exp, a, p, ap)
    }
    fn execute_at_epoch(&mut self, epoch: u32, exp: u64, a: &Action, executor: usize) -> Result<(), String> {
        let d = self.digest(epoch, exp, a);
        let ix = Instruction::Execute {
            multisig_id: ID,
            epoch,
            expires_at: exp,
            action: a.clone(),
            executor: self.k[executor].sign(&execute_message(&d)),
            at: self.c.now,
        };
        self.send(ix, rows::execute(&MSIG, &ID, &d, a))
    }
    fn execute(&mut self, exp: u64, a: &Action, executor: usize) -> Result<(), String> {
        let epoch = self.ms().epoch;
        self.execute_at_epoch(epoch, exp, a, executor)
    }
    fn spend(&mut self, asset: Asset, amount: u128, signers: &[usize]) -> Result<(), String> {
        let uses = self.ms().limits[0].uses;
        let msg = spend_message(&self.cfg, 0, uses, &DEST, &asset, amount);
        let approvals = signers.iter().map(|&i| self.k[i].sign(&msg)).collect();
        let ix = Instruction::Spend {
            multisig_id: ID,
            limit: 0,
            uses,
            asset,
            amount,
            approvals,
            at: self.c.now,
        };
        self.send(ix, rows::spend(&MSIG, &ID, &asset, DEST))
    }
    fn exp(&self) -> u64 {
        self.c.now + HOUR
    }
    /// Snapshot of every balance a refusal must not move.
    fn balances(&self) -> (u128, u128, u128, u128) {
        (self.c.native(self.vault), self.c.token(self.vault), self.c.native(DEST), self.c.native(self.payer.id))
    }
}

fn pay(amount: u128) -> Action {
    Action::Calls(vec![vault_transfer(&MSIG, &ID, &Asset::Native, DEST, amount)])
}

fn pay_token(amount: u128) -> Action {
    Action::Calls(vec![vault_transfer(&MSIG, &ID, &Asset::Token { definition_id: DEF }, DEST, amount)])
}

#[track_caller]
fn refused(r: Result<impl std::fmt::Debug, String>, want: u32) {
    let e = r.expect_err("expected a refusal");
    assert_eq!(code(&e), Some(want), "{e}");
}

#[test]
fn full_lifecycle_pays_native_and_tokens_from_the_vault() {
    let mut w = W::ready(0);
    let exp = w.exp();
    let a = pay(40);
    let d = w.queue(exp, &a, &[0, 1]).unwrap();
    let q = w.c.queued(&ID, &d);
    assert_eq!((q.status, q.approvers.len()), (STATUS_QUEUED, 2));
    w.execute(exp, &a, 2).unwrap();
    assert_eq!(w.c.native(DEST), 40);
    assert_eq!(w.c.native(w.vault), 960);
    assert_eq!(w.c.queued(&ID, &d).status, STATUS_EXECUTED);
    let t = pay_token(1_234);
    w.queue(exp, &t, &[1, 2]).unwrap();
    w.execute(exp, &t, 2).unwrap();
    assert_eq!(w.c.token(DEST), 1_234);
    assert_eq!(w.c.token(w.vault), 5_000 - 1_234);
    let m = w.ms();
    assert_eq!((m.queued, m.executed), (2, 2));
}

#[test]
fn a_vault_cannot_be_drained_before_it_is_fully_initialised() {
    let mut w = world();
    let cfg = config(&w.k, 0);
    w.create(cfg).unwrap();
    let before = w.balances();
    let exp = w.exp();
    refused(w.queue(exp, &pay(10), &[0, 1, 2]), E_NOT_INITIALISED);
    refused(w.spend(Asset::Native, 10, &[1]), E_NOT_INITIALISED);
    // activation itself needs the threshold
    refused(w.activate(&[0]), E_BELOW_THRESHOLD);
    assert_eq!(w.balances(), before);
    w.activate(&[0, 2]).unwrap();
    w.queue(exp, &pay(10), &[0, 1]).unwrap();
    w.execute(exp, &pay(10), 2).unwrap();
    assert_eq!(w.c.native(DEST), 10);
}

#[test]
fn an_action_cannot_execute_below_threshold() {
    let mut w = W::ready(0);
    let exp = w.exp();
    let before = w.balances();
    refused(w.queue(exp, &pay(1), &[0]), E_BELOW_THRESHOLD);
    refused(w.queue(exp, &pay(1), &[1, 1]), E_DUPLICATE);
    refused(w.execute(exp, &pay(1), 2), E_NOT_QUEUED);
    assert_eq!(w.balances(), before);
}

#[test]
fn approvals_are_bound_to_one_action() {
    let mut w = W::ready(0);
    let exp = w.exp();
    let (a, b) = (pay(1), pay(500));
    let da = w.digest(0, exp, &a);
    // approvals of `a` presented for `b`
    let p = w.k[0].sign(&propose_message(&da));
    let ap = vec![w.k[0].sign(&approve_message(&da)), w.k[1].sign(&approve_message(&da))];
    refused(w.queue_with(0, exp, &b, p.clone(), ap.clone()), E_BAD_SIGNATURE);
    w.queue_with(0, exp, &a, p.clone(), ap.clone()).unwrap();
    // and the same package cannot queue `a` twice
    refused(w.queue_with(0, exp, &a, p, ap), E_EXISTS);
    // an outsider's signature counts for nothing
    refused(w.queue(exp, &b, &[0, 3]), E_BAD_ROLE);
}

#[test]
fn the_vault_can_be_the_authority_of_another_program() {
    // An approved action creates a multisig in another program instance with
    // the vault as its creator: the callee sees the vault as a signer because
    // this program attached the vault's seed, and only then.
    let mut w = W::ready(0);
    let exp = w.exp();
    let inner_id = [0x77; 32];
    let inner = Config {
        members: vec![member(&w.k[3], ROLE_PROPOSE | ROLE_APPROVE | ROLE_EXECUTE)],
        threshold: 1,
        time_lock_ms: 0,
        limits: vec![],
    };
    let ix = Instruction::Create {
        multisig_id: inner_id,
        config: inner,
    };
    let a = Action::Calls(vec![Call {
        program: MSIG2,
        accounts: vec![
            CallAccount {
                account: config_account(&MSIG2, &inner_id),
                shard_program: MSIG2,
            },
            CallAccount {
                account: w.vault,
                shard_program: MSIG2,
            },
        ],
        data: borsh::to_vec(&ix).unwrap(),
    }]);
    w.queue(exp, &a, &[0, 1]).unwrap();
    w.execute(exp, &a, 2).unwrap();
    assert_eq!(w.c.multisig(MSIG2, &inner_id).creator, w.vault);
    // Sent directly, without the vault's seed, the same call is refused: the
    // vault has no key.
    let direct = rows::create(&MSIG2, &[0x78; 32], w.vault);
    let payer = Key::new(1);
    let ix = Instruction::Create {
        multisig_id: [0x78; 32],
        config: Config {
            members: vec![member(&w.k[3], ROLE_PROPOSE | ROLE_APPROVE | ROLE_EXECUTE)],
            threshold: 1,
            time_lock_ms: 0,
            limits: vec![],
        },
    };
    refused(w.c.send_to(MSIG2, &ix, direct, &[&payer]), E_NOT_SIGNED);
}

#[test]
fn an_action_carries_only_the_vaults_authority() {
    let mut w = W::ready(0);
    let exp = w.exp();
    let steal = Action::Calls(vec![Call {
        program: NATIVE_TOKEN_PROGRAM_ID,
        accounts: vec![
            CallAccount {
                account: w.payer.id,
                shard_program: NATIVE_TOKEN_PROGRAM_ID,
            },
            CallAccount {
                account: DEST,
                shard_program: NATIVE_TOKEN_PROGRAM_ID,
            },
        ],
        data: borsh::to_vec(&lee_core::native_token::Instruction::Transfer { amount: 5 }).unwrap(),
    }]);
    w.queue(exp, &steal, &[0, 1]).unwrap();
    let before = w.balances();
    refused(w.execute(exp, &steal, 2), E_FORBIDDEN_ACCOUNT);
    assert_eq!(w.balances(), before);
    let me = Action::Calls(vec![Call {
        program: MSIG,
        accounts: vec![],
        data: vec![],
    }]);
    w.queue(exp, &me, &[0, 1]).unwrap();
    refused(w.execute(exp, &me, 2), E_FORBIDDEN_ACCOUNT);
}

#[test]
fn roles_are_enforced_independently() {
    let mut w = W::ready(0);
    let exp = w.exp();
    let a = pay(3);
    let d = w.digest(0, exp, &a);
    // k[1] approves but may not propose
    let p = w.k[1].sign(&propose_message(&d));
    let ap = vec![w.k[0].sign(&approve_message(&d)), w.k[1].sign(&approve_message(&d))];
    refused(w.queue_with(0, exp, &a, p, ap), E_BAD_ROLE);
    w.queue(exp, &a, &[0, 1]).unwrap();
    // k[0] may not execute; k[2] may
    refused(w.execute(exp, &a, 0), E_BAD_ROLE);
    w.execute(exp, &a, 2).unwrap();
}

#[test]
fn the_time_lock_runs_on_the_chains_clock() {
    let mut w = W::ready(10 * 60_000);
    let exp = w.exp();
    let a = pay(7);
    w.queue(exp, &a, &[0, 1]).unwrap();
    w.c.now += 9 * 60_000;
    refused(w.execute(exp, &a, 2), E_TIME_LOCKED);
    // naming a later time than the block's is refused by the sequencer's
    // window check, before the program runs
    let d = w.digest(0, exp, &a);
    let ix = Instruction::Execute {
        multisig_id: ID,
        epoch: 0,
        expires_at: exp,
        action: a.clone(),
        executor: w.k[2].sign(&execute_message(&d)),
        at: w.c.now + 60_000,
    };
    let e = w.send(ix, rows::execute(&MSIG, &ID, &d, &a)).unwrap_err();
    assert!(code(&e).is_none() && e.contains("validity window"), "{e}");
    w.c.now += 60_000;
    w.execute(exp, &a, 2).unwrap();
    assert_eq!(w.c.native(DEST), 7);
}

#[test]
fn proposals_expire() {
    let mut w = W::ready(0);
    let exp = w.c.now + 60_000;
    let a = pay(1);
    w.queue(exp, &a, &[0, 1]).unwrap();
    w.c.now = exp;
    refused(w.execute(exp, &a, 2), E_EXPIRED);
    refused(w.queue(exp, &pay(2), &[0, 1]), E_EXPIRED);
}

fn rotated(w: &W, threshold: u64, time_lock_ms: u64) -> Config {
    Config {
        members: vec![
            member(&w.k[0], ROLE_PROPOSE | ROLE_APPROVE),
            member(&w.k[2], ROLE_APPROVE | ROLE_EXECUTE),
            member(&w.k[3], ROLE_APPROVE),
        ],
        threshold,
        time_lock_ms,
        limits: vec![],
    }
}

#[test]
fn a_configuration_change_invalidates_pending_approvals() {
    let mut w = W::ready(0);
    let exp = w.exp();
    let a = pay(9);
    // approvals for `a` collected under epoch 0, not queued yet
    let d0 = w.digest(0, exp, &a);
    let p = w.k[0].sign(&propose_message(&d0));
    let ap = vec![w.k[0].sign(&approve_message(&d0)), w.k[1].sign(&approve_message(&d0))];
    // a change removes k[1] and adds k[3]: through the same M-of-N flow
    let change = Action::Config(rotated(&w, 2, 0));
    refused(w.queue(exp, &change, &[0]), E_BELOW_THRESHOLD);
    w.queue(exp, &change, &[0, 2]).unwrap();
    w.execute(exp, &change, 2).unwrap();
    assert_eq!(w.ms().epoch, 1);
    // the epoch-0 package no longer counts, under either label
    refused(w.queue_with(0, exp, &a, p.clone(), ap.clone()), E_WRONG_EPOCH);
    refused(w.queue_with(1, exp, &a, p, ap), E_BAD_SIGNATURE);
    // the removed member's fresh approval does not count either
    refused(w.queue(exp, &a, &[0, 1]), E_BAD_ROLE);
    // re-approved by the current members, it goes through
    w.queue(exp, &a, &[0, 3]).unwrap();
    w.execute(exp, &a, 2).unwrap();
    assert_eq!(w.c.native(DEST), 9);
}

#[test]
fn configuration_changes_are_serialised() {
    let mut w = W::ready(0);
    let exp = w.exp();
    let c1 = Action::Config(rotated(&w, 2, 0));
    let c2 = Action::Config(rotated(&w, 3, 0));
    w.queue(exp, &c1, &[0, 2]).unwrap();
    w.queue(exp, &c2, &[0, 2]).unwrap();
    w.execute(exp, &c1, 2).unwrap();
    refused(w.execute_at_epoch(0, exp, &c2, 2), E_WRONG_EPOCH);
    assert_eq!(w.ms().threshold, 2);
}

#[test]
fn a_queued_action_survives_a_change_and_the_new_quorum_can_veto_it() {
    let lock = 10 * 60_000;
    let mut w = W::ready(lock);
    let exp = w.c.now + 24 * HOUR;
    let a = pay(50);
    let da = w.queue(exp, &a, &[0, 1]).unwrap();
    // k[1], an approver of `a`, is removed during `a`'s delay
    let change = Action::Config(rotated(&w, 2, lock));
    w.queue(exp, &change, &[0, 2]).unwrap();
    w.c.now += lock;
    w.execute(exp, &change, 2).unwrap();
    // the cutoff is the queue: `a` keeps its quorum...
    assert_eq!(w.c.queued(&ID, &da).status, STATUS_QUEUED);
    // ...and the new configuration vetoes it at once: a veto skips the lock
    let veto = Action::CancelQueued { digest: da };
    w.queue(exp, &veto, &[0, 3]).unwrap();
    w.execute(exp, &veto, 2).unwrap();
    assert_eq!(w.c.queued(&ID, &da).status, STATUS_CANCELLED);
    let before = w.balances();
    refused(w.execute_at_epoch(0, exp, &a, 2), E_NOT_QUEUED);
    assert_eq!(w.balances(), before);
}

#[test]
fn a_queued_action_executes_after_a_change_when_not_vetoed() {
    let mut w = W::ready(0);
    let exp = w.exp();
    let a = pay(11);
    w.queue(exp, &a, &[0, 1]).unwrap();
    let change = Action::Config(rotated(&w, 2, 0));
    w.queue(exp, &change, &[0, 2]).unwrap();
    w.execute(exp, &change, 2).unwrap();
    w.execute_at_epoch(0, exp, &a, 2).unwrap();
    assert_eq!(w.c.native(DEST), 11);
}

#[test]
fn spending_limits_pay_small_amounts_without_the_quorum() {
    let mut w = W::ready(0);
    w.spend(Asset::Native, 60, &[1]).unwrap();
    w.spend(Asset::Native, 40, &[1]).unwrap();
    assert_eq!(w.c.native(DEST), 100);
    let before = w.balances();
    refused(w.spend(Asset::Native, 1, &[1]), E_LIMIT_EXCEEDED);
    refused(w.spend(Asset::Native, 1, &[0]), E_BAD_ROLE);
    refused(w.spend(Asset::Native, 1, &[]), E_BELOW_THRESHOLD);
    refused(w.spend(Asset::Token { definition_id: DEF }, 1, &[1]), E_BAD_LIMIT);
    assert_eq!(w.balances(), before);
    w.c.now += 24 * HOUR;
    w.spend(Asset::Native, 100, &[1]).unwrap();
    assert_eq!(w.c.native(DEST), 200);
}

#[test]
fn a_spend_signature_is_good_once() {
    let mut w = W::ready(0);
    let msg = spend_message(&w.cfg, 0, 0, &DEST, &Asset::Native, 10);
    let ix = Instruction::Spend {
        multisig_id: ID,
        limit: 0,
        uses: 0,
        asset: Asset::Native,
        amount: 10,
        approvals: vec![w.k[1].sign(&msg)],
        at: w.c.now,
    };
    w.send(ix.clone(), rows::spend(&MSIG, &ID, &Asset::Native, DEST)).unwrap();
    refused(w.send(ix, rows::spend(&MSIG, &ID, &Asset::Native, DEST)), E_BAD_LIMIT);
    assert_eq!(w.c.native(DEST), 10);
}

#[test]
fn the_registry_is_append_only() {
    let mut w = W::ready(0);
    let prog = programs::token_account_id();
    let ix = Instruction::Register {
        program: prog,
        image_id: [3; 8],
        idl_sha256: [4; 32],
        name: "token".into(),
    };
    let r = rows::register(&MSIG, &prog, &[4; 32], w.payer.id);
    w.send(ix.clone(), r.clone()).unwrap();
    refused(w.send(ix, r), E_EXISTS);
}

#[test]
fn no_member_account_appears_in_any_transaction() {
    // Pr.1: approvals, proposals and the execute authorisation are signatures
    // over digests. The only LEZ account that signs is the fee payer, and no
    // member key's account is ever named.
    let mut w = W::ready(0);
    let exp = w.exp();
    let a = pay(1);
    w.queue(exp, &a, &[0, 1]).unwrap();
    w.execute(exp, &a, 2).unwrap();
    w.spend(Asset::Native, 1, &[1]).unwrap();
    let signers: BTreeSet<_> = w.c.signers.iter().copied().collect();
    assert_eq!(signers, BTreeSet::from([w.payer.id]));
    for k in &w.k {
        assert!(!w.c.named.contains(&k.id), "a member's account was named");
    }
}

/// Every refusal code the program defines, fired through the real binary.
#[test]
fn every_refusal_code_fires_on_chain() {
    let mut seen = BTreeSet::new();
    let mut hit = |r: Result<(), String>| {
        let e = r.expect_err("expected a refusal");
        seen.insert(code(&e).unwrap_or_else(|| panic!("no code: {e}")));
    };

    // 7100: nothing at this id
    let mut w = world();
    hit(w.activate_unknown());
    // 7111, 7116: invalid configurations
    let mut bad = config(&w.k, 0);
    bad.threshold = 9;
    hit(w.create(bad));
    let mut dup = config(&w.k, 0);
    dup.members[1].key = dup.members[0].key;
    hit(w.create(dup));
    // 7115: Create without a signature on the creator row
    let r = rows::create(&MSIG, &ID, AccountId::new([0x55; 32]));
    hit(w.send(Instruction::Create { multisig_id: ID, config: config(&w.k, 0) }, r));
    // 7114: wrong rows
    hit(w.send(Instruction::Create { multisig_id: ID, config: config(&w.k, 0) }, rows::activate(&MSIG, &ID)));
    w.create(config(&w.k, 0)).unwrap();
    // 7103: created twice
    hit(w.create(config(&w.k, 0)));
    // 7101: not initialised
    let exp = w.exp();
    hit(w.queue(exp, &pay(1), &[0, 1]).map(|_| ()));
    // 7105: activation below threshold
    hit(w.activate(&[0]));
    w.activate(&[0, 1]).unwrap();
    // 7102: an epoch that is not current
    let a = pay(1);
    let d = w.digest(5, exp, &a);
    let p = w.k[0].sign(&propose_message(&d));
    let ap = vec![w.k[0].sign(&approve_message(&d)), w.k[1].sign(&approve_message(&d))];
    hit(w.queue_with(5, exp, &a, p, ap).map(|_| ()));
    // 7104: an outsider
    hit(w.queue(exp, &a, &[0, 4]).map(|_| ()));
    // 7110: a bad signature
    let d = w.digest(0, exp, &a);
    let p = w.k[0].sign(&propose_message(&d));
    let ap = vec![w.k[0].sign(&approve_message(&d)), w.k[1].sign(&execute_message(&d))];
    hit(w.queue_with(0, exp, &a, p, ap).map(|_| ()));
    // 7109: execute what was never queued
    hit(w.execute(exp, &a, 2));
    // 7106: an action naming the payer
    let steal = Action::Calls(vec![Call {
        program: NATIVE_TOKEN_PROGRAM_ID,
        accounts: vec![CallAccount {
            account: w.payer.id,
            shard_program: NATIVE_TOKEN_PROGRAM_ID,
        }],
        data: vec![],
    }]);
    w.queue(exp, &steal, &[0, 1]).unwrap();
    hit(w.execute(exp, &steal, 2));
    // 7108: expired
    let soon = w.c.now + 1;
    w.c.now = soon;
    hit(w.queue(soon, &pay(2), &[0, 1]).map(|_| ()));
    // 7112, 7113: spending limit
    hit(w.spend(Asset::Native, 101, &[1]));
    hit(w.spend(Asset::Token { definition_id: DEF }, 1, &[1]));
    // 7107: time lock (a fresh multisig with a lock)
    let mut t = W::ready(60_000);
    let e = t.exp();
    t.queue(e, &pay(1), &[0, 1]).unwrap();
    hit(t.execute(e, &pay(1), 2));

    let all: BTreeSet<u32> = ALL.iter().map(|(c, _)| *c).collect();
    assert_eq!(seen, all, "missing: {:?}", all.difference(&seen).collect::<Vec<_>>());
}

impl W {
    fn activate_unknown(&mut self) -> Result<(), String> {
        self.send(
            Instruction::Activate {
                multisig_id: ID,
                epoch: 0,
                approvals: vec![],
            },
            rows::activate(&MSIG, &ID),
        )
    }
}

#[test]
fn the_queued_record_is_the_queued_pda_of_the_digest() {
    let w = W::ready(0);
    let a = pay(1);
    let d = w.digest(0, w.exp(), &a);
    assert_eq!(rows::queue(&MSIG, &ID, &d)[1].account_id, queued_account(&MSIG, &ID, &d));
}
