//! Real proving of approval verification (P.1, P.2): the guest's `apply` of
//! `VerifyQueue`, the step that checks M BIP-340 approvals plus the
//! proposer's signature, proved with the RISC Zero prover on this machine.
//!
//! Development mode is refused: `RISC0_DEV_MODE` must be unset or 0. Under the
//! private posture this is the receipt a member's client produces and the
//! privacy-preserving circuit composes, so its time and size are the figures
//! that matter for capacity planning.
//!
//! `cargo run --release --bin prove -- 1 3 5 7 10`

use accord_core::{
    action_digest, approve_message, config_account, propose_message, vault_account, vault_transfer, Action, Asset, Effect,
    Member, Multisig, ROLE_APPROVE, ROLE_EXECUTE, ROLE_PROPOSE,
};
use accord_executor_tests::{Key, BIN, MSIG};
use lee_core::program::{ApplyInput, CallKind};
use lee_core::account::{ProgramShardSelector, ShardData};
use risc0_zkvm::{default_prover, ExecutorEnv, ProverOpts};
use std::time::Instant;

const ID: [u8; 32] = [0x31; 32];

fn frame(bytes: &[u8]) -> Vec<u8> {
    let mut out = (bytes.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(bytes);
    out
}

fn main() {
    assert!(
        std::env::var("RISC0_DEV_MODE").map(|v| v == "0" || v.is_empty()).unwrap_or(true),
        "refusing to benchmark in development mode"
    );
    let ms: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().expect("an M")).collect();
    let ms = if ms.is_empty() { vec![1, 3, 5, 7, 10] } else { ms };
    let bin = std::fs::read(std::env::var("ACCORD_BIN").unwrap_or_else(|_| BIN.to_string())).expect("the guest binary");
    let cfg = config_account(&MSIG, &ID);
    println!("| M | signatures verified | user cycles | segments | prove (s, wall) | succinct receipt (bytes) |");
    println!("|---:|---:|---:|---:|---:|---:|");
    for m in ms {
        let keys: Vec<Key> = (0..m).map(|i| Key::new(40 + i as u8)).collect();
        let state = Multisig {
            version: 1,
            initialised: 1,
            epoch: 0,
            members: keys.iter().map(|k| Member { key: k.approval_key(), roles: ROLE_PROPOSE | ROLE_APPROVE | ROLE_EXECUTE, weight: 1 }).collect(),
            threshold: m as u64,
            time_lock_ms: 0,
            limits: vec![],
            vault: vault_account(&MSIG, &ID),
            creator: keys[0].id,
            queued: 0,
            executed: 0,
        };
        let action = Action::Calls(vec![vault_transfer(&MSIG, &ID, &Asset::Native, keys[0].id, 1)]);
        let exp = 2_000_000_000_000;
        let digest = action_digest(&cfg, 0, exp, &action);
        let effect = Effect::VerifyQueue {
            epoch: 0,
            digest,
            time_lock_ms: 0,
            proposer: keys[0].sign(&propose_message(&digest)),
            approvals: keys.iter().map(|k| k.sign(&approve_message(&digest))).collect(),
            at: 1_700_000_000_000,
            expires_at: exp,
        };
        let input = ApplyInput {
            self_account_id: MSIG,
            selector: ProgramShardSelector::new(cfg, MSIG),
            pre_data: ShardData::try_from(borsh::to_vec(&state).unwrap()).unwrap(),
            effect_data: borsh::to_vec(&effect).unwrap(),
        };
        let mut stdin = frame(&borsh::to_vec(&CallKind::Apply).unwrap());
        stdin.extend(frame(&borsh::to_vec(&input).unwrap()));
        let env = ExecutorEnv::builder().write_slice(&stdin).build().unwrap();
        let t = Instant::now();
        let info = default_prover().prove_with_opts(env, &bin, &ProverOpts::succinct()).expect("proving");
        let secs = t.elapsed().as_secs_f64();
        let size = bincode_len(&info.receipt);
        println!(
            "| {m} | {} | {} | {} | {secs:.1} | {size} |",
            m + 1,
            info.stats.user_cycles,
            info.stats.segments,
        );
    }
}

fn bincode_len(r: &risc0_zkvm::Receipt) -> usize {
    borsh::to_vec(r).map(|v| v.len()).unwrap_or(0)
}
