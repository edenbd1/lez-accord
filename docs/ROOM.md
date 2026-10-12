# The coordination room

`accord room` carries approval packages and deliberation between members over
Logos Delivery, end-to-end encrypted. The program is the only authority on
approvals; the room is a transport that carries no trust (DESIGN.md §9).

## Keys and topics

- **Room secret**: 32 random bytes per configuration epoch, made by the member
  who opens the room.
- **Invites**: the secret sealed to each current member's approval key:
  ephemeral secp256k1 ECDH against the member's BIP-340 key (lifted to even
  y; the shared x-coordinate is the same for either parity), SHA-256 with a
  domain tag, ChaCha20-Poly1305. Only that member's approval key opens it.
- **Room topic**: `/accord/1/<SHA-256(tag ‖ secret ‖ epoch)[..16]>/proto`.
  Nothing in it names the multisig, and the topics of two epochs are
  unrelated.
- **Frames**: `nonce ‖ ChaCha20-Poly1305(room key, {sender, sig, text})`.
  `sig` is the sender's approval-key signature over
  `bind_message(configuration, epoch, SHA-256(text))`: every frame is bound
  to the multisig, the epoch and its content, and a receiver drops any frame
  whose sender is not in the configuration it reads from chain.
- **Packages heard are merged**, never trusted: `listen` recomputes each
  package's digest from its action bytes, keeps only approval signatures that
  verify over it, and writes `<digest>.json`, which `accord queue` submits.

## Transport

`liblogosdelivery`, opened at run time from `ACCORD_DELIVERY_LIB`: a
Core-mode node on the `logos.dev` preset, relay subscribe and send, and the
`onMessageReceived` listener. Built from `logos-co/logos-delivery` (the
library target, `make liblogosdelivery`).

## Commands

```bash
export ACCORD_DELIVERY_LIB=/path/to/liblogosdelivery.dylib
accord room open treasury --key alice --out alice.room --invites invites.json [--publish]
accord room join treasury --key bob --out bob.room --invites invites.json   # or hear it on the invite topic
accord room listen carol.room --into inbox --seconds 300 --until-approvals 2 &
accord room post alice.room --key alice --package pay.json
accord room post bob.room --key bob --package pay-approved-by-bob.json
accord room post bob.room --key bob --say "approved; the invoice matches"
accord queue inbox/<digest>.json
```

## What a room participant can do

| Attack | Effect | Defence |
|---|---|---|
| withhold a package or an approval | delays quorum | liveness only; any member can re-post |
| reorder | none | packages are independent and keyed by digest |
| replay | none | the digest binds multisig, epoch and expiry; the queued record is written once |
| inject a forged approval | none | dropped by `listen`, and the program verifies every signature anyway |
| show different members different packages | a member may sign a digest the others did not see | each client decodes the exact bytes it signs (`accord decode`) |
| a non-member posting | none | frames whose sender is not in the configuration are dropped |

## Prototype limits, stated

The RFP names the Logos chat module (MLS group conversations) as the room's
substrate (S.2). This prototype carries the same packages over the Delivery
layer the chat module itself runs on, with its own sealing; moving the
frames into a chat-module group conversation per epoch, with
`add_group_member` driven by the configuration, is grant work. Room history
is relay-only here: a member who is offline misses frames until someone
re-posts; Delivery's Store queries close that.
