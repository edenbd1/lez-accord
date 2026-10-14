#!/usr/bin/env python3
"""Markdown explorer links for every transaction of the given manifests.

usage: links.py <explorer-url> <manifest>...   (writes markdown to stdout)
"""
import sys
explorer, *manifests = sys.argv[1:]
print("# Testnet run: every transaction on the explorer\n")
for m in manifests:
    try: lines = open(m).read().splitlines()
    except FileNotFoundError: continue
    print(f"## `{m}`\n")
    for l in lines:
        if l.startswith("# program "):
            header = l.split()[2]
            print(f"Program header: [`{header}`]({explorer}/account/{header}), ImageID `{l.split()[3]}`\n")
    print("| step | expected | verdict | block | transaction |\n|---|---|---|---:|---|")
    for l in lines:
        if l.startswith("#") or not l.strip(): continue
        f = l.split("\t")
        if len(f) < 5: continue
        tx = f[4]
        link = f"[`{tx[:16]}…`]({explorer}/transaction/{tx})" if len(tx) == 64 and f[2] in ("applied", "reverted") else f"`{tx}`"
        print(f"| {f[0]} | {f[1]} | {f[2]} | {f[3]} | {link} |")
    print()
