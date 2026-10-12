//! `accord`: create, propose, approve, queue, execute, spend through and
//! inspect accord_multisig multisigs on LEZ v0.3.
//!
//! Approvals are collected off-chain as **approval packages**: a JSON file
//! carrying the exact action bytes, the digest they hash to, and the
//! signatures collected so far. `propose` starts one, `approve` decodes the
//! action from the package's bytes, shows it, and adds a signature, `queue`
//! and `execute` submit it. The package travels however the members choose:
//! the coordination room (`room` commands, over Logos Delivery), a file, a
//! message. The program checks every signature, so the transport carries no
//! trust.
//!
//! Approval keys live in `--keys` (`ACCORD_KEYS`, default `~/.accord/keys`),
//! one hex secret per file. They never sign a LEZ transaction; the wallet's
//! fee payer does. Read-only commands (`show`, `queued`, `ids`, `decode`,
//! `image-id`, `now`, `key`) need no wallet.
//!
//! Every transaction prints one JSON line: the operation, its hash, its block
//! and a verdict read back from the chain (`applied`, `reverted`, `rejected`,
//! `not-included`), with the refusal code's advice when the program refused.

mod room;

use std::{path::PathBuf, str::FromStr, time::Duration};

use accord_core::{
    self as core, action_digest, activate_message, approve_message, config_account, errors, execute_message,
    propose_message, queued_account, registry_account, rows, spend_message, vault_account, vault_transfer, Action,
    Asset, Call, CallAccount, Config, Instruction, LimitSpec, Member, Multisig, Queued, Sig, ROLE_APPROVE, ROLE_EXECUTE,
    ROLE_PROPOSE,
};
use anyhow::{anyhow, bail, ensure, Context as _, Result};
use clap::{Parser, Subcommand};
use lee::{program::Program, AccountId, PrivateKey, ProgramShardSelector, PublicKey, Signature};
use lee_core::native_token::NATIVE_TOKEN_PROGRAM_ID;
use sequencer_service_rpc::{RpcClient as _, SequencerClient, SequencerClientBuilder};
use serde::{Deserialize, Serialize};
use wallet::{program_facades::program_loader::ProgramLoader, AccountIdentity, AccountMention, WalletCore};

const LOCAL_RPC: &str = "http://127.0.0.1:3040";

#[derive(Parser)]
#[command(name = "accord", version, about = "accord_multisig on LEZ v0.3")]
struct Cli {
    /// The program's header account (base58).
    #[arg(long, env = "ACCORD_PROGRAM", global = true)]
    program: Option<AccountId>,
    /// The sequencer the read-only commands ask. Commands that sign use the
    /// wallet's own sequencer.
    #[arg(long, env = "ACCORD_RPC", global = true, default_value = LOCAL_RPC)]
    rpc: String,
    /// The guest binary (deploy, image-id).
    #[arg(long, env = "ACCORD_ELF", global = true, default_value = "artifacts/programs/v0.3/accord_multisig.bin")]
    elf: PathBuf,
    /// Where approval keys are kept.
    #[arg(long, env = "ACCORD_KEYS", global = true)]
    keys: Option<PathBuf>,
    /// The account that pays a public transaction's fee.
    #[arg(long, env = "ACCORD_PAYER", global = true)]
    payer: Option<AccountId>,
    /// Send even if the local pre-check says the program will refuse.
    #[arg(long, global = true)]
    force: bool,
    /// A label for the JSON line.
    #[arg(long, global = true, default_value = "")]
    label: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Approval keys: `key new <name>` or `key show <name>`.
    Key {
        #[arg(value_parser = ["new", "show"])]
        op: String,
        name: String,
    },
    /// The ImageID of `--elf`.
    ImageId,
    /// The chain clock (ms).
    Now,
    /// The configuration and vault accounts of a multisig id (and a queued record's, with `--digest`).
    Ids {
        multisig: String,
        #[arg(long)]
        digest: Option<String>,
    },
    /// Deploy `--elf` through program_loader.
    Deploy {
        #[arg(long)]
        payer: AccountId,
        /// Deploy into this header account, already in the wallet (same id after a reset). Needs `--segment`.
        #[arg(long, requires = "segment")]
        header: Option<AccountId>,
        #[arg(long, requires = "header")]
        segment: Vec<AccountId>,
        /// Make the header immutable: needed for a private vault, whose binding
        /// to this program the privacy circuit checks against the header's mirror.
        #[arg(long)]
        immutable: bool,
    },
    /// Create a multisig. Members are `<key name or hex>:<roles>[:weight]`,
    /// roles a subset of `p` (propose), `a` (approve), `e` (execute).
    Create {
        multisig: String,
        #[arg(long = "member", required = true)]
        members: Vec<String>,
        #[arg(long)]
        threshold: u64,
        /// `0`, `90s`, `10m`, `1h`, `2d`.
        #[arg(long, default_value = "0")]
        time_lock: String,
        /// `<spender>[+<spender>...]:<min signers>:<amount>:<period>[:<token definition>]`.
        #[arg(long = "limit")]
        limits: Vec<String>,
    },
    /// Read a multisig, its vault balances and its spending limits.
    Show { multisig: String },
    /// Read a queued action.
    Queued { multisig: String, digest: String },
    /// Start an approval package for an action, signed by a proposing key.
    Propose {
        multisig: String,
        /// `<to>:<amount>[:<token definition>]`, a transfer out of the vault. Repeatable.
        #[arg(long)]
        transfer: Vec<String>,
        /// Transfer out of this account instead of the public vault: the private
        /// vault's id (`accord vault id`).
        #[arg(long)]
        from: Option<AccountId>,
        /// A JSON file of calls: `[{"program": .., "accounts": [[account, shard program], ..], "data": hex}]`.
        #[arg(long)]
        calls: Option<PathBuf>,
        /// A JSON configuration replacing the current one.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Veto a queued action by digest.
        #[arg(long)]
        cancel: Option<String>,
        /// `+1h`, `+30m`, or milliseconds.
        #[arg(long, default_value = "+1h")]
        expires: String,
        #[arg(long)]
        key: String,
        /// Free text shown to approvers, marked as the proposer's.
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Decode a package's action from its exact bytes and check its digest (U.6).
    Decode { package: PathBuf },
    /// Decode, show, and add an approval signature to a package.
    Approve {
        package: PathBuf,
        #[arg(long)]
        key: String,
    },
    /// Submit a package's approvals: the action becomes queued.
    Queue { package: PathBuf },
    /// Execute a queued package, authorised by an executing key.
    Execute {
        package: PathBuf,
        #[arg(long)]
        key: String,
        /// The action moves the private vault of this wallet group: send a
        /// privacy-preserving transaction, proved locally.
        #[arg(long)]
        private_vault: Option<String>,
    },
    /// Activate a multisig with approvals of its configuration (all keys local).
    Activate {
        multisig: String,
        #[arg(long = "key", required = true)]
        signers: Vec<String>,
    },
    /// Spend from the vault within a spending limit.
    Spend {
        multisig: String,
        #[arg(long, default_value_t = 0)]
        limit: u32,
        #[arg(long)]
        to: AccountId,
        #[arg(long)]
        amount: u128,
        #[arg(long)]
        token: Option<AccountId>,
        #[arg(long = "key", required = true)]
        signers: Vec<String>,
        /// Spend from the private vault whose keys the wallet group `<name>` holds,
        /// in a privacy-preserving transaction proved locally.
        #[arg(long)]
        private_vault: Option<String>,
    },
    /// The private vault: a private PDA of this program under the vault seed,
    /// keyed by a wallet group (Group Master Secret) the members share.
    Vault {
        #[command(subcommand)]
        op: VaultCmd,
    },
    /// The coordination room over Logos Delivery (docs/ROOM.md).
    Room {
        #[command(subcommand)]
        op: RoomCmd,
    },
    /// Register a program's IDL hash and ImageID in the on-chain registry.
    Register {
        target: AccountId,
        #[arg(long)]
        idl: PathBuf,
        #[arg(long)]
        name: String,
        /// The ImageID to record; read from the target's header if omitted.
        #[arg(long)]
        image_id: Option<String>,
    },
}

#[derive(Subcommand)]
enum VaultCmd {
    /// Register the private vault in the wallet, so a sync follows its state
    /// (each member's wallet does this once, after joining the group).
    Track {
        multisig: String,
        #[arg(long)]
        group: String,
    },
    /// The private vault's account id for a group.
    Id {
        multisig: String,
        #[arg(long)]
        group: String,
    },
    /// Fund the private vault from a public account (a privacy-preserving transfer).
    Fund {
        multisig: String,
        #[arg(long)]
        group: String,
        #[arg(long)]
        from: AccountId,
        #[arg(long)]
        amount: u128,
    },
    /// Try to move the private vault's funds directly with the native token
    /// program, holding the group keys but without the multisig. Must fail.
    Steal {
        multisig: String,
        #[arg(long)]
        group: String,
        #[arg(long)]
        to: AccountId,
        #[arg(long)]
        amount: u128,
    },
}

#[derive(Subcommand)]
enum RoomCmd {
    /// Open the room of the multisig's current epoch: a fresh secret, sealed
    /// to every current member, invites posted on the invite topic and written to `--invites`.
    Open {
        multisig: String,
        #[arg(long)]
        key: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        invites: PathBuf,
        /// Also publish the invites over Delivery.
        #[arg(long)]
        publish: bool,
    },
    /// Join with your invite (from a file, or heard on the invite topic within `--seconds`).
    Join {
        multisig: String,
        #[arg(long)]
        key: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        invites: Option<PathBuf>,
        #[arg(long, default_value_t = 120)]
        seconds: u64,
    },
    /// Post an approval package, or a line of deliberation, to the room.
    Post {
        room: PathBuf,
        #[arg(long)]
        key: String,
        #[arg(long)]
        package: Option<PathBuf>,
        #[arg(long)]
        say: Option<String>,
        /// Seconds to stay up after sending, so relays carry it.
        #[arg(long, default_value_t = 15)]
        linger: u64,
    },
    /// Listen for `--seconds`, verify every frame, and merge the approvals of
    /// every package heard into `--into/<digest>.json`.
    Listen {
        room: PathBuf,
        #[arg(long)]
        into: PathBuf,
        #[arg(long, default_value_t = 120)]
        seconds: u64,
        /// Stop once a package in `--into` holds this many approvals.
        #[arg(long)]
        until_approvals: Option<usize>,
    },
}

/// An approval package: everything an approver needs to check what they
/// sign, and everything the program needs to verify it.
#[derive(Serialize, Deserialize, Clone)]
struct Package {
    program: String,
    multisig_id: String,
    config_account: String,
    epoch: u32,
    expires_at: u64,
    /// borsh(Action), hex: the bytes every signature commits to.
    action: String,
    digest: String,
    /// The proposer's text, not part of the digest.
    description: String,
    proposer: Option<SigJson>,
    approvals: Vec<SigJson>,
}

#[derive(Serialize, Deserialize, Clone)]
struct SigJson {
    key: String,
    sig: String,
}

impl From<&Sig> for SigJson {
    fn from(s: &Sig) -> Self {
        Self { key: hex::encode(s.key), sig: hex::encode(s.sig) }
    }
}

impl TryFrom<&SigJson> for Sig {
    type Error = anyhow::Error;
    fn try_from(s: &SigJson) -> Result<Self> {
        Ok(Sig {
            key: hex::decode(&s.key)?.try_into().map_err(|_| anyhow!("a key is 32 bytes"))?,
            sig: hex::decode(&s.sig)?.try_into().map_err(|_| anyhow!("a signature is 64 bytes"))?,
        })
    }
}

impl Package {
    fn read(p: &PathBuf) -> Result<Self> {
        serde_json::from_slice(&std::fs::read(p).with_context(|| format!("reading {}", p.display()))?)
            .with_context(|| format!("{} is not an approval package", p.display()))
    }
    fn write(&self, p: &PathBuf) -> Result<()> {
        std::fs::write(p, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
    fn action(&self) -> Result<Action> {
        Ok(borsh::from_slice(&hex::decode(&self.action)?)?)
    }
    fn multisig_id(&self) -> Result<[u8; 32]> {
        hex::decode(&self.multisig_id)?.try_into().map_err(|_| anyhow!("a multisig id is 32 bytes"))
    }
    fn program(&self) -> Result<AccountId> {
        AccountId::from_str(&self.program).map_err(|e| anyhow!("{e:?}"))
    }
    /// Recompute the digest from the bytes and refuse a package whose digest
    /// or configuration account does not match them.
    fn checked_digest(&self) -> Result<[u8; 32]> {
        let program = self.program()?;
        let id = self.multisig_id()?;
        let cfg = config_account(&program, &id);
        ensure!(cfg.to_string() == self.config_account, "the package's configuration account is not the multisig's");
        let d = action_digest(&cfg, self.epoch, self.expires_at, &self.action()?);
        ensure!(hex::encode(d) == self.digest, "the package's digest does not match its action bytes: do not sign it");
        Ok(d)
    }
}

/// A 32-byte id: 64 hex digits, or text of at most 32 bytes, zero-padded.
fn id32(s: &str) -> Result<[u8; 32]> {
    if s.len() == 64 {
        if let Ok(v) = hex::decode(s) {
            return Ok(v.try_into().expect("32 bytes"));
        }
    }
    ensure!(s.len() <= 32, "an id is 64 hex digits or at most 32 bytes of text: {s}");
    Ok(core::seed_from_str(s))
}

fn duration(s: &str) -> Result<u64> {
    if let Ok(n) = s.parse::<u64>() {
        return Ok(n);
    }
    let (digits, unit) = s.split_at(s.len() - 1);
    let n: u64 = digits.parse().with_context(|| format!("not a duration: {s}"))?;
    Ok(n * match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => bail!("unit is s, m, h or d: {s}"),
    })
}

struct Ctx {
    wallet: Option<WalletCore>,
    rpc: SequencerClient,
    program: Option<AccountId>,
    keys: PathBuf,
    payer: Option<AccountId>,
    force: bool,
    label: String,
}

impl Ctx {
    fn program(&self) -> Result<AccountId> {
        self.program.ok_or_else(|| anyhow!("name the program's header with --program or ACCORD_PROGRAM"))
    }
    fn wallet(&self) -> &WalletCore {
        self.wallet.as_ref().expect("a signing command opens the wallet")
    }
    async fn account(&self, id: AccountId) -> Result<lee::Account> {
        match &self.wallet {
            Some(w) => w.get_account_public(id).await,
            None => self.rpc.get_account(id).await.map_err(|e| anyhow!("reading {id} from the sequencer: {e}")),
        }
    }
    async fn shard(&self, id: AccountId, program: AccountId) -> Result<Vec<u8>> {
        Ok(self.account(id).await?.data.shard(program).to_vec())
    }
    async fn now(&self) -> Result<u64> {
        let clock = clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
        let acc = self.account(clock).await?;
        Ok(clock_core::ClockAccountData::from_bytes(acc.data.shard(clock_core::clock_account_id())).timestamp)
    }
    async fn multisig(&self, id: &[u8; 32]) -> Result<Multisig> {
        let p = self.program()?;
        let bytes = self.shard(config_account(&p, id), p).await?;
        ensure!(!bytes.is_empty(), "no multisig at id {}", hex::encode(id));
        Ok(borsh::from_slice(&bytes)?)
    }

    // -------------------------------------------------------------- keys

    fn key_path(&self, name: &str) -> PathBuf {
        self.keys.join(format!("{name}.key"))
    }
    fn secret(&self, name: &str) -> Result<PrivateKey> {
        let p = self.key_path(name);
        let hexs = std::fs::read_to_string(&p).with_context(|| format!("no approval key {name} at {}", p.display()))?;
        let b: [u8; 32] = hex::decode(hexs.trim())?.try_into().map_err(|_| anyhow!("a key file holds 32 bytes"))?;
        PrivateKey::try_new(b).map_err(|e| anyhow!("{e:?}"))
    }
    fn sign(&self, name: &str, msg: &[u8; 32]) -> Result<Sig> {
        let sk = self.secret(name)?;
        Ok(Sig { key: *PublicKey::new_from_private_key(&sk).value(), sig: Signature::new(&sk, msg).value })
    }
    /// A member key: a local key name, or 64 hex digits.
    fn public(&self, s: &str) -> Result<[u8; 32]> {
        if s.len() == 64 {
            if let Ok(v) = hex::decode(s) {
                return Ok(v.try_into().expect("32 bytes"));
            }
        }
        Ok(*PublicKey::new_from_private_key(&self.secret(s)?).value())
    }

    // ------------------------------------------------------------ sending

    fn mentions(&self, rows: Vec<ProgramShardSelector>, signers: &[AccountId]) -> Vec<AccountMention> {
        rows.into_iter()
            .map(|r| {
                let who = if signers.contains(&r.account_id) {
                    AccountIdentity::Public(r.account_id)
                } else {
                    AccountIdentity::PublicNoSign(r.account_id)
                };
                who.select_program_shard(r.program_account_id)
            })
            .collect()
    }

    /// Send `ix`, then report whether the shards in `watch` changed.
    async fn send(&self, op: &str, ix: &Instruction, rows: Vec<ProgramShardSelector>, signers: &[AccountId], watch: &[(AccountId, AccountId)]) -> Result<serde_json::Value> {
        let program = self.program()?;
        let mut before = vec![];
        for (a, p) in watch {
            before.push(self.shard(*a, *p).await?);
        }
        let data = Program::serialize_instruction(ix.clone())?;
        let sent = self.wallet().send_pub_tx_paid_by(self.mentions(rows, signers), data, program, self.payer).await;
        let hash = match sent {
            Ok(h) => h,
            Err(e) => {
                let line = serde_json::json!({"op": op, "label": self.label, "verdict": "rejected", "error": format!("{e:?}")});
                println!("{line}");
                return Ok(line);
            }
        };
        let polled = tokio::time::timeout(Duration::from_secs(180), self.wallet().poll_transaction(hash)).await;
        let block = match polled {
            Ok(Ok((_, b))) => Some(b),
            _ => None,
        };
        let mut after = vec![];
        for (a, p) in watch {
            after.push(self.shard(*a, *p).await?);
        }
        let verdict = match block {
            None => "not-included",
            Some(_) if after != before => "applied",
            Some(_) => "reverted",
        };
        let line = serde_json::json!({"op": op, "label": self.label, "tx": hash.to_string(), "block": block, "verdict": verdict});
        println!("{line}");
        Ok(line)
    }
}

fn roles_of(s: &str) -> Result<u8> {
    let mut r = 0;
    for c in s.chars() {
        r |= match c {
            'p' => ROLE_PROPOSE,
            'a' => ROLE_APPROVE,
            'e' => ROLE_EXECUTE,
            _ => bail!("roles are letters among p, a, e: {s}"),
        };
    }
    Ok(r)
}

fn role_names(r: u8) -> Vec<&'static str> {
    [(ROLE_PROPOSE, "propose"), (ROLE_APPROVE, "approve"), (ROLE_EXECUTE, "execute")]
        .iter()
        .filter(|(b, _)| r & b != 0)
        .map(|(_, n)| *n)
        .collect()
}

fn parse_member(ctx: &Ctx, s: &str) -> Result<Member> {
    let parts: Vec<&str> = s.split(':').collect();
    ensure!(parts.len() == 2 || parts.len() == 3, "a member is <key>:<roles>[:weight]: {s}");
    Ok(Member {
        key: ctx.public(parts[0])?,
        roles: roles_of(parts[1])?,
        weight: parts.get(2).map(|w| w.parse()).transpose()?.unwrap_or(1),
    })
}

fn parse_limit(ctx: &Ctx, s: &str) -> Result<LimitSpec> {
    let parts: Vec<&str> = s.split(':').collect();
    ensure!(parts.len() == 4 || parts.len() == 5, "a limit is <spender>[+..]:<min>:<amount>:<period>[:<token>]: {s}");
    Ok(LimitSpec {
        spenders: parts[0].split('+').map(|k| ctx.public(k)).collect::<Result<_>>()?,
        min_signers: parts[1].parse()?,
        amount: parts[2].parse()?,
        period_ms: duration(parts[3])?,
        asset: match parts.get(4) {
            Some(t) => Asset::Token { definition_id: AccountId::from_str(t).map_err(|e| anyhow!("{e:?}"))? },
            None => Asset::Native,
        },
    })
}

/// A configuration file: `{"members": ["<key>:<roles>[:w]", ..], "threshold": n, "time_lock": "10m", "limits": [".."]}`.
#[derive(Deserialize)]
struct ConfigFile {
    members: Vec<String>,
    threshold: u64,
    #[serde(default = "zero")]
    time_lock: String,
    #[serde(default)]
    limits: Vec<String>,
}

fn zero() -> String {
    "0".into()
}

#[derive(Deserialize)]
struct CallFile {
    program: String,
    accounts: Vec<(String, String)>,
    data: String,
}

fn acc(s: &str) -> Result<AccountId> {
    AccountId::from_str(s).map_err(|e| anyhow!("not an account id {s}: {e:?}"))
}

/// The action, decoded from its exact bytes, as a member must see it before
/// signing (U.6). Unknown programs are shown raw, with a warning.
fn describe(program: &AccountId, action: &Action) -> serde_json::Value {
    match action {
        Action::Calls(calls) => serde_json::json!({
            "kind": "calls",
            "calls": calls.iter().map(|c| describe_call(program, c)).collect::<Vec<_>>(),
        }),
        Action::Config(c) => serde_json::json!({
            "kind": "configuration change",
            "members": c.members.iter().map(|m| serde_json::json!({"key": hex::encode(m.key), "roles": role_names(m.roles), "weight": m.weight})).collect::<Vec<_>>(),
            "threshold": c.threshold, "time_lock_ms": c.time_lock_ms,
            "limits": c.limits.iter().map(limit_json).collect::<Vec<_>>(),
        }),
        Action::CancelQueued { digest } => serde_json::json!({"kind": "veto a queued action", "digest": hex::encode(digest)}),
    }
}

fn limit_json(l: &LimitSpec) -> serde_json::Value {
    serde_json::json!({
        "spenders": l.spenders.iter().map(hex::encode).collect::<Vec<_>>(), "min_signers": l.min_signers,
        "asset": asset_json(&l.asset), "amount": l.amount.to_string(), "period_ms": l.period_ms,
    })
}

fn asset_json(a: &Asset) -> String {
    match a {
        Asset::Native => "native".into(),
        Asset::Token { definition_id } => format!("token {definition_id}"),
    }
}

fn describe_call(me: &AccountId, c: &Call) -> serde_json::Value {
    let accounts: Vec<_> = c.accounts.iter().map(|a| format!("{} [{}]", a.account, shard_name(me, &a.shard_program))).collect();
    let decoded = if c.program == NATIVE_TOKEN_PROGRAM_ID {
        borsh::from_slice::<lee_core::native_token::Instruction>(&c.data).ok().map(|lee_core::native_token::Instruction::Transfer { amount }| {
            serde_json::json!({
                "program": "native token", "instruction": "Transfer", "amount": amount.to_string(),
                "from": c.accounts.first().map(|a| a.account.to_string()), "to": c.accounts.get(1).map(|a| a.account.to_string()),
            })
        })
    } else if c.program == core::token_program() {
        borsh::from_slice::<token_core::Instruction>(&c.data).ok().map(|i| match i {
            token_core::Instruction::Transfer { amount_to_transfer, descriptor } => serde_json::json!({
                "program": "token", "instruction": "Transfer", "amount": amount_to_transfer.to_string(),
                "definition": descriptor.definition_id.to_string(),
                "from": c.accounts.first().map(|a| a.account.to_string()), "to": c.accounts.get(1).map(|a| a.account.to_string()),
            }),
            _ => serde_json::json!({"program": "token", "instruction": "other (not a transfer)", "data_hex": hex::encode(&c.data)}),
        })
    } else {
        borsh::from_slice::<Instruction>(&c.data).ok().map(|i| serde_json::json!({"program": "accord_multisig (another instance)", "instruction": format!("{i:?}")}))
    };
    match decoded {
        Some(d) => serde_json::json!({"target": c.program.to_string(), "decoded": d, "accounts": accounts}),
        None => serde_json::json!({
            "target": c.program.to_string(),
            "warning": "This program is not in the registry: the action cannot be decoded.",
            "data_hex": hex::encode(&c.data), "data_len": c.data.len(), "accounts": accounts,
        }),
    }
}

fn shard_name(me: &AccountId, p: &AccountId) -> String {
    if *p == NATIVE_TOKEN_PROGRAM_ID {
        "native".into()
    } else if *p == core::token_program() {
        "token".into()
    } else if p == me {
        "this program".into()
    } else {
        p.to_string()
    }
}

fn image_hex(program: &Program) -> String {
    program.id().iter().flat_map(|w| w.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
}

fn multisig_json(p: &AccountId, id: &[u8; 32], m: &Multisig) -> serde_json::Value {
    serde_json::json!({
        "multisig_id": hex::encode(id), "configuration": config_account(p, id).to_string(), "vault": m.vault.to_string(),
        "initialised": m.initialised != 0, "epoch": m.epoch, "threshold": m.threshold, "time_lock_ms": m.time_lock_ms,
        "members": m.members.iter().map(|x| serde_json::json!({"key": hex::encode(x.key), "roles": role_names(x.roles), "weight": x.weight})).collect::<Vec<_>>(),
        "limits": m.limits.iter().map(|l| { let mut j = limit_json(&l.spec); j["spent"] = l.spent.to_string().into(); j["uses"] = l.uses.into(); j["period_start"] = l.period_start.into(); j }).collect::<Vec<_>>(),
        "creator": m.creator.to_string(), "queued": m.queued, "executed": m.executed,
    })
}

fn queued_json(q: &Queued) -> serde_json::Value {
    let status = match q.status {
        core::STATUS_QUEUED => "queued",
        core::STATUS_EXECUTED => "executed",
        core::STATUS_CANCELLED => "cancelled",
        _ => "unknown",
    };
    serde_json::json!({
        "digest": hex::encode(q.digest), "kind": q.kind, "epoch": q.epoch, "status": status,
        "quorum_at": q.quorum_at, "executable_at": core::executable_at(q), "expires_at": q.expires_at,
        "proposer": hex::encode(q.proposer), "approvers": q.approvers.iter().map(hex::encode).collect::<Vec<_>>(),
        "executed_at": q.executed_at,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let rpc = SequencerClientBuilder::default().build(&cli.rpc).with_context(|| format!("not a sequencer URL: {}", cli.rpc))?;
    let signs = matches!(
        cli.cmd,
        Cmd::Vault { .. } | Cmd::Deploy { .. } | Cmd::Create { .. } | Cmd::Queue { .. } | Cmd::Execute { .. } | Cmd::Activate { .. } | Cmd::Spend { .. } | Cmd::Register { .. }
    );
    let wallet = if signs {
        Some(WalletCore::from_env().await.context("this command sends a transaction, so it needs a v0.3 wallet home (LEE_WALLET_HOME_DIR)")?)
    } else {
        None
    };
    let keys = cli.keys.clone().unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".accord/keys"));
    let ctx = Ctx { wallet, rpc, program: cli.program, keys, payer: cli.payer, force: cli.force, label: cli.label.clone() };
    run(ctx, cli.cmd, &cli.elf).await
}

async fn run(mut ctx: Ctx, cmd: Cmd, elf: &PathBuf) -> Result<()> {
    match cmd {
        Cmd::Key { op, name } => {
            let p = ctx.key_path(&name);
            if op == "new" {
                ensure!(!p.exists(), "approval key {name} exists at {}", p.display());
                std::fs::create_dir_all(&ctx.keys)?;
                let sk = PrivateKey::new_os_random();
                std::fs::write(&p, hex::encode(sk.value()))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
                }
            }
            println!("{}", serde_json::json!({"name": name, "approval_key": hex::encode(ctx.public(&name)?)}));
        }
        Cmd::ImageId => {
            let program = Program::new(std::fs::read(elf).with_context(|| format!("reading {}", elf.display()))?.into())?;
            println!("{}", image_hex(&program));
        }
        Cmd::Now => println!("{}", ctx.now().await?),
        Cmd::Ids { multisig, digest } => {
            let p = ctx.program()?;
            let id = id32(&multisig)?;
            let mut j = serde_json::json!({"multisig_id": hex::encode(id), "configuration": config_account(&p, &id).to_string(), "vault": vault_account(&p, &id).to_string()});
            if let Some(d) = digest {
                j["queued"] = queued_account(&p, &id, &id32(&d)?).to_string().into();
            }
            println!("{j}");
        }
        Cmd::Deploy { payer, header, segment, immutable } => {
            let bytecode = std::fs::read(elf).with_context(|| format!("reading {}", elf.display()))?;
            let program = Program::new(bytecode.clone().into())?;
            let user_elf = risc0_binfmt::ProgramBinary::decode(&bytecode).map_err(|e| anyhow!("{e}"))?.user_elf.len();
            let segments = user_elf.div_ceil(program_loader_core::MAX_SEGMENT_DATA_LEN);
            let wallet = ctx.wallet.as_mut().expect("deploy opens the wallet");
            let (header, segs) = match header {
                Some(h) => {
                    ensure!(segment.len() == segments, "this binary needs {segments} segment accounts, got {}", segment.len());
                    (h, segment)
                }
                None => {
                    let h = wallet.create_new_account_public(None).0;
                    let s: Vec<AccountId> = std::iter::repeat_with(|| wallet.create_new_account_public(None).0).take(segments).collect();
                    wallet.store_persistent_data()?;
                    (h, s)
                }
            };
            ProgramLoader(&*wallet).deploy(header, &segs, bytecode, immutable, Some(payer)).await?;
            println!("{}", serde_json::json!({"op": "deploy", "header": header.to_string(), "image_id": image_hex(&program), "segments": segs.iter().map(|s| s.to_string()).collect::<Vec<_>>()}));
        }
        Cmd::Create { multisig, members, threshold, time_lock, limits } => {
            let p = ctx.program()?;
            let id = id32(&multisig)?;
            let config = Config {
                members: members.iter().map(|m| parse_member(&ctx, m)).collect::<Result<_>>()?,
                threshold,
                time_lock_ms: duration(&time_lock)?,
                limits: limits.iter().map(|l| parse_limit(&ctx, l)).collect::<Result<_>>()?,
            };
            let creator = ctx.payer.ok_or_else(|| anyhow!("create names the fee payer as creator: pass --payer"))?;
            let cfg = config_account(&p, &id);
            ctx.send("create", &Instruction::Create { multisig_id: id, config }, rows::create(&p, &id, creator), &[creator], &[(cfg, p)]).await?;
        }
        Cmd::Show { multisig } => {
            let p = ctx.program()?;
            let id = id32(&multisig)?;
            let m = ctx.multisig(&id).await?;
            let mut j = multisig_json(&p, &id, &m);
            let vault = ctx.account(m.vault).await?;
            j["vault_native"] = vault.data.native_balance().unwrap_or(0).to_string().into();
            j["clock"] = ctx.now().await.unwrap_or(0).into();
            println!("{j}");
        }
        Cmd::Queued { multisig, digest } => {
            let p = ctx.program()?;
            let id = id32(&multisig)?;
            let d = id32(&digest)?;
            let bytes = ctx.shard(queued_account(&p, &id, &d), p).await?;
            ensure!(!bytes.is_empty(), "nothing is queued at digest {digest}");
            println!("{}", queued_json(&borsh::from_slice(&bytes)?));
        }
        Cmd::Propose { multisig, transfer, from, calls, config, cancel, expires, key, description, out } => {
            let p = ctx.program()?;
            let id = id32(&multisig)?;
            let m = ctx.multisig(&id).await?;
            let mut chosen = 0;
            let mut action = None;
            if !transfer.is_empty() {
                chosen += 1;
                let mut cs = vec![];
                for t in &transfer {
                    let parts: Vec<&str> = t.split(':').collect();
                    ensure!(parts.len() == 2 || parts.len() == 3, "a transfer is <to>:<amount>[:<token>]: {t}");
                    let asset = match parts.get(2) {
                        Some(d) => Asset::Token { definition_id: acc(d)? },
                        None => Asset::Native,
                    };
                    let mut c = vault_transfer(&p, &id, &asset, acc(parts[0])?, parts[1].parse()?);
                    if let Some(f) = from {
                        c.accounts[0].account = f;
                    }
                    cs.push(c);
                }
                action = Some(Action::Calls(cs));
            }
            if let Some(f) = calls {
                chosen += 1;
                let list: Vec<CallFile> = serde_json::from_slice(&std::fs::read(&f)?)?;
                let cs = list
                    .iter()
                    .map(|c| {
                        Ok(Call {
                            program: acc(&c.program)?,
                            accounts: c.accounts.iter().map(|(a, s)| Ok(CallAccount { account: acc(a)?, shard_program: acc(s)? })).collect::<Result<_>>()?,
                            data: hex::decode(&c.data)?,
                        })
                    })
                    .collect::<Result<_>>()?;
                action = Some(Action::Calls(cs));
            }
            if let Some(f) = config {
                chosen += 1;
                let c: ConfigFile = serde_json::from_slice(&std::fs::read(&f)?)?;
                action = Some(Action::Config(Config {
                    members: c.members.iter().map(|x| parse_member(&ctx, x)).collect::<Result<_>>()?,
                    threshold: c.threshold,
                    time_lock_ms: duration(&c.time_lock)?,
                    limits: c.limits.iter().map(|l| parse_limit(&ctx, l)).collect::<Result<_>>()?,
                }));
            }
            if let Some(d) = cancel {
                chosen += 1;
                action = Some(Action::CancelQueued { digest: id32(&d)? });
            }
            ensure!(chosen == 1, "name exactly one action: --transfer, --calls, --config or --cancel");
            let action = action.expect("one action");
            let expires_at = match expires.strip_prefix('+') {
                Some(d) => ctx.now().await? + duration(d)?,
                None => expires.parse()?,
            };
            let cfg = config_account(&p, &id);
            let digest = action_digest(&cfg, m.epoch, expires_at, &action);
            let pkg = Package {
                program: p.to_string(),
                multisig_id: hex::encode(id),
                config_account: cfg.to_string(),
                epoch: m.epoch,
                expires_at,
                action: hex::encode(borsh::to_vec(&action)?),
                digest: hex::encode(digest),
                description,
                proposer: Some((&ctx.sign(&key, &propose_message(&digest))?).into()),
                approvals: vec![],
            };
            pkg.write(&out)?;
            println!("{}", serde_json::json!({"op": "propose", "package": out.display().to_string(), "digest": pkg.digest, "epoch": m.epoch, "expires_at": expires_at, "action": describe(&p, &action)}));
        }
        Cmd::Decode { package } => {
            let pkg = Package::read(&package)?;
            let d = pkg.checked_digest()?;
            let p = pkg.program()?;
            println!(
                "{}",
                serde_json::json!({
                    "digest": hex::encode(d), "digest_matches_bytes": true, "epoch": pkg.epoch, "expires_at": pkg.expires_at,
                    "action": describe(&p, &pkg.action()?),
                    "proposer_description (not signed)": pkg.description,
                    "approvals": pkg.approvals.len(),
                })
            );
        }
        Cmd::Approve { package, key } => {
            let mut pkg = Package::read(&package)?;
            let d = pkg.checked_digest()?;
            let p = pkg.program()?;
            eprintln!("{}", serde_json::to_string_pretty(&describe(&p, &pkg.action()?))?);
            let s = ctx.sign(&key, &approve_message(&d))?;
            ensure!(!pkg.approvals.iter().any(|a| a.key == hex::encode(s.key)), "this key already approved the package");
            pkg.approvals.push((&s).into());
            pkg.write(&package)?;
            println!("{}", serde_json::json!({"op": "approve", "digest": hex::encode(d), "key": hex::encode(s.key), "approvals": pkg.approvals.len()}));
        }
        Cmd::Queue { package } => {
            let pkg = Package::read(&package)?;
            let d = pkg.checked_digest()?;
            let p = pkg.program()?;
            ensure!(p == ctx.program()?, "the package is for program {p}");
            let id = pkg.multisig_id()?;
            let m = ctx.multisig(&id).await?;
            let approvals: Vec<Sig> = pkg.approvals.iter().map(Sig::try_from).collect::<Result<_>>()?;
            if !ctx.force {
                let w: u64 = approvals.iter().filter_map(|a| m.members.iter().find(|x| x.key == a.key && x.roles & ROLE_APPROVE != 0)).map(|x| u64::from(x.weight)).sum();
                ensure!(w >= m.threshold, "approvals weigh {w} of the threshold {}: {}", m.threshold, errors::advice(errors::E_BELOW_THRESHOLD));
                ensure!(pkg.epoch == m.epoch, "{}", errors::advice(errors::E_WRONG_EPOCH));
            }
            let ix = Instruction::Queue {
                multisig_id: id,
                epoch: pkg.epoch,
                expires_at: pkg.expires_at,
                action: pkg.action()?,
                time_lock_ms: m.time_lock_ms,
                proposer: Sig::try_from(pkg.proposer.as_ref().ok_or_else(|| anyhow!("the package has no proposer signature"))?)?,
                approvals,
                at: ctx.now().await?,
            };
            let q = queued_account(&p, &id, &d);
            ctx.send("queue", &ix, rows::queue(&p, &id, &d), &[], &[(q, p)]).await?;
        }
        Cmd::Execute { package, key, private_vault } => {
            let pkg = Package::read(&package)?;
            let d = pkg.checked_digest()?;
            let p = ctx.program()?;
            let id = pkg.multisig_id()?;
            let action = pkg.action()?;
            let q = queued_account(&p, &id, &d);
            if !ctx.force {
                let bytes = ctx.shard(q, p).await?;
                ensure!(!bytes.is_empty(), "{}", errors::advice(errors::E_NOT_QUEUED));
                let qd: Queued = borsh::from_slice(&bytes)?;
                let now = ctx.now().await?;
                ensure!(qd.status == core::STATUS_QUEUED, "{}", errors::advice(errors::E_NOT_QUEUED));
                ensure!(action.kind() == core::KIND_CANCEL || now >= core::executable_at(&qd), "{} (executable at {})", errors::advice(errors::E_TIME_LOCKED), core::executable_at(&qd));
            }
            let ix = Instruction::Execute {
                multisig_id: id,
                epoch: pkg.epoch,
                expires_at: pkg.expires_at,
                action: action.clone(),
                executor: ctx.sign(&key, &execute_message(&d))?,
                at: ctx.now().await?,
            };
            match private_vault {
                None => {
                    ctx.send("execute", &ix, rows::execute(&p, &id, &d, &action), &[], &[(q, p)]).await?;
                }
                Some(group) => {
                    let (vault, identity) = private_vault_identity(&ctx, &id, &group)?;
                    let mentions: Vec<AccountMention> = rows::execute(&p, &id, &d, &action)
                        .into_iter()
                        .map(|r| {
                            if r.account_id == vault {
                                identity.clone().select_program_shard(r.program_account_id)
                            } else {
                                AccountIdentity::PublicNoSign(r.account_id).select_program_shard(r.program_account_id)
                            }
                        })
                        .collect();
                    let program = Program::new(std::fs::read(elf).with_context(|| format!("reading {}", elf.display()))?.into())?;
                    let pwd = lee::privacy_preserving_transaction::circuit::ProgramWithDependencies::new(program, p, std::collections::HashMap::new());
                    let before = ctx.shard(q, p).await?;
                    eprintln!("proving the private-vault execution locally; this takes minutes");
                    let sent = ctx.wallet().send_privacy_preserving_tx(mentions, Program::serialize_instruction(ix)?, &pwd).await.map(|(h, _)| h);
                    report_private(&ctx, "execute-private", sent.map_err(|e| anyhow!("{e:?}")), q, p, before, vault).await?;
                }
            }
        }
        Cmd::Activate { multisig, signers } => {
            let p = ctx.program()?;
            let id = id32(&multisig)?;
            let m = ctx.multisig(&id).await?;
            let msg = activate_message(&config_account(&p, &id), m.epoch);
            let approvals = signers.iter().map(|k| ctx.sign(k, &msg)).collect::<Result<_>>()?;
            let cfg = config_account(&p, &id);
            ctx.send("activate", &Instruction::Activate { multisig_id: id, epoch: m.epoch, approvals }, rows::activate(&p, &id), &[], &[(cfg, p)]).await?;
        }
        Cmd::Vault { op } => vault_cmd(&mut ctx, op, elf).await?,
        Cmd::Spend { multisig, limit, to, amount, token, signers, private_vault } => {
            let p = ctx.program()?;
            let id = id32(&multisig)?;
            let m = ctx.multisig(&id).await?;
            let l = m.limits.get(limit as usize).ok_or_else(|| anyhow!("{}", errors::advice(errors::E_BAD_LIMIT)))?;
            let asset = token.map_or(Asset::Native, |definition_id| Asset::Token { definition_id });
            let cfg = config_account(&p, &id);
            let msg = spend_message(&cfg, limit, l.uses, &to, &asset, amount);
            let approvals = signers.iter().map(|k| ctx.sign(k, &msg)).collect::<Result<_>>()?;
            let ix = Instruction::Spend { multisig_id: id, limit, uses: l.uses, asset, amount, approvals, at: ctx.now().await? };
            match private_vault {
                None => {
                    ctx.send("spend", &ix, rows::spend(&p, &id, &asset, to), &[], &[(cfg, p)]).await?;
                }
                Some(group) => {
                    let (vault, identity) = private_vault_identity(&ctx, &id, &group)?;
                    let mentions = vec![
                        AccountIdentity::PublicNoSign(cfg).select_program_shard(p),
                        identity.select_program_shard(asset.shard_program()),
                        AccountIdentity::PublicNoSign(to).select_program_shard(asset.shard_program()),
                    ];
                    let program = Program::new(std::fs::read(elf).with_context(|| format!("reading {}", elf.display()))?.into())?;
                    let pwd = lee::privacy_preserving_transaction::circuit::ProgramWithDependencies::new(program, p, std::collections::HashMap::new());
                    let before = ctx.shard(cfg, p).await?;
                    eprintln!("proving the private-vault spend locally; this takes minutes");
                    let sent = ctx.wallet().send_privacy_preserving_tx(mentions, Program::serialize_instruction(ix)?, &pwd).await.map(|(h, _)| h);
                    report_private(&ctx, "spend-private", sent.map_err(|e| anyhow!("{e:?}")), cfg, p, before, vault).await?;
                }
            }
        }
        Cmd::Room { op } => room_cmd(&ctx, op).await?,
        Cmd::Register { target, idl, name, image_id } => {
            let p = ctx.program()?;
            let bytes = std::fs::read(&idl)?;
            let idl_sha256 = core::sha256(&[&bytes]);
            let image_id: [u32; 8] = match image_id {
                Some(h) => {
                    let b = hex::decode(h)?;
                    ensure!(b.len() == 32, "an ImageID is 32 bytes");
                    std::array::from_fn(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()))
                }
                None => {
                    let shard = ctx.shard(target, lee_core::program::PROGRAM_LOADER_ACCOUNT_ID).await?;
                    lee_core::program::ProgramHeader::from_bytes(&shard).ok_or_else(|| anyhow!("{target} holds no program header; pass --image-id"))?.image_id
                }
            };
            let registrant = ctx.payer.ok_or_else(|| anyhow!("register names the fee payer as registrant: pass --payer"))?;
            let entry = registry_account(&p, &target, &idl_sha256);
            ctx.send("register", &Instruction::Register { program: target, image_id, idl_sha256, name }, rows::register(&p, &target, &idl_sha256, registrant), &[registrant], &[(entry, p)]).await?;
        }
    }
    Ok(())
}

fn read_json<T: for<'a> Deserialize<'a>>(p: &PathBuf) -> Result<T> {
    serde_json::from_slice(&std::fs::read(p).with_context(|| format!("reading {}", p.display()))?).with_context(|| format!("parsing {}", p.display()))
}

/// Merge `incoming`'s valid approvals into the package stored at `path`
/// (or store it). Signatures are checked here, against the digest the
/// package's own bytes hash to; the program checks them again.
fn merge_package(path: &PathBuf, incoming: &Package) -> Result<usize> {
    let d = incoming.checked_digest()?;
    let mut base = if path.exists() { Package::read(path)? } else { Package { approvals: vec![], ..incoming.clone() } };
    ensure!(base.digest == incoming.digest, "a different package is stored at {}", path.display());
    if base.proposer.is_none() {
        base.proposer = incoming.proposer.clone();
    }
    for a in &incoming.approvals {
        let s = Sig::try_from(a)?;
        if accord_core::verify(&s.key, &approve_message(&d), &s.sig) && !base.approvals.iter().any(|b| b.key == a.key) {
            base.approvals.push(a.clone());
        }
    }
    base.write(path)?;
    Ok(base.approvals.len())
}

/// The node library takes over the process's standard output once it starts,
/// so results are written through a copy of fd 1 taken before.
static OUT: std::sync::OnceLock<std::sync::Mutex<std::fs::File>> = std::sync::OnceLock::new();

macro_rules! say {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let mut f = OUT.get().expect("room output").lock().unwrap();
        let _ = writeln!(f, $($t)*);
        let _ = f.flush();
    }};
}

async fn room_cmd(ctx: &Ctx, op: RoomCmd) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd as _;
        let fd = unsafe { libc_dup(1) };
        ensure!(fd >= 0, "cannot duplicate stdout");
        let _ = OUT.set(std::sync::Mutex::new(unsafe { std::fs::File::from_raw_fd(fd) }));
    }
    match op {
        RoomCmd::Open { multisig, key, out, invites, publish } => {
            let p = ctx.program()?;
            let id = id32(&multisig)?;
            let m = ctx.multisig(&id).await?;
            let me = ctx.public(&key)?;
            ensure!(m.members.iter().any(|x| x.key == me), "{key} is not a member of this configuration");
            let mut secret = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret);
            let r = room::Room { program: p.to_string(), multisig_id: hex::encode(id), config_account: config_account(&p, &id).to_string(), epoch: m.epoch, secret: hex::encode(secret) };
            let inv: Vec<room::Invite> = m.members.iter().map(|x| room::seal_invite(&x.key, &secret, m.epoch)).collect::<Result<_>>()?;
            std::fs::write(&out, serde_json::to_vec_pretty(&r)?)?;
            std::fs::write(&invites, serde_json::to_vec_pretty(&inv)?)?;
            if publish {
                let d = room::Delivery::up()?;
                let t = room::invite_topic(&config_account(&p, &id));
                for i in &inv {
                    d.publish(&t, &serde_json::to_vec(i)?)?;
                }
                std::thread::sleep(Duration::from_secs(10));
            }
            say!("{}", serde_json::json!({"op": "room-open", "epoch": m.epoch, "topic": r.topic()?, "invites": inv.len(), "invite_topic": room::invite_topic(&config_account(&p, &id))}));
        }
        RoomCmd::Join { multisig, key, out, invites, seconds } => {
            let p = ctx.program()?;
            let id = id32(&multisig)?;
            let me = ctx.public(&key)?;
            let sk = ctx.secret(&key)?;
            let mine = |i: &room::Invite| room::open_invite(i, sk.value(), &me).ok().map(|s| (s, i.epoch));
            let found = match invites {
                Some(f) => read_json::<Vec<room::Invite>>(&f)?.iter().find_map(mine),
                None => {
                    let d = room::Delivery::up()?;
                    let t = room::invite_topic(&config_account(&p, &id));
                    d.subscribe(&t)?;
                    let start = std::time::Instant::now();
                    let mut got = None;
                    while got.is_none() && start.elapsed() < Duration::from_secs(seconds) {
                        while let Some((topic, payload)) = d.next() {
                            if topic == t {
                                if let Ok(i) = serde_json::from_slice::<room::Invite>(&payload) {
                                    got = got.or_else(|| mine(&i));
                                }
                            }
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    got
                }
            };
            let (secret, epoch) = found.ok_or_else(|| anyhow!("no invite opens with {key}"))?;
            let r = room::Room { program: p.to_string(), multisig_id: hex::encode(id), config_account: config_account(&p, &id).to_string(), epoch, secret: hex::encode(secret) };
            std::fs::write(&out, serde_json::to_vec_pretty(&r)?)?;
            say!("{}", serde_json::json!({"op": "room-join", "epoch": epoch, "topic": r.topic()?}));
        }
        RoomCmd::Post { room: rp, key, package, say, linger } => {
            let r: room::Room = read_json(&rp)?;
            let signer = |m: &[u8; 32]| ctx.sign(&key, m);
            let frame = match (package, say) {
                (Some(f), None) => {
                    let pkg = Package::read(&f)?;
                    pkg.checked_digest()?;
                    r.seal(&signer, "package", &serde_json::to_value(&pkg)?)?
                }
                (None, Some(text)) => r.seal(&signer, "chat", &serde_json::json!({"text": text}))?,
                _ => bail!("post one of --package or --say"),
            };
            let d = room::Delivery::up()?;
            d.publish(&r.topic()?, &frame)?;
            std::thread::sleep(Duration::from_secs(linger));
            say!("{}", serde_json::json!({"op": "room-post", "topic": r.topic()?, "bytes": frame.len()}));
        }
        RoomCmd::Listen { room: rp, into, seconds, until_approvals } => {
            let r: room::Room = read_json(&rp)?;
            let id = id32(&r.multisig_id)?;
            let members: Vec<[u8; 32]> = ctx.multisig(&id).await?.members.iter().map(|m| m.key).collect();
            std::fs::create_dir_all(&into)?;
            let d = room::Delivery::up()?;
            let t = r.topic()?;
            d.subscribe(&t)?;
            eprintln!("listening on {t}");
            let start = std::time::Instant::now();
            let mut most = 0;
            while start.elapsed() < Duration::from_secs(seconds) {
                while let Some((topic, payload)) = d.next() {
                    if topic != t {
                        continue;
                    }
                    match r.open(&payload, &members) {
                        Ok((who, body)) => match body["kind"].as_str() {
                            Some("package") => {
                                let pkg: Package = serde_json::from_value(body["body"].clone())?;
                                let path = into.join(format!("{}.json", pkg.digest));
                                match merge_package(&path, &pkg) {
                                    Ok(n) => {
                                        most = most.max(n);
                                        say!("{}", serde_json::json!({"op": "room-package", "from": hex::encode(who), "digest": pkg.digest, "approvals": n, "path": path.display().to_string()}));
                                    }
                                    Err(e) => say!("{}", serde_json::json!({"op": "room-dropped", "from": hex::encode(who), "why": e.to_string()})),
                                }
                            }
                            _ => say!("{}", serde_json::json!({"op": "room-chat", "from": hex::encode(who), "text": body["body"]["text"]})),
                        },
                        Err(e) => say!("{}", serde_json::json!({"op": "room-dropped", "why": e.to_string()})),
                    }
                }
                if until_approvals.is_some_and(|n| most >= n) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    Ok(())
}

extern "C" {
    #[link_name = "dup"]
    fn libc_dup(fd: i32) -> i32;
}

/// The private vault of `multisig_id` under the wallet group `group`: a
/// private PDA of this program under the vault seed, with keys every holder
/// of the group's Group Master Secret derives alike.
fn private_vault_identity(ctx: &Ctx, multisig_id: &[u8; 32], group: &str) -> Result<(AccountId, AccountIdentity)> {
    let p = ctx.program()?;
    let holder = ctx
        .wallet()
        .storage()
        .key_chain()
        .group_key_holder(&wallet::account::Label::new(group))
        .ok_or_else(|| anyhow!("no wallet group {group}: create it with `wallet group new {group}`, or join it"))?;
    // The derivation input names this program instance; any 32 bytes serve.
    let pid: [u32; 8] = std::array::from_fn(|i| u32::from_le_bytes(p.value()[i * 4..i * 4 + 4].try_into().unwrap()));
    let seed = core::vault_seed(multisig_id);
    let keys = holder.derive_keys_for_pda(&pid, &seed);
    let (npk, vpk) = (keys.generate_nullifier_public_key(), keys.generate_viewing_public_key());
    let identifier = lee_core::Identifier::ZERO;
    let id = AccountId::for_private_pda(&p, &seed, &npk, &vpk, identifier);
    Ok((id, AccountIdentity::PrivatePdaShared { authority: p, seed, nsk: keys.nullifier_secret_key(), vpk, identifier }))
}

async fn report_private(ctx: &Ctx, op: &str, sent: Result<common::HashType>, watch: AccountId, prog: AccountId, before: Vec<u8>, vault: AccountId) -> Result<()> {
    let hash = match sent {
        Ok(h) => h,
        Err(e) => {
            println!("{}", serde_json::json!({"op": op, "label": ctx.label, "verdict": "rejected", "error": e.to_string(), "vault": vault.to_string()}));
            return Ok(());
        }
    };
    let polled = tokio::time::timeout(Duration::from_secs(240), ctx.wallet().poll_transaction(hash)).await;
    let block = match polled {
        Ok(Ok((_, b))) => Some(b),
        _ => None,
    };
    let after = ctx.shard(watch, prog).await?;
    let verdict = match block {
        None => "not-included",
        Some(_) if after != before => "applied",
        Some(_) => "reverted",
    };
    println!("{}", serde_json::json!({"op": op, "label": ctx.label, "tx": hash.to_string(), "block": block, "verdict": verdict, "vault": vault.to_string()}));
    Ok(())
}

async fn vault_cmd(ctx: &mut Ctx, op: VaultCmd, _elf: &PathBuf) -> Result<()> {
    match op {
        VaultCmd::Track { multisig, group } => {
            let id = id32(&multisig)?;
            let (vault, _) = private_vault_identity(ctx, &id, &group)?;
            let p = ctx.program()?;
            let pid: [u32; 8] = std::array::from_fn(|i| u32::from_le_bytes(p.value()[i * 4..i * 4 + 4].try_into().unwrap()));
            let w = ctx.wallet.as_mut().expect("vault opens the wallet");
            if w.storage().key_chain().shared_private_account(vault).is_none() {
                w.storage_mut().key_chain_mut().insert_shared_private_account(
                    vault,
                    wallet::storage::key_chain::SharedAccountEntry {
                        group_label: wallet::account::Label::new(&group),
                        identifier: lee_core::Identifier::ZERO,
                        pda_seed: Some(core::vault_seed(&id)),
                        authority_program_id: Some(pid),
                        account: lee::Account::default(),
                    },
                );
            }
            w.sync_to_latest_block().await?;
            w.store_persistent_data()?;
            let bal = w.storage().key_chain().shared_private_account(vault).map(|e| e.account.data.native_balance().unwrap_or(0)).unwrap_or(0);
            println!("{}", serde_json::json!({"op": "vault-track", "private_vault": vault.to_string(), "balance_seen_by_this_member": bal.to_string()}));
        }
        VaultCmd::Id { multisig, group } => {
            let (id, _) = private_vault_identity(ctx, &id32(&multisig)?, &group)?;
            println!("{}", serde_json::json!({"private_vault": id.to_string()}));
        }
        VaultCmd::Fund { multisig, group, from, amount } => {
            let (vault, identity) = private_vault_identity(ctx, &id32(&multisig)?, &group)?;
            let mentions = vec![AccountIdentity::Public(from).balance(), identity.balance()];
            let ix = Program::serialize_instruction(lee_core::native_token::Instruction::Transfer { amount })?;
            eprintln!("proving the transfer into the private vault locally");
            let sent = ctx
                .wallet()
                .send_privacy_preserving_tx(mentions, ix, &lee::privacy_preserving_transaction::circuit::ProgramWithDependencies::native())
                .await
                .map(|(h, _)| h);
            let hash = sent.map_err(|e| anyhow!("{e:?}"))?;
            let polled = tokio::time::timeout(Duration::from_secs(240), ctx.wallet().poll_transaction(hash)).await;
            let block = polled.ok().and_then(|r| r.ok()).map(|(_, b)| b);
            println!("{}", serde_json::json!({"op": "vault-fund", "label": ctx.label, "tx": hash.to_string(), "block": block, "verdict": if block.is_some() { "applied" } else { "not-included" }, "vault": vault.to_string()}));
        }
        VaultCmd::Steal { multisig, group, to, amount } => {
            let (vault, identity) = private_vault_identity(ctx, &id32(&multisig)?, &group)?;
            let mentions = vec![identity.balance(), AccountIdentity::PublicNoSign(to).balance()];
            let ix = Program::serialize_instruction(lee_core::native_token::Instruction::Transfer { amount })?;
            let sent = ctx
                .wallet()
                .send_privacy_preserving_tx(mentions, ix, &lee::privacy_preserving_transaction::circuit::ProgramWithDependencies::native())
                .await
                .map(|(h, _)| h);
            let line = match sent {
                Err(e) => serde_json::json!({"op": "vault-steal", "label": ctx.label, "verdict": "rejected", "error": format!("{e:?}"), "vault": vault.to_string()}),
                Ok(h) => {
                    let polled = tokio::time::timeout(Duration::from_secs(240), ctx.wallet().poll_transaction(h)).await;
                    let block = polled.ok().and_then(|r| r.ok()).map(|(_, b)| b);
                    serde_json::json!({"op": "vault-steal", "label": ctx.label, "tx": h.to_string(), "block": block, "verdict": if block.is_some() { "applied" } else { "not-included" }, "vault": vault.to_string()})
                }
            };
            println!("{line}");
        }
    }
    Ok(())
}
