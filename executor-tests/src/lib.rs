//! A one-node LEZ v0.3 chain in memory, for the committed multisig binary.
//!
//! Transactions are signed `PublicTransaction`s and go through
//! `ValidatedStateDiff::from_public_transaction_with_cycle_budget`, the call
//! the sequencer's settlement makes, then `V03State::apply_state_diff`. Nothing
//! is mocked between the message and the state: the plan and every apply run in
//! the RISC0 executor (BIP-340 verification included), the chained native and
//! token transfers run the real v0.3.0 programs, and the block timestamp is
//! checked against the windows the plan sets.

use lee::{
    public_transaction::{Message, WitnessSet},
    Account, AccountId, PrivateKey, ProgramShardSelector, PublicKey, PublicTransaction, ShardData, Signature,
    V03State, ValidatedStateDiff,
};
use accord_core::{Instruction, Multisig, Queued, Sig};
use token_core::{TokenDefinition, TokenHolding};

/// The most execution gas one transaction may declare; gas is cycles one for one.
pub const GAS_CAP: u64 = 10_000_000;

/// The header account the tests deploy the program at.
pub const MSIG: AccountId = AccountId::new(*b"accord_multisig/test/header/0001");
/// A second instance of the same binary, used as "another program" an
/// approved action calls with the vault as its authority.
pub const MSIG2: AccountId = AccountId::new(*b"accord_multisig/test/header/0002");

pub const BIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../artifacts/programs/v0.3/accord_multisig.bin");

/// A LEZ key: as an account (it signs transactions) or as an approval key (it
/// signs digests). The same type serves both, which is the point: a wallet key
/// works as an approval key.
pub struct Key {
    pub sk: PrivateKey,
    pub id: AccountId,
}

impl Key {
    #[must_use]
    pub fn new(seed: u8) -> Self {
        let mut b = [seed; 32];
        b[0] = 0x11;
        let sk = PrivateKey::try_new(b).expect("a valid secp256k1 scalar");
        let id = AccountId::from(&PublicKey::new_from_private_key(&sk));
        Self { sk, id }
    }

    /// The BIP-340 x-only key the configuration stores.
    #[must_use]
    pub fn approval_key(&self) -> [u8; 32] {
        *PublicKey::new_from_private_key(&self.sk).value()
    }

    /// Sign a 32-byte message exactly as the LEZ wallet signs a transaction.
    #[must_use]
    pub fn sign(&self, message: &[u8; 32]) -> Sig {
        Sig {
            key: self.approval_key(),
            sig: Signature::new(&self.sk, message).value,
        }
    }
}

pub struct Chain {
    pub state: V03State,
    pub block: u64,
    /// The block timestamp, milliseconds.
    pub now: u64,
    /// Cycles the last transaction used, landed or not.
    pub last_cycles: u64,
    /// Every account any transaction so far named, for the Pr.1 check.
    pub named: Vec<AccountId>,
    pub signers: Vec<AccountId>,
}

#[must_use]
pub fn holding_shard(definition_id: AccountId, balance: u128) -> ShardData {
    ShardData::from(&TokenHolding::Fungible { definition_id, balance })
}

impl Chain {
    /// `native`: accounts funded with native balance. `tokens`: (holder,
    /// definition, balance). Every definition named is created too.
    #[must_use]
    pub fn new(native: &[(AccountId, u128)], tokens: &[(AccountId, AccountId, u128)]) -> Self {
        let bin = std::fs::read(BIN).expect("build the guest first: artifacts/programs/v0.3/accord_multisig.bin");
        let program = lee::program::Program::new(bin.clone().into()).expect("a RISC0 program binary");
        let program2 = lee::program::Program::new(bin.into()).expect("a RISC0 program binary");
        let token = programs::token_account_id();
        let mut accounts: Vec<(AccountId, Account)> = native.iter().map(|(id, b)| (*id, Account::funded(*b))).collect();
        for (holder, def, bal) in tokens {
            let acc = accounts.iter_mut().find(|(id, _)| id == holder);
            let shard = holding_shard(*def, *bal);
            match acc {
                Some((_, a)) => a.data.set_shard(token, shard),
                None => accounts.push((*holder, Account::default().with_shard(token, shard))),
            }
            if !accounts.iter().any(|(id, _)| id == def) {
                let d = TokenDefinition::Fungible {
                    name: "TEST".into(),
                    total_supply: 1 << 100,
                    metadata_id: None,
                };
                accounts.push((*def, Account::default().with_shard(token, ShardData::from(&d))));
            }
        }
        let state = V03State::new()
            .with_public_accounts(accounts)
            .with_named_programs([(MSIG, program), (MSIG2, program2), (token, programs::token())]);
        Self {
            state,
            block: 10,
            now: 1_700_000_000_000,
            last_cycles: 0,
            named: vec![],
            signers: vec![],
        }
    }

    /// Send `ix` to `program` over `rows`, signed by `signers`, in a block at
    /// `self.now`. `Err` carries the refusal as the state machine reports it.
    pub fn send_to(&mut self, program: AccountId, ix: &Instruction, rows: Vec<ProgramShardSelector>, signers: &[&Key]) -> Result<(), String> {
        self.named.extend(rows.iter().map(|r| r.account_id));
        self.signers.extend(signers.iter().map(|k| k.id));
        let nonces = signers.iter().map(|k| self.state.get_account_by_id(k.id).nonce).collect();
        let message = Message::try_new(program, rows, nonces, ix).map_err(|e| e.to_string())?;
        let keys: Vec<&PrivateKey> = signers.iter().map(|k| &k.sk).collect();
        let tx = PublicTransaction::new(message.clone(), WitnessSet::for_message(&message, &keys));
        let r = ValidatedStateDiff::from_public_transaction_with_cycle_budget(&tx, &self.state, self.block, self.now, GAS_CAP);
        self.block += 1;
        match r {
            Ok((diff, charge)) => {
                self.last_cycles = charge.cycles;
                self.state.apply_state_diff(diff);
                Ok(())
            }
            Err(e) => Err(format!("{e}")),
        }
    }

    pub fn send(&mut self, ix: &Instruction, rows: Vec<ProgramShardSelector>, signers: &[&Key]) -> Result<(), String> {
        self.send_to(MSIG, ix, rows, signers)
    }

    #[must_use]
    pub fn native(&self, id: AccountId) -> u128 {
        self.state.get_account_by_id(id).data.native_balance().expect("canonical balance")
    }

    #[must_use]
    pub fn token(&self, id: AccountId) -> u128 {
        let acc = self.state.get_account_by_id(id);
        let shard = acc.data.shard(programs::token_account_id());
        if shard.is_empty() {
            return 0;
        }
        match TokenHolding::try_from(shard).expect("a token holding") {
            TokenHolding::Fungible { balance, .. } => balance,
            other => panic!("not fungible: {other:?}"),
        }
    }

    #[must_use]
    pub fn shard(&self, id: AccountId, program: AccountId) -> Vec<u8> {
        self.state.get_account_by_id(id).data.shard(program).to_vec()
    }

    #[must_use]
    pub fn multisig(&self, program: AccountId, multisig_id: &[u8; 32]) -> Multisig {
        accord_core::load(&self.shard(accord_core::config_account(&program, multisig_id), program))
    }

    #[must_use]
    pub fn queued(&self, multisig_id: &[u8; 32], digest: &[u8; 32]) -> Queued {
        accord_core::load_queued(&self.shard(accord_core::queued_account(&MSIG, multisig_id, digest), MSIG))
    }
}

/// The refusal code a failed send carries, e.g. `Some(7105)`.
#[must_use]
pub fn code(err: &str) -> Option<u32> {
    let at = err.find(": E")? + 3;
    err[at..at + 4].parse().ok()
}
