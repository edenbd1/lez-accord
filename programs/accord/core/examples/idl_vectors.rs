//! Borsh vectors for the IDL drift check (scripts/check-idl.py).
//!
//! One sample of every instruction variant, of every stored account and of
//! every event, with the values they were built from; plus the event
//! selectors and the refusal codes. The script decodes each vector with
//! nothing but the IDL and requires the same values back and every byte
//! consumed, so a field added, removed, retyped or reordered in the Rust types
//! without the IDL fails the check. Every scalar in a sample is distinct, so
//! two same-typed fields swapped fail too.

use accord_core::*;
use borsh::BorshSerialize;
use lee_core::account::AccountId;
use serde_json::{json, Value};

fn a(b: u8) -> AccountId {
    AccountId::new([b; 32])
}

fn hx(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn aid(x: &AccountId) -> Value {
    json!(hx(x.value()))
}

fn asset_json(x: &Asset) -> Value {
    match x {
        Asset::Native => json!({"Native": {}}),
        Asset::Token { definition_id } => json!({"Token": {"definition_id": aid(definition_id)}}),
    }
}

fn member_json(m: &Member) -> Value {
    json!({"key": hx(&m.key), "roles": m.roles, "weight": m.weight})
}

fn spec_json(l: &LimitSpec) -> Value {
    json!({
        "spenders": l.spenders.iter().map(|s| hx(s)).collect::<Vec<_>>(), "min_signers": l.min_signers,
        "asset": asset_json(&l.asset), "amount": l.amount.to_string(), "period_ms": l.period_ms,
    })
}

fn config_json(c: &Config) -> Value {
    json!({
        "members": c.members.iter().map(member_json).collect::<Vec<_>>(), "threshold": c.threshold,
        "time_lock_ms": c.time_lock_ms, "limits": c.limits.iter().map(spec_json).collect::<Vec<_>>(),
    })
}

fn sig_json(s: &Sig) -> Value {
    json!({"key": hx(&s.key), "sig": hx(&s.sig)})
}

fn action_json(x: &Action) -> Value {
    match x {
        Action::Calls(calls) => json!({"Calls": {"calls": calls.iter().map(|c| json!({
            "program": aid(&c.program),
            "accounts": c.accounts.iter().map(|a| json!({"account": aid(&a.account), "shard_program": aid(&a.shard_program)})).collect::<Vec<_>>(),
            "data": hx(&c.data),
        })).collect::<Vec<_>>()}}),
        Action::Config(c) => json!({"Config": {"config": config_json(c)}}),
        Action::CancelQueued { digest } => json!({"CancelQueued": {"digest": hx(digest)}}),
    }
}

fn v<T: BorshSerialize>(name: &str, x: &T, values: Value) -> Value {
    json!({"name": name, "hex": hx(&borsh::to_vec(x).unwrap()), "values": values})
}

fn main() {
    let tok = Asset::Token { definition_id: a(0x14) };
    let spec = LimitSpec { spenders: vec![[0x21; 32], [0x22; 32]], min_signers: 2, asset: tok, amount: (1u128 << 90) + 3, period_ms: 86_400_007 };
    let config = Config {
        members: vec![Member { key: [0x31; 32], roles: 3, weight: 5 }, Member { key: [0x32; 32], roles: 6, weight: 7 }],
        threshold: 11,
        time_lock_ms: 600_013,
        limits: vec![spec.clone()],
    };
    let s1 = Sig { key: [0x41; 32], sig: [0x42; 64] };
    let s2 = Sig { key: [0x43; 32], sig: [0x44; 64] };
    let calls = Action::Calls(vec![Call {
        program: a(0x51),
        accounts: vec![CallAccount { account: a(0x52), shard_program: a(0x53) }],
        data: vec![1, 2, 3, 4, 5],
    }]);
    let id = [0x61; 32];
    let ix = |x: &Instruction| x.clone();
    let samples = vec![
        (ix(&Instruction::Create { multisig_id: id, config: config.clone() }), json!({"multisig_id": hx(&id), "config": config_json(&config)})),
        (ix(&Instruction::Activate { multisig_id: id, epoch: 17, approvals: vec![s1.clone(), s2.clone()] }), json!({"multisig_id": hx(&id), "epoch": 17, "approvals": [sig_json(&s1), sig_json(&s2)]})),
        (
            ix(&Instruction::Queue { multisig_id: id, epoch: 19, expires_at: 1_700_000_000_023, action: calls.clone(), time_lock_ms: 29, proposer: s1.clone(), approvals: vec![s2.clone()], at: 1_700_000_000_031 }),
            json!({"multisig_id": hx(&id), "epoch": 19, "expires_at": 1_700_000_000_023u64, "action": action_json(&calls), "time_lock_ms": 29, "proposer": sig_json(&s1), "approvals": [sig_json(&s2)], "at": 1_700_000_000_031u64}),
        ),
        (
            ix(&Instruction::Execute { multisig_id: id, epoch: 37, expires_at: 1_700_000_000_041, action: Action::Config(config.clone()), executor: s2.clone(), at: 1_700_000_000_043 }),
            json!({"multisig_id": hx(&id), "epoch": 37, "expires_at": 1_700_000_000_041u64, "action": action_json(&Action::Config(config.clone())), "executor": sig_json(&s2), "at": 1_700_000_000_043u64}),
        ),
        (
            ix(&Instruction::Spend { multisig_id: id, limit: 47, uses: 53, asset: Asset::Native, amount: 59, approvals: vec![s1.clone()], at: 1_700_000_000_061 }),
            json!({"multisig_id": hx(&id), "limit": 47, "uses": 53, "asset": asset_json(&Asset::Native), "amount": "59", "approvals": [sig_json(&s1)], "at": 1_700_000_000_061u64}),
        ),
        (
            ix(&Instruction::Register { program: a(0x71), image_id: [1, 2, 3, 4, 5, 6, 7, 8], idl_sha256: [0x72; 32], name: "token".into() }),
            json!({"program": aid(&a(0x71)), "image_id": [1, 2, 3, 4, 5, 6, 7, 8], "idl_sha256": hx(&[0x72; 32]), "name": "token"}),
        ),
    ];
    let names = ["create", "activate", "queue", "execute", "spend", "register"];
    let instructions: Vec<Value> = samples.iter().zip(names).map(|((x, val), n)| v(n, x, val.clone())).collect();

    let ms = Multisig {
        version: 1,
        initialised: 1,
        epoch: 67,
        members: config.members.clone(),
        threshold: 71,
        time_lock_ms: 73,
        limits: vec![Limit { spec: spec.clone(), period_start: 79, spent: 83, uses: 89 }],
        vault: a(0x81),
        creator: a(0x82),
        queued: 97,
        executed: 101,
    };
    let ms_json = json!({
        "version": 1, "initialised": 1, "epoch": 67, "members": config.members.iter().map(member_json).collect::<Vec<_>>(),
        "threshold": 71, "time_lock_ms": 73,
        "limits": [{"spec": spec_json(&spec), "period_start": 79, "spent": "83", "uses": 89}],
        "vault": aid(&a(0x81)), "creator": aid(&a(0x82)), "queued": 97, "executed": 101,
    });
    let q = Queued {
        digest: [0x91; 32],
        kind: 2,
        epoch: 103,
        quorum_at: 107,
        expires_at: 109,
        time_lock_ms: 113,
        proposer: [0x92; 32],
        approvers: vec![[0x93; 32]],
        status: 1,
        executed_at: 127,
    };
    let q_json = json!({
        "digest": hx(&[0x91; 32]), "kind": 2, "epoch": 103, "quorum_at": 107, "expires_at": 109, "time_lock_ms": 113,
        "proposer": hx(&[0x92; 32]), "approvers": [hx(&[0x93; 32])], "status": 1, "executed_at": 127,
    });
    let r = RegistryEntry { program: a(0xa1), image_id: [9, 10, 11, 12, 13, 14, 15, 16], idl_sha256: [0xa2; 32], name: "accord".into(), registrant: a(0xa3) };
    let r_json = json!({"program": aid(&a(0xa1)), "image_id": [9, 10, 11, 12, 13, 14, 15, 16], "idl_sha256": hx(&[0xa2; 32]), "name": "accord", "registrant": aid(&a(0xa3))});

    let events = vec![
        v("Created", &Created { multisig_id: id, vault: a(0xb1) }, json!({"multisig_id": hx(&id), "vault": aid(&a(0xb1))})),
        v("Activated", &Activated { multisig_id: id, epoch: 131 }, json!({"multisig_id": hx(&id), "epoch": 131})),
        v("ActionQueued", &ActionQueued { multisig_id: id, digest: [0xb2; 32], kind: 1, at: 137 }, json!({"multisig_id": hx(&id), "digest": hx(&[0xb2; 32]), "kind": 1, "at": 137})),
        v("ActionExecuted", &ActionExecuted { multisig_id: id, digest: [0xb3; 32], kind: 2, at: 139 }, json!({"multisig_id": hx(&id), "digest": hx(&[0xb3; 32]), "kind": 2, "at": 139})),
        v("Spent", &Spent { multisig_id: id, limit: 149, amount: 151, destination: a(0xb4) }, json!({"multisig_id": hx(&id), "limit": 149, "amount": "151", "destination": aid(&a(0xb4))})),
        v("Registered", &Registered { program: a(0xb5), idl_sha256: [0xb6; 32] }, json!({"program": aid(&a(0xb5)), "idl_sha256": hx(&[0xb6; 32])})),
    ];
    let out = json!({
        "instructions": instructions,
        "accounts": [v("Multisig", &ms, ms_json), v("Queued", &q, q_json), v("RegistryEntry", &r, r_json)],
        "events": events,
        "selectors": EVENT_NAMES.iter().map(|n| json!({"name": n, "selector": hx(&event_selector(n))})).collect::<Vec<_>>(),
        "errors": errors::ALL.iter().map(|(c, n)| json!({"code": c, "name": n, "advice": errors::advice(*c)})).collect::<Vec<_>>(),
        "window_ms": WINDOW_MS,
        "roles": {"propose": ROLE_PROPOSE, "approve": ROLE_APPROVE, "execute": ROLE_EXECUTE},
    });
    println!("{out}");
}
