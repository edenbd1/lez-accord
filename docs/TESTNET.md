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
- **The v0.3.1 wallet** uses a different `wallet_config.json` shape (a
  `sequencer` field); a v0.3.1 binary is built at `_lez/v031/wallet` as a
  fallback for funding and balance checks.

The run: `LEE_WALLET_HOME_DIR=<prototype wallet home> PAYER=<funded account> scripts/testnet.sh`,
with run-unique tags (`acc-pub-MMDDHHMM`, `acc-priv-MMDDHHMM`), writing
`evidence/testnet/`.
