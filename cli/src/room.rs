//! The coordination room: approval packages and deliberation, end-to-end
//! encrypted, over Logos Delivery.
//!
//! **Keys.** A room has a 32-byte secret per configuration epoch. The member
//! who opens it seals the secret to every current member's approval key
//! (ECDH on secp256k1 with an ephemeral key, then ChaCha20-Poly1305), and
//! posts the sealed invites. A member's client opens its invite with its own
//! approval key; nobody else can.
//!
//! **Topic.** Derived from the room secret and the epoch, so an observer of
//! Delivery cannot tell which multisig a topic belongs to, or that two epochs
//! of one room are related (Pr.2). The invite topic is derived from the
//! configuration account, which only members and disclosure audiences know
//! under the private posture.
//!
//! **Messages.** Every frame is `nonce ‖ ChaCha20-Poly1305(room key, body)`.
//! The body names its sender's approval key and carries that key's BIP-340
//! signature over [`bind_message`], binding the frame to the multisig, the
//! epoch and the body. A receiver drops a frame whose sender is not a member
//! of the configuration it reads from chain. The program, not the room, is
//! the authority on approvals: a frame can only carry signatures the program
//! will verify again.
//!
//! **Transport.** `liblogosdelivery`, opened at run time (`ACCORD_DELIVERY_LIB`):
//! a Core-mode node on the `logos.dev` preset, relay subscribe and send, and
//! the `onMessageReceived` listener.

use std::{
    collections::VecDeque,
    ffi::{c_char, c_int, c_void, CStr, CString},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use accord_core::{bind_message, sha256, verify, Sig};
use anyhow::{anyhow, bail, ensure, Context as _, Result};
use chacha20poly1305::{aead::Aead as _, ChaCha20Poly1305, KeyInit as _, Nonce};
use k256::{
    elliptic_curve::sec1::{FromEncodedPoint as _, ToEncodedPoint as _},
    AffinePoint, EncodedPoint, ProjectivePoint, Scalar,
};
use lee::AccountId;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};

pub const TAG_TOPIC: &[u8] = b"accord_multisig/v1/room/topic";
pub const TAG_KEY: &[u8] = b"accord_multisig/v1/room/key";
pub const TAG_INVITE: &[u8] = b"accord_multisig/v1/room/invite";
pub const TAG_INVITE_TOPIC: &[u8] = b"accord_multisig/v1/room/invites";

/// What a member keeps locally about one room.
#[derive(Serialize, Deserialize, Clone)]
pub struct Room {
    pub program: String,
    pub multisig_id: String,
    pub config_account: String,
    pub epoch: u32,
    /// Never leaves the member's machine.
    pub secret: String,
}

impl Room {
    pub fn secret(&self) -> Result<[u8; 32]> {
        hex::decode(&self.secret)?.try_into().map_err(|_| anyhow!("a room secret is 32 bytes"))
    }
    pub fn config(&self) -> Result<AccountId> {
        self.config_account.parse().map_err(|e| anyhow!("{e:?}"))
    }
    pub fn topic(&self) -> Result<String> {
        Ok(topic(&self.secret()?, self.epoch))
    }
    fn key(&self) -> Result<[u8; 32]> {
        Ok(sha256(&[TAG_KEY, &self.secret()?, &self.epoch.to_le_bytes()]))
    }

    /// Seal a body for the room, signed by the sender's approval key.
    pub fn seal(&self, sender: &dyn Fn(&[u8; 32]) -> Result<Sig>, kind: &str, body: &serde_json::Value) -> Result<Vec<u8>> {
        let text = serde_json::to_vec(&serde_json::json!({"kind": kind, "body": body}))?;
        let binding = bind_message(&self.config()?, self.epoch, &sha256(&[&text]));
        let s = sender(&binding)?;
        let frame = serde_json::to_vec(&Frame { sender: hex::encode(s.key), sig: hex::encode(s.sig), text: String::from_utf8(text)? })?;
        let cipher = ChaCha20Poly1305::new(&self.key()?.into());
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = cipher.encrypt(Nonce::from_slice(&nonce), frame.as_slice()).map_err(|_| anyhow!("sealing failed"))?;
        Ok([nonce.to_vec(), ct].concat())
    }

    /// Open a frame: decrypt, check the sender's signature, and return the
    /// sender key and the body. `members` are the current approval keys.
    pub fn open(&self, bytes: &[u8], members: &[[u8; 32]]) -> Result<([u8; 32], serde_json::Value)> {
        ensure!(bytes.len() > 12, "a frame is a nonce and a ciphertext");
        let cipher = ChaCha20Poly1305::new(&self.key()?.into());
        let plain = cipher
            .decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
            .map_err(|_| anyhow!("not sealed with this room's key"))?;
        let f: Frame = serde_json::from_slice(&plain)?;
        let key: [u8; 32] = hex::decode(&f.sender)?.try_into().map_err(|_| anyhow!("a key is 32 bytes"))?;
        let sig: [u8; 64] = hex::decode(&f.sig)?.try_into().map_err(|_| anyhow!("a signature is 64 bytes"))?;
        ensure!(members.contains(&key), "the sender is not a member of this configuration");
        let binding = bind_message(&self.config()?, self.epoch, &sha256(&[f.text.as_bytes()]));
        ensure!(verify(&key, &binding, &sig), "the sender's signature does not verify");
        Ok((key, serde_json::from_str(&f.text)?))
    }
}

#[derive(Serialize, Deserialize)]
struct Frame {
    sender: String,
    sig: String,
    text: String,
}

#[must_use]
pub fn topic(secret: &[u8; 32], epoch: u32) -> String {
    let h = sha256(&[TAG_TOPIC, secret, &epoch.to_le_bytes()]);
    format!("/accord/1/{}/proto", hex::encode(&h[..16]))
}

#[must_use]
pub fn invite_topic(config: &AccountId) -> String {
    let h = sha256(&[TAG_INVITE_TOPIC, config.value()]);
    format!("/accord/1/{}/proto", hex::encode(&h[..16]))
}

/// BIP-340's lift_x: the point with even y whose x is `key`.
fn lift_x(key: &[u8; 32]) -> Result<ProjectivePoint> {
    let mut sec1 = [0u8; 33];
    sec1[0] = 2;
    sec1[1..].copy_from_slice(key);
    let ep = EncodedPoint::from_bytes(sec1).map_err(|_| anyhow!("not a key"))?;
    Option::<AffinePoint>::from(AffinePoint::from_encoded_point(&ep)).map(ProjectivePoint::from).ok_or_else(|| anyhow!("not a curve point"))
}

fn x_of(p: &ProjectivePoint) -> [u8; 32] {
    let enc = p.to_affine().to_encoded_point(true);
    enc.as_bytes()[1..].try_into().expect("32 bytes")
}

fn scalar(sk: &[u8; 32]) -> Result<Scalar> {
    Option::<Scalar>::from(<Scalar as k256::elliptic_curve::PrimeField>::from_repr((*sk).into())).ok_or_else(|| anyhow!("not a scalar"))
}

/// A room secret sealed to one member's approval key.
#[derive(Serialize, Deserialize, Clone)]
pub struct Invite {
    pub to: String,
    pub ephemeral: String,
    pub sealed: String,
    pub epoch: u32,
}

/// Seal `secret` to `member` (an x-only approval key). The x-coordinate of
/// the shared point is the same whichever y the member's key has, so the
/// member opens it with their own secret key.
pub fn seal_invite(member: &[u8; 32], secret: &[u8; 32], epoch: u32) -> Result<Invite> {
    let mut e = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut e);
    let es = scalar(&e)?;
    let eph = x_of(&(ProjectivePoint::GENERATOR * es));
    let shared = x_of(&(lift_x(member)? * es));
    let key = sha256(&[TAG_INVITE, &shared, &eph, member]);
    let ct = ChaCha20Poly1305::new(&key.into())
        .encrypt(Nonce::from_slice(&[0u8; 12]), secret.as_slice())
        .map_err(|_| anyhow!("sealing failed"))?;
    Ok(Invite { to: hex::encode(member), ephemeral: hex::encode(eph), sealed: hex::encode(ct), epoch })
}

pub fn open_invite(inv: &Invite, sk: &[u8; 32], member: &[u8; 32]) -> Result<[u8; 32]> {
    ensure!(inv.to == hex::encode(member), "this invite is for another member");
    let eph: [u8; 32] = hex::decode(&inv.ephemeral)?.try_into().map_err(|_| anyhow!("bad ephemeral key"))?;
    let shared = x_of(&(lift_x(&eph)? * scalar(sk)?));
    let key = sha256(&[TAG_INVITE, &shared, &eph, member]);
    let plain = ChaCha20Poly1305::new(&key.into())
        .decrypt(Nonce::from_slice(&[0u8; 12]), hex::decode(&inv.sealed)?.as_slice())
        .map_err(|_| anyhow!("this invite was not sealed to your key"))?;
    plain.try_into().map_err(|_| anyhow!("a room secret is 32 bytes"))
}

// ------------------------------------------------------------- transport

#[repr(C)]
struct OneString {
    s: *const c_char,
}

type CreateFn = unsafe extern "C" fn(*const OneString, extern "C" fn(c_int, *const c_char, *const c_char, *mut c_void), *mut c_void) -> *mut c_void;
type ScalarFn = unsafe extern "C" fn(*mut c_void, extern "C" fn(c_int, *mut c_char, usize, *mut c_void), *mut c_void) -> c_int;
type ReqFn = unsafe extern "C" fn(*mut c_void, extern "C" fn(c_int, *const c_char, *const c_char, *mut c_void), *mut c_void, *const OneString) -> c_int;
type ListenFn = unsafe extern "C" fn(*mut c_void, *const c_char, extern "C" fn(c_int, *const c_char, usize, *mut c_void), *mut c_void) -> u64;
type DestroyFn = unsafe extern "C" fn(*mut c_void) -> c_int;

#[derive(Default)]
struct Shared {
    created: Option<bool>,
    started: Option<bool>,
    reply: Option<bool>,
    error: String,
    inbox: VecDeque<(String, Vec<u8>)>,
}

type SharedBox = Mutex<Shared>;

extern "C" fn on_created(ret: c_int, _ctx: *const c_char, err: *const c_char, ud: *mut c_void) {
    let s = unsafe { &*(ud as *const SharedBox) };
    let mut g = s.lock().unwrap();
    if ret != 0 && !err.is_null() {
        g.error = unsafe { CStr::from_ptr(err) }.to_string_lossy().into();
    }
    g.created = Some(ret == 0);
}

extern "C" fn on_scalar(ret: c_int, msg: *mut c_char, len: usize, ud: *mut c_void) {
    if ret == 3 {
        return; // progress tick
    }
    let s = unsafe { &*(ud as *const SharedBox) };
    let mut g = s.lock().unwrap();
    if ret != 0 && !msg.is_null() {
        g.error = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(msg as *const u8, len) }).into();
    }
    g.started = Some(ret == 0);
}

extern "C" fn on_reply(ret: c_int, _reply: *const c_char, err: *const c_char, ud: *mut c_void) {
    let s = unsafe { &*(ud as *const SharedBox) };
    let mut g = s.lock().unwrap();
    if ret != 0 && !err.is_null() {
        g.error = unsafe { CStr::from_ptr(err) }.to_string_lossy().into();
    }
    g.reply = Some(ret == 0);
}

/// Relay traffic, on the library's event thread: copy and leave. The payload
/// arrives as a JSON array of byte values (`message.payload`).
extern "C" fn on_message(ret: c_int, ev: *const c_char, len: usize, ud: *mut c_void) {
    if ret != 0 || ev.is_null() || len == 0 {
        return;
    }
    let raw = unsafe { std::slice::from_raw_parts(ev as *const u8, len) }.to_vec();
    let Ok(j) = serde_json::from_slice::<serde_json::Value>(&raw) else { return };
    if j["eventType"] != "message_received" {
        return;
    }
    let topic = j["message"]["contentTopic"].as_str().unwrap_or("").to_string();
    let Some(bytes) = j["message"]["payload"].as_array() else { return };
    let payload: Vec<u8> = bytes.iter().filter_map(|b| b.as_u64().map(|v| v as u8)).collect();
    let s = unsafe { &*(ud as *const SharedBox) };
    s.lock().unwrap().inbox.push_back((topic, payload));
}

pub struct Delivery {
    _lib: libloading::Library,
    ctx: *mut c_void,
    shared: Arc<SharedBox>,
    subscribe: ReqFn,
    send: ReqFn,
    destroy: DestroyFn,
    stop: ScalarFn,
}

fn wait(shared: &SharedBox, secs: u64, pick: impl Fn(&Shared) -> Option<bool>) -> Result<()> {
    let t = Instant::now();
    loop {
        if let Some(ok) = pick(&shared.lock().unwrap()) {
            if ok {
                return Ok(());
            }
            bail!("delivery: {}", shared.lock().unwrap().error);
        }
        if t.elapsed() > Duration::from_secs(secs) {
            bail!("delivery: no answer in {secs}s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl Delivery {
    /// Open the library and bring a node up on the `logos.dev` network.
    pub fn up() -> Result<Self> {
        let path = std::env::var("ACCORD_DELIVERY_LIB").context("set ACCORD_DELIVERY_LIB to liblogosdelivery (docs/ROOM.md)")?;
        let lib = unsafe { libloading::Library::new(&path) }.with_context(|| format!("opening {path}"))?;
        let get = |name: &[u8]| -> Result<*const c_void> {
            Ok(*unsafe { lib.get::<*const c_void>(name) }.map_err(|e| anyhow!("{path}: {e}"))?)
        };
        let create: CreateFn = unsafe { std::mem::transmute(get(b"logosdelivery_create_node\0")?) };
        let start: ScalarFn = unsafe { std::mem::transmute(get(b"logosdelivery_start_node\0")?) };
        let stop: ScalarFn = unsafe { std::mem::transmute(get(b"logosdelivery_stop_node\0")?) };
        let subscribe: ReqFn = unsafe { std::mem::transmute(get(b"logosdelivery_subscribe\0")?) };
        let send: ReqFn = unsafe { std::mem::transmute(get(b"logosdelivery_send\0")?) };
        let listen: ListenFn = unsafe { std::mem::transmute(get(b"logosdelivery_add_event_listener\0")?) };
        let destroy: DestroyFn = unsafe { std::mem::transmute(get(b"logosdelivery_destroy\0")?) };
        let shared = Arc::new(SharedBox::default());
        let ud = Arc::as_ptr(&shared) as *mut c_void;
        let config = CString::new(r#"{"mode":"Core","preset":"logos.dev","messagingOverrides":{"log-level":"ERROR"}}"#)?;
        let req = OneString { s: config.as_ptr() };
        let ctx = unsafe { create(&req, on_created, ud) };
        ensure!(!ctx.is_null(), "logosdelivery_create_node returned no context");
        wait(&shared, 60, |s| s.created)?;
        let name = CString::new("onMessageReceived")?;
        ensure!(unsafe { listen(ctx, name.as_ptr(), on_message, ud) } != 0, "the node refused the message listener");
        unsafe { start(ctx, on_scalar, ud) };
        wait(&shared, 180, |s| s.started)?;
        Ok(Self { _lib: lib, ctx, shared, subscribe, send, destroy, stop })
    }

    fn call(&self, f: ReqFn, arg: &str) -> Result<()> {
        self.shared.lock().unwrap().reply = None;
        let c = CString::new(arg)?;
        let req = OneString { s: c.as_ptr() };
        let ud = Arc::as_ptr(&self.shared) as *mut c_void;
        ensure!(unsafe { f(self.ctx, on_reply, ud, &req) } == 0, "delivery refused the call");
        wait(&self.shared, 60, |s| s.reply)
    }

    pub fn subscribe(&self, topic: &str) -> Result<()> {
        self.call(self.subscribe, topic)
    }

    pub fn publish(&self, topic: &str, payload: &[u8]) -> Result<()> {
        use base64::Engine as _;
        let msg = serde_json::json!({"contentTopic": topic, "payload": base64::engine::general_purpose::STANDARD.encode(payload), "ephemeral": false});
        self.call(self.send, &msg.to_string())
    }

    pub fn next(&self) -> Option<(String, Vec<u8>)> {
        self.shared.lock().unwrap().inbox.pop_front()
    }
}

impl Drop for Delivery {
    fn drop(&mut self) {
        let ud = Arc::as_ptr(&self.shared) as *mut c_void;
        self.shared.lock().unwrap().started = None;
        unsafe { (self.stop)(self.ctx, on_scalar, ud) };
        let _ = wait(&self.shared, 20, |s| s.started);
        unsafe { (self.destroy)(self.ctx) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lee::{PrivateKey, PublicKey, Signature};

    fn key(i: u8) -> (PrivateKey, [u8; 32]) {
        let sk = PrivateKey::try_new([i; 32]).unwrap();
        let pk = *PublicKey::new_from_private_key(&sk).value();
        (sk, pk)
    }

    #[test]
    fn an_invite_opens_only_for_its_member() {
        let secret = [7u8; 32];
        for i in 1..20u8 {
            // keys of both y parities come up in this range
            let (sk, pk) = key(i);
            let inv = seal_invite(&pk, &secret, 3).unwrap();
            assert_eq!(open_invite(&inv, sk.value(), &pk).unwrap(), secret);
            let (other_sk, _) = key(i + 100);
            assert!(open_invite(&inv, other_sk.value(), &pk).is_err());
        }
    }

    #[test]
    fn frames_are_sealed_signed_and_bound_to_the_room() {
        let (sk, pk) = key(5);
        let (_, outsider) = key(6);
        let room = Room {
            program: String::new(),
            multisig_id: String::new(),
            config_account: AccountId::new([9; 32]).to_string(),
            epoch: 1,
            secret: hex::encode([1u8; 32]),
        };
        let signer = |m: &[u8; 32]| Ok(Sig { key: pk, sig: Signature::new(&sk, m).value });
        let f = room.seal(&signer, "chat", &serde_json::json!({"text": "approve?"})).unwrap();
        let (who, body) = room.open(&f, &[pk]).unwrap();
        assert_eq!((who, body["body"]["text"].as_str()), (pk, Some("approve?")));
        // a non-member sender is dropped
        assert!(room.open(&f, &[outsider]).is_err());
        // another epoch's key cannot read it, and the topics are unrelated
        let next = Room { epoch: 2, ..room.clone() };
        assert!(next.open(&f, &[pk]).is_err());
        assert_ne!(room.topic().unwrap(), next.topic().unwrap());
        // a flipped byte fails authentication
        let mut g = f.clone();
        *g.last_mut().unwrap() ^= 1;
        assert!(room.open(&g, &[pk]).is_err());
    }
}
