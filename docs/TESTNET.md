# The public testnet (v0.3.1 sequencer)

The public LEZ testnet was relaunched on 2026-10-12 with sequencer v0.3.1.
Between v0.3.0 (`db66590a`, what this repository builds against) and v0.3.1
(`1a003bfb`), `lee_core` (the guest-facing crate) is unchanged, so the
committed guest runs there as built. What changed and matters here:

- **The builtin token program is gone** (`lez/programs/token` and its artifact
  are deleted in v0.3.1). Native transfers, the vault, approvals and every
  multisig rule are unaffected; `Asset::Token` actions have no token program
  to call on that chain. The token paths are exercised against the v0.3.0
  token program in `executor-tests`.
- **`getProgramIds`** now returns only the privacy circuit, so the v0.3.0
  `wallet check-health` reports "Missing token program ID from remote". The
  `accord` CLI does not depend on it: read-only commands (`now`, `ids`,
  `show`, `queued`) work against `https://testnet.lez.logos.co`, and a v0.3.0
  wallet home opens.
- **The block header lost its producer key and signature.** A v0.3.0
  `common` cannot decode a v0.3.1 block ("Unexpected variant tag"), which
  public transactions never notice but a private account's sync does: the
  first testnet run's private vault could not be read or spent. The CLI now
  patches in v0.3.1's `common` (`cli/vendor/lez-common`, `src/` copied from
  `1a003bfb`, nothing else changed); local runs and CI use a v0.3.1 sequencer
  to match.
- **The v0.3.1 wallet** uses a different `wallet_config.json` shape (a
  `sequencer` field); a v0.3.1 binary is built at `_lez/v031/wallet` as a
  fallback for funding and balance checks.

The run: `LEE_WALLET_HOME_DIR=<prototype wallet home> PAYER=<funded account> scripts/testnet.sh`,
with run-unique tags (`acc-pub-MMDDHHMM`, `acc-priv-MMDDHHMM`), writing
`evidence/testnet/`.

Two wallet settings matter on the public testnet (`wallet_config.json` of the
wallet home):

- **`gas_limit`: 2,000,000.** Before execution the payer must hold
  `gas_limit × base fee` (8 at the floor) as a reserve, and the unspent part
  is refunded after. The run's multisigs have a threshold of 2, and a queue
  carrying three signatures uses about 0.9M cycles (`docs/BENCHMARKS.md`),
  so 2M leaves room and keeps the reserve at 16M instead of the 80M a 10M
  limit asks for. A larger M needs a larger limit: about 275,000 per approval.
- **Polling**: `seq_poll_timeout` 60s, `seq_tx_poll_max_blocks` 40,
  `seq_poll_max_retries` 30. Testnet blocks come every 15 to 60 s, and the
  defaults (12 s, 5 blocks) give up on a deploy segment that is still on its
  way.

The testnet drops some transactions, includes others late, and has rewritten
recent blocks. Two things in the run account for that:

- **A public transaction not included within 180 s is sent again**, up to three
  attempts (`cli/src/main.rs`, `send`). Each attempt is signed with the payer's
  current nonce, so while an earlier attempt is only late they share a nonce
  and at most one lands; every earlier attempt is looked for before the next
  is sent. Locally no transaction is ever dropped, so a `not-included` verdict
  on the testnet means the sequencer lost all three.
- **At the end of a run, `scripts/reresolve.py` re-reads every transaction's
  block** and rewrites the manifest's block column where a rewrite moved it,
  with a `# moved` line for each; a transaction recorded `not-included` that
  landed afterwards gets a `# late` line, and its verdict is left as the run
  saw it.
