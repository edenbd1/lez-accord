//! Host tests of plan and apply, without a zkVM: every effect a plan emits is
//! applied to an in-memory map of this program's shards, the way the runtime
//! applies them. The executor tests run the same logic inside the guest
//! binary through the LEE state machine, chained calls included.

use super::*;
use k256::schnorr::SigningKey;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

const ME: AccountId = AccountId::new([7; 32]);
const PAYER: AccountId = AccountId::new([1; 32]);
const ID: [u8; 32] = [9; 32];
const T0: u64 = 1_700_000_000_000;

struct K(SigningKey);

impl K {
    fn new(i: u8) -> Self {
        let mut b = [0u8; 32];
        b[0] = i;
        b[31] = 1;
        Self(SigningKey::from_bytes(&b).unwrap())
    }
    fn key(&self) -> [u8; 32] {
        self.0.verifying_key().to_bytes().into()
    }
    fn sign(&self, msg: &[u8; 32]) -> Sig {
        use k256::schnorr::signature::hazmat::PrehashSigner as _;
        let s: Signature = self.0.sign_prehash(msg).unwrap();
        Sig {
            key: self.key(),
            sig: s.to_bytes(),
        }
    }
}

fn member(k: &K, roles: u8) -> Member {
    Member {
        key: k.key(),
        roles,
        weight: 1,
    }
}

fn refusal<T>(f: impl FnOnce() -> T) -> String {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(_) => panic!("expected a refusal"),
        Err(e) => e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default(),
    }
}

fn code(msg: &str) -> u32 {
    msg[1..5].parse().unwrap_or_else(|_| panic!("no code in {msg:?}"))
}

#[derive(Default)]
struct World {
    shards: HashMap<AccountId, Vec<u8>>,
    calls: Vec<ChainedCall>,
}

fn meta(id: AccountId, prog: AccountId, signed: bool) -> AccountMeta {
    AccountMeta::new(id, signed, prog)
}

impl World {
    fn send(&mut self, ix: Instruction, accounts: Vec<AccountMeta>) -> Result<(), u32> {
        let input = PlanInput {
            self_account_id: ME,
            caller_account_id: None,
            accounts,
            instruction_data: borsh::to_vec(&ix).unwrap(),
        };
        let mut staged = self.shards.clone();
        let r = catch_unwind(AssertUnwindSafe(|| {
            let p = plan(&input, ix);
            for e in &p.output().effects {
                let effect = Effect::try_from_slice(&e.data).unwrap();
                let pre = staged.get(&e.selector.account_id).cloned().unwrap_or_default();
                if let Some(post) = apply(effect, &pre) {
                    staged.insert(e.selector.account_id, post);
                }
            }
            (staged, p.output().chained_calls.clone())
        }));
        match r {
            Ok((s, calls)) => {
                self.shards = s;
                self.calls = calls;
                Ok(())
            }
            Err(e) => {
                let m = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                Err(code(&m))
            }
        }
    }
    fn ms(&self) -> Multisig {
        load(&self.shards[&config_account(&ME, &ID)])
    }
    fn queued(&self, d: &[u8; 32]) -> Queued {
        load_queued(&self.shards[&queued_account(&ME, &ID, d)])
    }
}

fn cfg_row() -> AccountMeta {
    meta(config_account(&ME, &ID), ME, false)
}

struct Fx {
    w: World,
    keys: Vec<K>,
}

/// A 2-of-3 multisig: key 0 proposes and approves, key 1 approves, key 2
/// approves and executes. Created and activated.
fn setup(time_lock_ms: u64) -> Fx {
    let keys: Vec<K> = (1..=4).map(K::new).collect();
    let config = Config {
        members: vec![
            member(&keys[0], ROLE_PROPOSE | ROLE_APPROVE),
            member(&keys[1], ROLE_APPROVE),
            member(&keys[2], ROLE_APPROVE | ROLE_EXECUTE),
        ],
        threshold: 2,
        time_lock_ms,
        limits: vec![LimitSpec {
            spenders: vec![keys[1].key()],
            min_signers: 1,
            asset: Asset::Native,
            amount: 100,
            period_ms: 86_400_000,
        }],
    };
    let mut w = World::default();
    w.send(
        Instruction::Create { multisig_id: ID, config },
        vec![cfg_row(), meta(PAYER, ME, true)],
    )
    .unwrap();
    let msg = activate_message(&config_account(&ME, &ID), 0);
    w.send(
        Instruction::Activate {
            multisig_id: ID,
            epoch: 0,
            approvals: vec![keys[0].sign(&msg), keys[1].sign(&msg)],
        },
        vec![cfg_row()],
    )
    .unwrap();
    Fx { w, keys }
}

fn pay(to: AccountId, amount: u128) -> Action {
    Action::Calls(vec![vault_transfer(&ME, &ID, &Asset::Native, to, amount)])
}

impl Fx {
    fn digest(&self, epoch: u32, exp: u64, a: &Action) -> [u8; 32] {
        action_digest(&config_account(&ME, &ID), epoch, exp, a)
    }
    fn queue(&mut self, epoch: u32, exp: u64, a: &Action, approvers: &[usize], at: u64) -> Result<[u8; 32], u32> {
        let d = self.digest(epoch, exp, a);
        let m = self.w.ms();
        let ix = Instruction::Queue {
            multisig_id: ID,
            epoch,
            expires_at: exp,
            action: a.clone(),
            time_lock_ms: m.time_lock_ms,
            proposer: self.keys[0].sign(&propose_message(&d)),
            approvals: approvers.iter().map(|&i| self.keys[i].sign(&approve_message(&d))).collect(),
            at,
        };
        self.w
            .send(ix, vec![cfg_row(), meta(queued_account(&ME, &ID, &d), ME, false)])
            .map(|()| d)
    }
    fn execute(&mut self, epoch: u32, exp: u64, a: &Action, executor: usize, at: u64, extra: Vec<AccountMeta>) -> Result<(), u32> {
        let d = self.digest(epoch, exp, a);
        let ix = Instruction::Execute {
            multisig_id: ID,
            epoch,
            expires_at: exp,
            action: a.clone(),
            executor: self.keys[executor].sign(&execute_message(&d)),
            at,
        };
        let mut rows = vec![cfg_row(), meta(queued_account(&ME, &ID, &d), ME, false)];
        rows.extend(extra);
        self.w.send(ix, rows)
    }
}

const EXP: u64 = T0 + 3_600_000;
const DEST: AccountId = AccountId::new([5; 32]);

fn vault_rows() -> Vec<AccountMeta> {
    vec![
        meta(vault_account(&ME, &ID), NATIVE_TOKEN_PROGRAM_ID, false),
        meta(DEST, NATIVE_TOKEN_PROGRAM_ID, false),
    ]
}

#[test]
fn create_writes_an_uninitialised_configuration() {
    let keys: Vec<K> = (1..=2).map(K::new).collect();
    let mut w = World::default();
    let config = Config {
        members: vec![member(&keys[0], ROLE_ALL), member(&keys[1], ROLE_ALL)],
        threshold: 2,
        time_lock_ms: 0,
        limits: vec![],
    };
    w.send(Instruction::Create { multisig_id: ID, config: config.clone() }, vec![cfg_row(), meta(PAYER, ME, true)])
        .unwrap();
    let m = w.ms();
    assert_eq!((m.initialised, m.epoch, m.threshold), (0, 0, 2));
    assert_eq!(m.vault, vault_account(&ME, &ID));
    // A second creation at the same id is refused.
    assert_eq!(
        w.send(Instruction::Create { multisig_id: ID, config }, vec![cfg_row(), meta(PAYER, ME, true)]),
        Err(E_EXISTS)
    );
}

#[test]
fn create_refuses_invalid_configurations() {
    let k: Vec<K> = (1..=2).map(K::new).collect();
    let bad = |members: Vec<Member>, threshold: u64| Config {
        members,
        threshold,
        time_lock_ms: 0,
        limits: vec![],
    };
    let cases = [
        (bad(vec![], 1), E_BAD_CONFIG),
        (bad(vec![member(&k[0], ROLE_ALL)], 2), E_BAD_CONFIG),
        (bad(vec![member(&k[0], ROLE_ALL)], 0), E_BAD_CONFIG),
        (bad(vec![member(&k[0], ROLE_ALL), member(&k[0], ROLE_ALL)], 1), E_DUPLICATE),
        (bad(vec![member(&k[0], ROLE_APPROVE)], 1), E_BAD_CONFIG),
        (bad(vec![member(&k[0], 8)], 1), E_BAD_CONFIG),
        (
            bad(
                vec![Member {
                    key: k[0].key(),
                    roles: ROLE_ALL,
                    weight: 0,
                }],
                1,
            ),
            E_BAD_CONFIG,
        ),
        (
            bad(
                vec![Member {
                    key: [0xff; 32],
                    roles: ROLE_ALL,
                    weight: 1,
                }],
                1,
            ),
            E_BAD_CONFIG,
        ),
    ];
    for (c, want) in cases {
        let mut w = World::default();
        assert_eq!(w.send(Instruction::Create { multisig_id: ID, config: c }, vec![cfg_row(), meta(PAYER, ME, true)]), Err(want));
    }
}

#[test]
fn create_needs_a_signer_and_the_configuration_pda() {
    let k = K::new(1);
    let config = Config {
        members: vec![member(&k, ROLE_ALL)],
        threshold: 1,
        time_lock_ms: 0,
        limits: vec![],
    };
    let mut w = World::default();
    assert_eq!(
        w.send(Instruction::Create { multisig_id: ID, config: config.clone() }, vec![cfg_row(), meta(PAYER, ME, false)]),
        Err(E_NOT_SIGNED)
    );
    assert_eq!(
        w.send(Instruction::Create { multisig_id: ID, config }, vec![meta(DEST, ME, false), meta(PAYER, ME, true)]),
        Err(E_ACCOUNTS)
    );
}

#[test]
fn activation_needs_the_threshold() {
    let keys: Vec<K> = (1..=3).map(K::new).collect();
    let config = Config {
        members: keys.iter().map(|k| member(k, ROLE_ALL)).collect(),
        threshold: 2,
        time_lock_ms: 0,
        limits: vec![],
    };
    let mut w = World::default();
    w.send(Instruction::Create { multisig_id: ID, config }, vec![cfg_row(), meta(PAYER, ME, true)]).unwrap();
    let msg = activate_message(&config_account(&ME, &ID), 0);
    let act = |a: Vec<Sig>| Instruction::Activate {
        multisig_id: ID,
        epoch: 0,
        approvals: a,
    };
    assert_eq!(w.send(act(vec![keys[0].sign(&msg)]), vec![cfg_row()]), Err(E_BELOW_THRESHOLD));
    assert_eq!(w.send(act(vec![keys[0].sign(&msg), keys[0].sign(&msg)]), vec![cfg_row()]), Err(E_DUPLICATE));
    let other = activate_message(&config_account(&ME, &[1; 32]), 0);
    assert_eq!(w.send(act(vec![keys[0].sign(&msg), keys[1].sign(&other)]), vec![cfg_row()]), Err(E_BAD_SIGNATURE));
    w.send(act(vec![keys[0].sign(&msg), keys[2].sign(&msg)]), vec![cfg_row()]).unwrap();
    assert_eq!(w.ms().initialised, 1);
    assert_eq!(w.send(act(vec![keys[0].sign(&msg), keys[2].sign(&msg)]), vec![cfg_row()]), Err(E_EXISTS));
}

#[test]
fn uninitialised_vault_cannot_be_debited() {
    let keys: Vec<K> = (1..=2).map(K::new).collect();
    let config = Config {
        members: keys.iter().map(|k| member(k, ROLE_ALL)).collect(),
        threshold: 1,
        time_lock_ms: 0,
        limits: vec![LimitSpec {
            spenders: vec![keys[0].key()],
            min_signers: 1,
            asset: Asset::Native,
            amount: 100,
            period_ms: 1000,
        }],
    };
    let mut fx = Fx {
        w: World::default(),
        keys,
    };
    fx.w.send(Instruction::Create { multisig_id: ID, config }, vec![cfg_row(), meta(PAYER, ME, true)]).unwrap();
    let a = pay(DEST, 10);
    assert_eq!(fx.queue(0, EXP, &a, &[0], T0), Err(E_NOT_INITIALISED));
    let msg = spend_message(&config_account(&ME, &ID), 0, 0, &DEST, &Asset::Native, 10);
    let spend = Instruction::Spend {
        multisig_id: ID,
        limit: 0,
        uses: 0,
        asset: Asset::Native,
        amount: 10,
        approvals: vec![fx.keys[0].sign(&msg)],
        at: T0,
    };
    let mut rows = vec![cfg_row()];
    rows.extend(vault_rows());
    assert_eq!(fx.w.send(spend, rows), Err(E_NOT_INITIALISED));
    // And a multisig that was never created cannot be touched at all.
    let mut empty = World::default();
    assert_eq!(
        empty.send(
            Instruction::Activate {
                multisig_id: ID,
                epoch: 0,
                approvals: vec![]
            },
            vec![cfg_row()]
        ),
        Err(E_NO_MULTISIG)
    );
}

#[test]
fn queue_then_execute_pays_from_the_vault() {
    let mut fx = setup(0);
    let a = pay(DEST, 40);
    let d = fx.queue(0, EXP, &a, &[0, 1], T0).unwrap();
    let q = fx.w.queued(&d);
    assert_eq!((q.status, q.quorum_at, q.approvers.len()), (STATUS_QUEUED, T0, 2));
    fx.execute(0, EXP, &a, 2, T0 + 1, vault_rows()).unwrap();
    assert_eq!(fx.w.queued(&d).status, STATUS_EXECUTED);
    let call = &fx.w.calls[0];
    assert_eq!(call.program_account_id, NATIVE_TOKEN_PROGRAM_ID);
    assert_eq!(call.pda_seeds, vec![vault_seed(&ID)]);
    assert_eq!(call.shard_selectors[0].account_id, vault_account(&ME, &ID));
    // A digest executes once.
    assert_eq!(fx.execute(0, EXP, &a, 2, T0 + 2, vault_rows()), Err(E_NOT_QUEUED));
    assert_eq!(fx.queue(0, EXP, &a, &[0, 1], T0 + 3), Err(E_EXISTS));
}

#[test]
fn below_threshold_is_refused() {
    let mut fx = setup(0);
    let a = pay(DEST, 1);
    assert_eq!(fx.queue(0, EXP, &a, &[0], T0), Err(E_BELOW_THRESHOLD));
    assert_eq!(fx.queue(0, EXP, &a, &[], T0), Err(E_BELOW_THRESHOLD));
    // Executing an action that never reached quorum finds no queued record.
    assert_eq!(fx.execute(0, EXP, &a, 2, T0, vault_rows()), Err(E_NOT_QUEUED));
}

#[test]
fn duplicate_approver_counts_once() {
    let mut fx = setup(0);
    assert_eq!(fx.queue(0, EXP, &pay(DEST, 1), &[1, 1], T0), Err(E_DUPLICATE));
}

#[test]
fn non_member_and_wrong_digest_are_refused() {
    let mut fx = setup(0);
    // key 3 is not a member
    assert_eq!(fx.queue(0, EXP, &pay(DEST, 1), &[0, 3], T0), Err(E_BAD_ROLE));
    // an approval over another action does not count for this one
    let a = pay(DEST, 1);
    let b = pay(DEST, 2);
    let d = fx.digest(0, EXP, &a);
    let other = fx.digest(0, EXP, &b);
    let ix = Instruction::Queue {
        multisig_id: ID,
        epoch: 0,
        expires_at: EXP,
        action: a,
        time_lock_ms: 0,
        proposer: fx.keys[0].sign(&propose_message(&d)),
        approvals: vec![fx.keys[0].sign(&approve_message(&d)), fx.keys[1].sign(&approve_message(&other))],
        at: T0,
    };
    assert_eq!(fx.w.send(ix, vec![cfg_row(), meta(queued_account(&ME, &ID, &d), ME, false)]), Err(E_BAD_SIGNATURE));
}

#[test]
fn approve_signature_cannot_stand_in_for_propose_or_execute() {
    let mut fx = setup(0);
    let a = pay(DEST, 1);
    let d = fx.digest(0, EXP, &a);
    let ix = Instruction::Queue {
        multisig_id: ID,
        epoch: 0,
        expires_at: EXP,
        action: a.clone(),
        time_lock_ms: 0,
        // the proposer's *approval* signature is not a proposal signature
        proposer: fx.keys[0].sign(&approve_message(&d)),
        approvals: vec![fx.keys[0].sign(&approve_message(&d)), fx.keys[1].sign(&approve_message(&d))],
        at: T0,
    };
    assert_eq!(fx.w.send(ix, vec![cfg_row(), meta(queued_account(&ME, &ID, &d), ME, false)]), Err(E_BAD_SIGNATURE));
    fx.queue(0, EXP, &a, &[0, 1], T0).unwrap();
    let ix = Instruction::Execute {
        multisig_id: ID,
        epoch: 0,
        expires_at: EXP,
        action: a,
        executor: fx.keys[2].sign(&approve_message(&d)),
        at: T0,
    };
    let mut rows = vec![cfg_row(), meta(queued_account(&ME, &ID, &d), ME, false)];
    rows.extend(vault_rows());
    assert_eq!(fx.w.send(ix, rows), Err(E_BAD_SIGNATURE));
}

#[test]
fn roles_are_independent() {
    let mut fx = setup(0);
    let a = pay(DEST, 1);
    let d = fx.digest(0, EXP, &a);
    // key 1 approves but cannot propose
    let ix = Instruction::Queue {
        multisig_id: ID,
        epoch: 0,
        expires_at: EXP,
        action: a.clone(),
        time_lock_ms: 0,
        proposer: fx.keys[1].sign(&propose_message(&d)),
        approvals: vec![fx.keys[0].sign(&approve_message(&d)), fx.keys[1].sign(&approve_message(&d))],
        at: T0,
    };
    assert_eq!(fx.w.send(ix, vec![cfg_row(), meta(queued_account(&ME, &ID, &d), ME, false)]), Err(E_BAD_ROLE));
    fx.queue(0, EXP, &a, &[0, 1], T0).unwrap();
    // key 0 proposes and approves but cannot execute; key 2 can
    assert_eq!(fx.execute(0, EXP, &a, 0, T0, vault_rows()), Err(E_BAD_ROLE));
    fx.execute(0, EXP, &a, 2, T0, vault_rows()).unwrap();
}

#[test]
fn expired_proposals_are_refused() {
    let mut fx = setup(0);
    let a = pay(DEST, 1);
    assert_eq!(fx.queue(0, T0, &a, &[0, 1], T0), Err(E_EXPIRED));
    fx.queue(0, T0 + 10, &a, &[0, 1], T0).unwrap();
    assert_eq!(fx.execute(0, T0 + 10, &a, 2, T0 + 10, vault_rows()), Err(E_EXPIRED));
    fx.execute(0, T0 + 10, &a, 2, T0 + 9, vault_rows()).unwrap();
}

#[test]
fn time_lock_is_enforced_and_snapshotted() {
    let mut fx = setup(60_000);
    let a = pay(DEST, 1);
    // the queue must name the configuration's time lock
    let d = fx.digest(0, EXP, &a);
    let ix = Instruction::Queue {
        multisig_id: ID,
        epoch: 0,
        expires_at: EXP,
        action: a.clone(),
        time_lock_ms: 0,
        proposer: fx.keys[0].sign(&propose_message(&d)),
        approvals: vec![fx.keys[0].sign(&approve_message(&d)), fx.keys[1].sign(&approve_message(&d))],
        at: T0,
    };
    assert_eq!(fx.w.send(ix, vec![cfg_row(), meta(queued_account(&ME, &ID, &d), ME, false)]), Err(E_WRONG_EPOCH));
    fx.queue(0, EXP, &a, &[0, 1], T0).unwrap();
    assert_eq!(fx.execute(0, EXP, &a, 2, T0 + 59_999, vault_rows()), Err(E_TIME_LOCKED));
    fx.execute(0, EXP, &a, 2, T0 + 60_000, vault_rows()).unwrap();
}

fn new_config(fx: &Fx, threshold: u64, time_lock_ms: u64) -> Config {
    Config {
        members: vec![
            member(&fx.keys[0], ROLE_PROPOSE | ROLE_APPROVE),
            member(&fx.keys[2], ROLE_APPROVE | ROLE_EXECUTE),
            member(&fx.keys[3], ROLE_APPROVE),
        ],
        threshold,
        time_lock_ms,
        limits: vec![],
    }
}

#[test]
fn config_change_goes_through_the_quorum_and_bumps_the_epoch() {
    let mut fx = setup(0);
    let change = Action::Config(new_config(&fx, 3, 0));
    // one approval is not enough to change the configuration either
    assert_eq!(fx.queue(0, EXP, &change, &[0], T0), Err(E_BELOW_THRESHOLD));
    fx.queue(0, EXP, &change, &[0, 2], T0).unwrap();
    fx.execute(0, EXP, &change, 2, T0, vec![]).unwrap();
    let m = fx.w.ms();
    assert_eq!((m.epoch, m.threshold, m.members.len()), (1, 3, 3));
    assert!(m.members.iter().all(|x| x.key != fx.keys[1].key()));
    // key 1 was removed: its approvals no longer count
    let a = pay(DEST, 1);
    assert_eq!(fx.queue(1, EXP, &a, &[0, 1, 2], T0 + 1), Err(E_BAD_ROLE));
    fx.queue(1, EXP, &a, &[0, 2, 3], T0 + 1).unwrap();
}

#[test]
fn approvals_under_the_old_epoch_do_not_count_after_a_change() {
    let mut fx = setup(0);
    let a = pay(DEST, 1);
    // approvals collected under epoch 0, not yet queued
    let d0 = fx.digest(0, EXP, &a);
    let old = (
        fx.keys[0].sign(&propose_message(&d0)),
        vec![fx.keys[0].sign(&approve_message(&d0)), fx.keys[2].sign(&approve_message(&d0))],
    );
    let change = Action::Config(new_config(&fx, 2, 0));
    fx.queue(0, EXP, &change, &[0, 2], T0).unwrap();
    fx.execute(0, EXP, &change, 2, T0, vec![]).unwrap();
    // the epoch-0 package is refused: its epoch is not current
    let ix = Instruction::Queue {
        multisig_id: ID,
        epoch: 0,
        expires_at: EXP,
        action: a.clone(),
        time_lock_ms: 0,
        proposer: old.0.clone(),
        approvals: old.1.clone(),
        at: T0 + 1,
    };
    assert_eq!(fx.w.send(ix, vec![cfg_row(), meta(queued_account(&ME, &ID, &d0), ME, false)]), Err(E_WRONG_EPOCH));
    // and relabelling it as epoch 1 changes the digest, so the signatures fail
    let d1 = fx.digest(1, EXP, &a);
    let ix = Instruction::Queue {
        multisig_id: ID,
        epoch: 1,
        expires_at: EXP,
        action: a,
        time_lock_ms: 0,
        proposer: old.0,
        approvals: old.1,
        at: T0 + 1,
    };
    assert_eq!(fx.w.send(ix, vec![cfg_row(), meta(queued_account(&ME, &ID, &d1), ME, false)]), Err(E_BAD_SIGNATURE));
}

#[test]
fn a_queued_action_survives_a_change_and_can_be_vetoed() {
    let mut fx = setup(600_000);
    let a = pay(DEST, 1);
    let queued = fx.queue(0, EXP, &a, &[0, 1], T0).unwrap();
    // key 1, an approver of `a`, is removed while `a` sits in its delay
    let change = Action::Config(new_config(&fx, 2, 600_000));
    fx.queue(0, EXP, &change, &[0, 2], T0).unwrap();
    fx.execute(0, EXP, &change, 2, T0 + 600_000, vec![]).unwrap();
    assert_eq!(fx.w.ms().epoch, 1);
    // the cutoff is the queue: `a` reached quorum before the change, so it stays executable...
    assert_eq!(fx.w.queued(&queued).status, STATUS_QUEUED);
    // ...unless the new configuration vetoes it, which skips the time lock
    let veto = Action::CancelQueued { digest: queued };
    fx.queue(1, EXP, &veto, &[0, 3], T0 + 600_001).unwrap();
    fx.execute(1, EXP, &veto, 2, T0 + 600_001, vec![meta(queued_account(&ME, &ID, &queued), ME, false)])
        .unwrap();
    assert_eq!(fx.w.queued(&queued).status, STATUS_CANCELLED);
    assert_eq!(fx.execute(0, EXP, &a, 2, T0 + 700_000, vault_rows()), Err(E_NOT_QUEUED));
}

#[test]
fn queued_action_executes_after_a_change_when_not_vetoed() {
    let mut fx = setup(0);
    let a = pay(DEST, 1);
    fx.queue(0, EXP, &a, &[0, 1], T0).unwrap();
    let change = Action::Config(new_config(&fx, 2, 0));
    fx.queue(0, EXP, &change, &[0, 2], T0).unwrap();
    fx.execute(0, EXP, &change, 2, T0, vec![]).unwrap();
    fx.execute(0, EXP, &a, 2, T0 + 1, vault_rows()).unwrap();
}

#[test]
fn config_changes_are_serialised() {
    let mut fx = setup(0);
    let c1 = Action::Config(new_config(&fx, 2, 0));
    let c2 = Action::Config(new_config(&fx, 3, 0));
    fx.queue(0, EXP, &c1, &[0, 2], T0).unwrap();
    fx.queue(0, EXP, &c2, &[0, 2], T0).unwrap();
    fx.execute(0, EXP, &c1, 2, T0, vec![]).unwrap();
    assert_eq!(fx.execute(0, EXP, &c2, 2, T0, vec![]), Err(E_WRONG_EPOCH));
}

#[test]
fn an_invalid_config_change_is_refused_at_execution() {
    let mut fx = setup(0);
    let mut c = new_config(&fx, 2, 0);
    c.threshold = 9;
    let change = Action::Config(c);
    fx.queue(0, EXP, &change, &[0, 2], T0).unwrap();
    assert_eq!(fx.execute(0, EXP, &change, 2, T0, vec![]), Err(E_BAD_CONFIG));
}

#[test]
fn actions_may_not_carry_a_signers_authority_or_call_this_program() {
    let mut fx = setup(0);
    // an action that would move the fee payer's own funds
    let a = Action::Calls(vec![Call {
        program: NATIVE_TOKEN_PROGRAM_ID,
        accounts: vec![
            CallAccount {
                account: PAYER,
                shard_program: NATIVE_TOKEN_PROGRAM_ID,
            },
            CallAccount {
                account: DEST,
                shard_program: NATIVE_TOKEN_PROGRAM_ID,
            },
        ],
        data: borsh::to_vec(&native_token::Instruction::Transfer { amount: 5 }).unwrap(),
    }]);
    fx.queue(0, EXP, &a, &[0, 1], T0).unwrap();
    let rows = vec![meta(PAYER, NATIVE_TOKEN_PROGRAM_ID, true), meta(DEST, NATIVE_TOKEN_PROGRAM_ID, false)];
    assert_eq!(fx.execute(0, EXP, &a, 2, T0, rows), Err(E_FORBIDDEN_ACCOUNT));
    let me_call = Action::Calls(vec![Call {
        program: ME,
        accounts: vec![],
        data: vec![],
    }]);
    fx.queue(0, EXP, &me_call, &[0, 1], T0).unwrap();
    assert_eq!(fx.execute(0, EXP, &me_call, 2, T0, vec![]), Err(E_FORBIDDEN_ACCOUNT));
}

#[test]
fn the_digest_commits_to_everything() {
    let c = config_account(&ME, &ID);
    let a = pay(DEST, 1);
    let base = action_digest(&c, 0, EXP, &a);
    assert_ne!(base, action_digest(&c, 1, EXP, &a));
    assert_ne!(base, action_digest(&c, 0, EXP + 1, &a));
    assert_ne!(base, action_digest(&config_account(&ME, &[1; 32]), 0, EXP, &a));
    assert_ne!(base, action_digest(&c, 0, EXP, &pay(DEST, 2)));
    assert_ne!(base, action_digest(&c, 0, EXP, &pay(PAYER, 1)));
    // and the per-step messages are domain-separated
    let m = [propose_message(&base), approve_message(&base), execute_message(&base)];
    assert!(m[0] != m[1] && m[1] != m[2] && m[0] != m[2]);
}

fn spend(fx: &mut Fx, uses: u64, amount: u128, signers: &[usize], at: u64) -> Result<(), u32> {
    let msg = spend_message(&config_account(&ME, &ID), 0, uses, &DEST, &Asset::Native, amount);
    let ix = Instruction::Spend {
        multisig_id: ID,
        limit: 0,
        uses,
        asset: Asset::Native,
        amount,
        approvals: signers.iter().map(|&i| fx.keys[i].sign(&msg)).collect(),
        at,
    };
    let mut rows = vec![cfg_row()];
    rows.extend(vault_rows());
    fx.w.send(ix, rows)
}

#[test]
fn spending_limit_allows_small_transfers_without_the_quorum() {
    let mut fx = setup(0);
    spend(&mut fx, 0, 60, &[1], T0).unwrap();
    assert_eq!(fx.w.calls[0].pda_seeds, vec![vault_seed(&ID)]);
    spend(&mut fx, 1, 40, &[1], T0 + 1).unwrap();
    // the period's allowance is used up
    assert_eq!(spend(&mut fx, 2, 1, &[1], T0 + 2), Err(E_LIMIT_EXCEEDED));
    // a new period restores it
    spend(&mut fx, 2, 100, &[1], T0 + 86_400_000).unwrap();
    let l = &fx.w.ms().limits[0];
    assert_eq!((l.uses, l.spent, l.period_start), (3, 100, T0 + 86_400_000));
}

#[test]
fn spend_signatures_are_good_once_and_only_from_spenders() {
    let mut fx = setup(0);
    spend(&mut fx, 0, 10, &[1], T0).unwrap();
    // replaying uses = 0 is refused
    assert_eq!(spend(&mut fx, 0, 10, &[1], T0), Err(E_BAD_LIMIT));
    // key 0 is a member but not a spender of this limit
    assert_eq!(spend(&mut fx, 1, 10, &[0], T0), Err(E_BAD_ROLE));
    assert_eq!(spend(&mut fx, 1, 10, &[], T0), Err(E_BELOW_THRESHOLD));
    // a signature over another amount does not verify
    let msg = spend_message(&config_account(&ME, &ID), 0, 1, &DEST, &Asset::Native, 11);
    let ix = Instruction::Spend {
        multisig_id: ID,
        limit: 0,
        uses: 1,
        asset: Asset::Native,
        amount: 10,
        approvals: vec![fx.keys[1].sign(&msg)],
        at: T0,
    };
    let mut rows = vec![cfg_row()];
    rows.extend(vault_rows());
    assert_eq!(fx.w.send(ix, rows), Err(E_BAD_SIGNATURE));
}

#[test]
fn spend_refuses_another_asset_or_a_signing_destination() {
    let mut fx = setup(0);
    let tok = Asset::Token {
        definition_id: AccountId::new([4; 32]),
    };
    let msg = spend_message(&config_account(&ME, &ID), 0, 0, &DEST, &tok, 10);
    let ix = Instruction::Spend {
        multisig_id: ID,
        limit: 0,
        uses: 0,
        asset: tok,
        amount: 10,
        approvals: vec![fx.keys[1].sign(&msg)],
        at: T0,
    };
    let rows = vec![
        cfg_row(),
        meta(vault_account(&ME, &ID), token_program(), false),
        meta(DEST, token_program(), false),
    ];
    assert_eq!(fx.w.send(ix, rows), Err(E_BAD_LIMIT));
    let msg = spend_message(&config_account(&ME, &ID), 0, 0, &DEST, &Asset::Native, 10);
    let ix = Instruction::Spend {
        multisig_id: ID,
        limit: 0,
        uses: 0,
        asset: Asset::Native,
        amount: 10,
        approvals: vec![fx.keys[1].sign(&msg)],
        at: T0,
    };
    let rows = vec![
        cfg_row(),
        meta(vault_account(&ME, &ID), NATIVE_TOKEN_PROGRAM_ID, false),
        meta(DEST, NATIVE_TOKEN_PROGRAM_ID, true),
    ];
    assert_eq!(fx.w.send(ix, rows), Err(E_FORBIDDEN_ACCOUNT));
}

#[test]
fn spend_refuses_a_signing_vault_row() {
    let mut fx = setup(0);
    let msg = spend_message(&config_account(&ME, &ID), 0, 0, &DEST, &Asset::Native, 10);
    let ix = Instruction::Spend {
        multisig_id: ID,
        limit: 0,
        uses: 0,
        asset: Asset::Native,
        amount: 10,
        approvals: vec![fx.keys[1].sign(&msg)],
        at: T0,
    };
    let rows = vec![cfg_row(), meta(PAYER, NATIVE_TOKEN_PROGRAM_ID, true), meta(DEST, NATIVE_TOKEN_PROGRAM_ID, false)];
    assert_eq!(fx.w.send(ix, rows), Err(E_FORBIDDEN_ACCOUNT));
}

#[test]
fn registry_is_append_only() {
    let mut w = World::default();
    let prog = AccountId::new([3; 32]);
    let ix = Instruction::Register {
        program: prog,
        image_id: [1; 8],
        idl_sha256: [2; 32],
        name: "token".into(),
    };
    let rows = vec![meta(registry_account(&ME, &prog, &[2; 32]), ME, false), meta(PAYER, ME, true)];
    w.send(ix.clone(), rows.clone()).unwrap();
    assert_eq!(w.send(ix, rows), Err(E_EXISTS));
}

#[test]
fn every_refusal_code_is_named_and_advised() {
    for (c, name) in ALL {
        assert!(!name.is_empty());
        assert_ne!(advice(c), "Unknown refusal.");
    }
}

#[test]
fn signatures_match_the_lez_scheme() {
    // `verify` is k256's BIP-340 over a 32-byte prehash, the scheme LEZ uses
    // for its own witness sets, so a wallet key works as an approval key.
    let k = K::new(1);
    let msg = [3u8; 32];
    let s = k.sign(&msg);
    assert!(verify(&s.key, &msg, &s.sig));
    assert!(!verify(&s.key, &[4u8; 32], &s.sig));
    let _ = refusal(|| refuse(E_DUPLICATE, "x"));
}
