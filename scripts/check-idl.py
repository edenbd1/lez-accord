#!/usr/bin/env python3
"""Fail if idl/accord_multisig.idl.json has drifted from the Rust types.

SPEL has no LEZ v0.3 support, so the IDL is written by hand, and a
hand-written IDL is only worth anything if something notices when it stops
describing the program.

The core crate's `idl_vectors` example borsh-encodes one sample of every
instruction, every stored account and every event, and prints the values it
used, all distinct. This script decodes each sample using only the IDL and
requires the same values back with every byte consumed. It also compares the
discriminants, the event selectors, the refusal codes, the roles and the
timestamp window. A field added, dropped, retyped or reordered on one side
only fails here.

Usage: scripts/check-idl.py            (runs the example itself)
       scripts/check-idl.py vectors.json
"""
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
IDL = json.loads((ROOT / "idl/accord_multisig.idl.json").read_text())
TYPES = {t["name"]: t["type"] for t in IDL["types"]}
INTS = {"u8": 1, "u16": 2, "u32": 4, "u64": 8, "u128": 16}


class Reader:
    def __init__(self, data):
        self.b = data
        self.i = 0

    def take(self, n):
        if self.i + n > len(self.b):
            raise ValueError("ran out of bytes")
        out = self.b[self.i:self.i + n]
        self.i += n
        return out


def u32(r):
    return int.from_bytes(r.take(4), "little")


def decode(ty, r):
    if isinstance(ty, str):
        if ty in INTS:
            v = int.from_bytes(r.take(INTS[ty]), "little")
            return str(v) if ty == "u128" else v
        if ty == "account_id":
            return r.take(32).hex()
        if ty == "bytes":
            return r.take(u32(r)).hex()
        if ty == "string":
            return r.take(u32(r)).decode()
        raise ValueError(f"unknown type {ty}")
    if "array" in ty:
        inner, n = ty["array"]
        if inner == "u8":
            return r.take(n).hex()
        return [decode(inner, r) for _ in range(n)]
    if "vec" in ty:
        return [decode(ty["vec"], r) for _ in range(u32(r))]
    if "defined" in ty:
        return decode_defined(TYPES[ty["defined"]], r)
    raise ValueError(f"unknown type {ty}")


def decode_fields(fields, r):
    return {f["name"]: decode(f["type"], r) for f in fields}


def decode_defined(t, r):
    if t["kind"] == "struct":
        return decode_fields(t["fields"], r)
    if t["kind"] == "enum":
        tag = r.take(1)[0]
        if tag >= len(t["variants"]):
            raise ValueError(f"enum tag {tag} out of range")
        var = t["variants"][tag]
        return {var["name"]: decode_fields(var["fields"], r)}
    raise ValueError(t)


def whole(fn, hexstr, name):
    r = Reader(bytes.fromhex(hexstr))
    out = fn(r)
    if r.i != len(r.b):
        raise ValueError(f"{name}: {len(r.b) - r.i} bytes left over")
    return out


def main():
    if len(sys.argv) > 1:
        vec = json.loads(pathlib.Path(sys.argv[1]).read_text())
    else:
        out = subprocess.run(
            ["cargo", "run", "-q", "--example", "idl_vectors", "-p", "accord-core",
             "--manifest-path", str(ROOT / "programs/accord/Cargo.toml")],
            check=True, capture_output=True, text=True)
        vec = json.loads(out.stdout)

    bad = []
    by_name = {i["name"]: i for i in IDL["instructions"]}
    if [i["name"] for i in vec["instructions"]] != [i["name"] for i in IDL["instructions"]]:
        bad.append("instruction list or order differs")
    for k, sample in enumerate(vec["instructions"]):
        spec = by_name.get(sample["name"])
        if spec is None:
            bad.append(f"{sample['name']}: missing from the IDL")
            continue
        if spec["discriminant"] != k:
            bad.append(f"{sample['name']}: IDL discriminant {spec['discriminant']}, Rust {k}")

        def ix(r, spec=spec):
            tag = r.take(1)[0]
            if tag != spec["discriminant"]:
                raise ValueError(f"discriminant {tag}, IDL says {spec['discriminant']}")
            return decode_fields(spec["args"], r)
        try:
            got = whole(ix, sample["hex"], sample["name"])
            if got != sample["values"]:
                bad.append(f"{sample['name']}: decodes to {got}, expected {sample['values']}")
        except ValueError as e:
            bad.append(f"{sample['name']}: {e}")
        win = spec.get("timestamp_window")
        if win and win != f"[at, at + {vec['window_ms']})":
            bad.append(f"{sample['name']}: window {win}, program uses {vec['window_ms']} ms")

    accounts = {a["name"]: a["type"] for a in IDL["accounts"]}
    for sample in vec["accounts"]:
        t = accounts.get(sample["name"])
        if t is None:
            bad.append(f"account {sample['name']}: missing from the IDL")
            continue
        try:
            got = whole(lambda r, t=t: decode_defined(t, r), sample["hex"], sample["name"])
            if got != sample["values"]:
                bad.append(f"account {sample['name']}: decodes to {got}")
        except ValueError as e:
            bad.append(f"account {sample['name']}: {e}")

    ev = {e["name"]: e for e in IDL["events"]}
    if [e["name"] for e in vec["events"]] != [e["name"] for e in IDL["events"]]:
        bad.append("event list or order differs")
    for sample in vec["events"]:
        spec = ev.get(sample["name"])
        if spec is None:
            bad.append(f"event {sample['name']}: missing from the IDL")
            continue
        try:
            got = whole(lambda r, s=spec: decode_fields(s["fields"], r), sample["hex"], sample["name"])
            if got != sample["values"]:
                bad.append(f"event {sample['name']}: decodes to {got}")
        except ValueError as e:
            bad.append(f"event {sample['name']}: {e}")
    for s in vec["selectors"]:
        if ev.get(s["name"], {}).get("selector") != s["selector"]:
            bad.append(f"event {s['name']}: selector is {s['selector']}")

    if vec["errors"] != IDL["errors"]:
        bad.append("error codes differ")
    if vec["roles"] != IDL["roles"]:
        bad.append("role bits differ")

    if bad:
        print("IDL drift:")
        for b in bad:
            print("  " + b)
        sys.exit(1)
    n = len(vec["instructions"]) + len(vec["accounts"]) + len(vec["events"])
    print(f"IDL matches the program: {n} borsh vectors decoded exactly, "
          f"{len(vec['selectors'])} selectors and {len(vec['errors'])} codes agree")


if __name__ == "__main__":
    main()
