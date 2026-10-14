# The public LEZ testnet run (v0.3.1 sequencer, 2026-10-14)

Program `accord_multisig`, header `CehtqLan7giiHfVV5qDD9dtXvVi2h7GMdXM87HD2bbQF`,
ImageID `24a69044e34c2604e1a277dc1958855f0a2d95d65d4b0735052b34a51d323cd9`:
the committed `artifacts/programs/v0.3/accord_multisig.bin`, the reproducible
Docker build that CI rebuilds and compares byte for byte. Deployed once,
immutable, and driven by both runs.

| run | manifest | verifier |
| --- | --- | --- |
| public lifecycle, `scripts/e2e.sh` | [`public.tsv`](public.tsv): 12 applied, 16 refused as expected, 13 state checks, 0 failures ([transcript](public-transcript.txt)) | [`34 checks pass, 0 fail`](public-verify.txt) |
| private vault, `scripts/e2e-private.sh` | [`private.tsv`](private.tsv): 6 applied, 1 refused as expected, 7 state checks, 0 failures ([transcript](private-transcript.txt)) | [`9 checks pass, 0 fail`](private-verify.txt) |

Re-check either from a clean clone, with nothing but `curl` and `python3`:

```bash
./scripts/verify-onchain.sh --manifest evidence/testnet/public.tsv  --rpc https://testnet.lez.logos.co
./scripts/verify-onchain.sh --manifest evidence/testnet/private.tsv --rpc https://testnet.lez.logos.co
```

What the testnet did during the runs, as the manifests record it (`# moved`,
`# late`, `# attempts` lines): it rewrote recent blocks, so five transactions
of the public run sit a few blocks from where the run first saw them; and it
included one refusal, `queue-removed-member`, only after the run had stopped
waiting on it, so the manifest keeps the run's `not-included` verdict and
names the attempt that later landed. Every transaction has an explorer link
in [`links.md`](links.md). On 2026-10-14 the explorer had indexed blocks up to
about 8,400 while these runs were in blocks 10,800 to 11,400, so the links
resolve once it catches up; the verifier asks the sequencer directly.

An earlier deployment the same day (header `9RgveUGv…`, ImageID `24d955ee…`)
ran the same lifecycle with the same results, but from a binary built with
the local toolchain, which a clean rebuild does not reproduce; it was replaced
by this one. Its record is kept in
[`../testnet-2026-10-14-first-binary/`](../testnet-2026-10-14-first-binary/).
