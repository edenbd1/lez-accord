> **Superseded.** This deployment (`9RgveUGv…`, ImageID `24d955ee…`) ran from a binary a clean rebuild does not reproduce; the current one is [`../testnet/`](../testnet/README.md).

# The public LEZ testnet run (v0.3.1 sequencer, 2026-10-14)

Program `accord_multisig`, header `9RgveUGvnm91JPJM8a8mPkScxenQrDNmjzagj8EMv6vr`,
ImageID `24d955ee85544249ebe79f3c9bab5482158898ef1753c655ebd59d030e27762a`
(the committed `artifacts/programs/v0.3/accord_multisig.bin`), deployed once
and driven by both runs.

| run | manifest | verifier |
| --- | --- | --- |
| public lifecycle, `scripts/e2e.sh` | [`public.tsv`](public.tsv): 12 applied, 16 refused as expected, 13 state checks, 0 failures ([transcript](public-transcript.txt)) | [`34 checks pass, 0 fail`](public-verify.txt); 3 transactions since moved a few blocks by a testnet block rewrite, reported by the verifier |
| private vault, `scripts/e2e-private.sh` | [`private.tsv`](private.tsv): 6 applied, 1 refused as expected, 7 state checks, 0 failures ([transcript](private-transcript.txt)) | [`9 checks pass, 0 fail`](private-verify.txt) |

Re-check either from a clean clone, with nothing but `curl` and `python3`:

```bash
./scripts/verify-onchain.sh --manifest evidence/testnet/public.tsv  --rpc https://testnet.lez.logos.co
./scripts/verify-onchain.sh --manifest evidence/testnet/private.tsv --rpc https://testnet.lez.logos.co
```

Every transaction has an explorer link in [`links.md`](links.md). On
2026-10-14 the explorer had indexed blocks up to about 7,585 (02:14 UTC)
while the sequencer was past 10,400, so these links resolve once it catches
up; the verifier asks the sequencer directly and does not wait for it.

The private run is the second attempt of the day. The first had every expected
effect on chain but two transactions included after the run stopped waiting
on them, which is why the CLI now records every attempt and waits longer
(`docs/TESTNET.md`).
