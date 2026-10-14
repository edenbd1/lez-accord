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

`scripts/e2e-room.sh` drives this transport end to end; its record,
[`evidence/v03/local-room.tsv`](../evidence/v03/local-room.tsv), dates from
2026-10-12 and the earlier, non-reproducible build of the guest. On 2026-10-14
the `liblogosdelivery` build at hand (August 2026) no longer delivered to the
listening node on `logos.dev`, so that run was not repeated on the current
binary; the room on the Logos chat module (below,
[`evidence/v03/local-chat.tsv`](../evidence/v03/local-chat.tsv)) is the path
run end to end on it.

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

## On the Logos chat module

`scripts/e2e-chat.sh` runs the room on the chat module itself: each of three
members runs its own headless Logos Core (`logosctl` 0.3.2, its own session
directory) with `chat_module` 0.3.0 and its `delivery_module` installed from
the official module catalog, and its own approval key in its own key
directory. Alice opens a group conversation for the multisig's epoch
(`create_group_conversation`) and adds Bob and Carol (`add_group_member`);
the package then travels only through that MLS group:

1. Alice proposes a transfer and posts the package to the group.
2. Bob and Carol each take it from the group, decode it from its bytes
   (`accord decode`: the digest is recomputed from what they will sign),
   approve it and post it back. Bob also posts a line of deliberation, and a
   copy carrying a forged approval.
3. Alice merges what she heard (`accord merge`): the forged approval is
   dropped because its signature does not verify against the digest the
   package's own bytes hash to, the two real ones merge, and she queues and
   executes it on chain.

The run on a local v0.3 sequencer with the logos.dev Delivery fleet:
[`evidence/v03/local-chat-transcript.txt`](../evidence/v03/local-chat-transcript.txt),
19 checks, 0 failures; its manifest
[`evidence/v03/local-chat.tsv`](../evidence/v03/local-chat.tsv) re-checked by
`scripts/verify-onchain.sh` ([output](../evidence/v03/local-chat-verify.txt)).
The same run with the logos.test preset never delivered the group welcome
within ten minutes (the logos.test fleet has been unreliable since the
testnet v0.3 switch), so the script defaults to logos.dev.

The Basecamp mini-app's backend (`app/src/accord_backend.cpp`) starts the
same chat module and decodes packages posted to a group next to the
proposal list; that path has not yet been run end to end with three members
the way the script above is.

## Prototype limits, stated

- The group is opened and grown by hand (`create_group_conversation`,
  `add_group_member`), not yet driven by the configuration: a configuration
  change does not yet open the next epoch's group or leave out a removed
  member. That, and the binding statement tying a chat identity to an
  approval key, is grant work (M3).
- Chat-module identities are per install, and nothing yet proves to the
  others that a chat address belongs to a given member; the program does
  not need it (it verifies every approval), but the room's view of who said
  what does.
- History: a member who joins later sees what the group carries from then
  on; Store-backed history for offline members is grant work.
- `accord room` (above) remains as the transport-agnostic fallback over
  Delivery with its own sealing.
