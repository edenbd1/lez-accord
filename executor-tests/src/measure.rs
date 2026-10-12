//! Whole-transaction cycle cost of each accord_multisig instruction on LEZ
//! v0.3, and approval verification as a function of M (P.2): the largest M
//! whose `queue` fits the 10,000,000 execution-gas cap.
//!
//! Cycles are what the state machine meters: the plan, every apply (BIP-340
//! verification runs inside `apply`), and every chained call, summed. Gas is
//! cycles one for one.
//!
//! `cargo run --release --bin measure` (ACCORD_BIN=<other .bin> to compare builds).

use accord_core::{
    action_digest, activate_message, approve_message, config_account, execute_message, propose_message, rows,
    spend_message, vault_account, vault_transfer, Action, Asset, Config, Instruction, LimitSpec, Member, MAX_MEMBERS,
    ROLE_APPROVE, ROLE_EXECUTE, ROLE_PROPOSE,
};
use accord_executor_tests::{Chain, Key, GAS_CAP, MSIG};
use lee::AccountId;

const ID: [u8; 32] = [0x31; 32];
const DEST: AccountId = AccountId::new([0xD1; 32]);

struct Run {
    c: Chain,
    k: Vec<Key>,
    payer: Key,
}

/// An `n`-member multisig with threshold `m`, activated, its vault funded.
fn setup(n: usize, m: u64) -> Result<(Run, u64), String> {
    let payer = Key::new(1);
    let k: Vec<Key> = (0..n).map(|i| Key::new(40 + i as u8)).collect();
    let vault = vault_account(&MSIG, &ID);
    let mut c = Chain::new(&[(payer.id, 1 << 60), (vault, 1 << 40)], &[]);
    let config = Config {
        members: k.iter().map(|x| Member { key: x.approval_key(), roles: ROLE_PROPOSE | ROLE_APPROVE | ROLE_EXECUTE, weight: 1 }).collect(),
        threshold: m,
        time_lock_ms: 0,
        limits: vec![LimitSpec { spenders: vec![k[0].approval_key()], min_signers: 1, asset: Asset::Native, amount: 1 << 30, period_ms: 1 }],
    };
    c.send(&Instruction::Create { multisig_id: ID, config }, rows::create(&MSIG, &ID, payer.id), &[&payer])?;
    let create = c.last_cycles;
    let msg = activate_message(&config_account(&MSIG, &ID), 0);
    let approvals = k.iter().take(m as usize).map(|x| x.sign(&msg)).collect();
    c.send(&Instruction::Activate { multisig_id: ID, epoch: 0, approvals }, rows::activate(&MSIG, &ID), &[&payer])?;
    Ok((Run { c, k, payer }, create))
}

/// Cycles of `queue` with `m` approvals (plus the proposer's signature), then
/// of `execute` of that native transfer.
fn queue_m(m: usize) -> Result<(u64, u64), String> {
    let n = m.max(1);
    let (mut r, _) = setup(n, m as u64).or_else(|_| setup(n, 1))?;
    let a = Action::Calls(vec![vault_transfer(&MSIG, &ID, &Asset::Native, DEST, 1)]);
    let exp = r.c.now + 3_600_000;
    let d = action_digest(&config_account(&MSIG, &ID), 0, exp, &a);
    let ix = Instruction::Queue {
        multisig_id: ID,
        epoch: 0,
        expires_at: exp,
        action: a.clone(),
        time_lock_ms: 0,
        proposer: r.k[0].sign(&propose_message(&d)),
        approvals: r.k.iter().take(m).map(|x| x.sign(&approve_message(&d))).collect(),
        at: r.c.now,
    };
    r.c.send(&ix, rows::queue(&MSIG, &ID, &d), &[&r.payer])?;
    let q = r.c.last_cycles;
    let ix = Instruction::Execute { multisig_id: ID, epoch: 0, expires_at: exp, action: a.clone(), executor: r.k[0].sign(&execute_message(&d)), at: r.c.now };
    r.c.send(&ix, rows::execute(&MSIG, &ID, &d, &a), &[&r.payer])?;
    Ok((q, r.c.last_cycles))
}

fn pct(c: u64) -> String {
    format!("{:.1}%", c as f64 * 100.0 / GAS_CAP as f64)
}

fn main() {
    println!("| op | cycles | share of the 10M gas cap |");
    println!("|---|---:|---:|");
    match setup(3, 2) {
        Ok((mut r, create)) => {
            println!("| create (3 members) | {create} | {} |", pct(create));
            let (q, e) = queue_m(2).unwrap();
            println!("| queue, M = 2 (3 signatures) | {q} | {} |", pct(q));
            println!("| execute, native transfer (1 signature) | {e} | {} |", pct(e));
            let msg = spend_message(&config_account(&MSIG, &ID), 0, 0, &DEST, &Asset::Native, 5);
            let ix = Instruction::Spend { multisig_id: ID, limit: 0, uses: 0, asset: Asset::Native, amount: 5, approvals: vec![r.k[0].sign(&msg)], at: r.c.now };
            r.c.send(&ix, rows::spend(&MSIG, &ID, &Asset::Native, DEST), &[&r.payer]).unwrap();
            println!("| spend within a limit (1 signature) | {} | {} |", r.c.last_cycles, pct(r.c.last_cycles));
        }
        Err(e) => println!("| create + activate (3 members, M = 2) | over the cap: {e} | |"),
    }
    println!();
    println!("| M (approvals) | queue cycles | share of the cap | marginal per approval |");
    println!("|---:|---:|---:|---:|");
    let mut prev: Option<u64> = None;
    let mut largest = 0;
    for m in 1..=MAX_MEMBERS {
        match queue_m(m) {
            Ok((q, _)) => {
                let marg = prev.map(|p| (q - p).to_string()).unwrap_or_else(|| "-".into());
                println!("| {m} | {q} | {} | {marg} |", pct(q));
                prev = Some(q);
                largest = m;
            }
            Err(e) => {
                println!("| {m} | over the cap | | {} |", e.lines().next().unwrap_or(""));
                break;
            }
        }
    }
    println!();
    println!("largest M whose queue fits one transaction: {largest} (of at most {MAX_MEMBERS} members)");
}
