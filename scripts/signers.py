#!/usr/bin/env python3
"""Every distinct signer of the transactions a manifest lists, read from the chain.

Pr.1: a multisig transaction must not carry a member's LEZ account. Accord's
approvals are signatures over digests inside the instruction, so the only
LEZ signer of every transaction should be the fee payer.

usage: signers.py <rpc> <manifest>   (prints the signer account ids, space-separated)
"""
import hashlib, json, sys, urllib.request

B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
def b58(b):
    n = int.from_bytes(b, "big"); s = ""
    while n:
        n, r = divmod(n, 58); s = B58[r] + s
    return "1" * (len(b) - len(b.lstrip(b"\0"))) + s

def call(rpc, method, param):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": [param]}).encode()
    req = urllib.request.Request(rpc, body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=25) as r:
        return json.load(r)

def keys(o):
    """Public keys found in a transaction's witness set, wherever the JSON puts them."""
    if isinstance(o, dict):
        for k, v in o.items():
            if k in ("signatures_and_public_keys",) and isinstance(v, list):
                for pair in v:
                    yield pair[1]
            else:
                yield from keys(v)
    elif isinstance(o, list):
        for v in o:
            yield from keys(v)

def account_of(pk):
    # LEZ: AccountId = SHA-256("/LEE/v0.3/AccountId/Public/" padded to 32 || x-only key)
    raw = bytes(pk) if isinstance(pk, list) else bytes.fromhex(pk)
    prefix = b"/LEE/v0.3/AccountId/Public/".ljust(32, b"\0")
    return b58(hashlib.sha256(prefix + raw).digest())

rpc, manifest = sys.argv[1], sys.argv[2]
seen = set()
for line in open(manifest):
    if line.startswith("#") or not line.strip():
        continue
    tx = line.rstrip("\n").split("\t")[4]
    if tx in ("", "-"):
        continue
    res = call(rpc, "getTransaction", tx).get("result")
    if not res:
        continue
    for pk in keys(res[0] if isinstance(res, list) else res):
        seen.add(account_of(pk))
print(" ".join(sorted(seen)))
