# A local LEZ v0.3 sequencer

```bash
git clone https://github.com/logos-blockchain/logos-execution-zone lez && cd lez
git checkout db66590ab821a4e142c211017a3866d007f6fa77
cargo build --release -p wallet                      # the v0.3.0 wallet the CLI's config format matches
git checkout 1a003bfbfb89fcf987f62975cd08a401169e1b8c
cargo build --release --features standalone -p sequencer_service   # v0.3.1, as on the testnet
mkdir -p /tmp/seq && cp lez/sequencer/service/configs/debug/sequencer_config.json /tmp/seq/
cd /tmp/seq && RUST_LOG=info sequencer_service sequencer_config.json --listen-address 127.0.0.1 &   # port 3040

export LEE_WALLET_HOME_DIR=/tmp/wallet-local
wallet check-health        # creates wallet_config.json
# point it at the local sequencer, and allow the 10M gas a large queue needs
sed -i.bak -e 's|https://testnet.lez.logos.co/|http://127.0.0.1:3040/|' -e 's/"gas_limit": 2000000/"gas_limit": 10000000/' \
  "$LEE_WALLET_HOME_DIR/wallet_config.json"
# a funded genesis key of the debug configuration (a public test key)
wallet account import public --private-key 10a26a9aec7d34b82364eeae45c5294dbb0a764b000b94eeb9b58511dc487c4d
```

Blocks are produced every 15 s, so `scripts/e2e.sh` takes about 15 minutes.
