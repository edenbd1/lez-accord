#!/usr/bin/env bash
# Build the accord_multisig guest and pack it into the R0BF program binary LEZ
# loads, at artifacts/programs/v0.3/accord_multisig.bin.
#
#   scripts/build-guest.sh          local build with the RISC Zero toolchain (rzup)
#   DOCKER=1 scripts/build-guest.sh RISC Zero's pinned builder, reproducible
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=artifacts/programs/v0.3/accord_multisig.bin
mkdir -p "$(dirname "$OUT")"
if [ "${DOCKER:-0}" = 1 ]; then
  RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1 cargo risczero build --manifest-path programs/accord/Cargo.toml
  cp programs/accord/target/riscv32im-risc0-zkvm-elf/docker/accord_multisig.bin "$OUT"
else
  TC="${RISC0_RUST:-$(ls -d "$HOME"/.risc0/toolchains/v1.*-rust-* | sort -V | tail -1)/bin}"
  FLAGS=$(printf '%s\x1f' -C passes=lower-atomic -C link-arg=-Ttext=0x00200800 -C link-arg=--fatal-warnings \
    -C panic=abort --cfg 'getrandom_backend="custom"')
  FLAGS=${FLAGS%$'\x1f'}
  (cd programs/accord && RUSTC="$TC/rustc" CARGO_ENCODED_RUSTFLAGS="$FLAGS" CC=/no_cc RISC0_FEATURE_bigint2= \
    CARGO_TARGET_DIR=target/local-guest cargo build --release --target riscv32im-risc0-zkvm-elf --bin accord_multisig)
  cargo run -q --release --manifest-path executor-tests/Cargo.toml --bin pack -- \
    programs/accord/target/local-guest/riscv32im-risc0-zkvm-elf/release/accord_multisig "$OUT"
fi
ls -l "$OUT"
