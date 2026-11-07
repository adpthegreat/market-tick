# Market Tick (Pinocchio)

A [Pinocchio](https://github.com/anza-xyz/pinocchio) port of
[firedancer-io/market-tick](https://github.com/firedancer-io/market-tick)
(`af3ea0a`), a Solana program that keeps a sub-slot clock which increments at a
regular interval throughout a slot.

The PDA seeds, the 88-byte V1 account layout, instruction tags and data,
account order, and the custom error numbers `6000..=6006` match upstream.
Upstream's `market-tick-abi` crate is vendored unmodified in
`crates/market-tick-abi`. `idls/market_tick.json` is generated with Shank from
`programs/market-tick/src/idl.rs`.

## Why a sub-slot clock

Solana's `Clock` sysvar moves once per slot (~200 ms). Every transaction in a
slot sees the same time.

That matters for proprietary AMMs (propAMMs), which quote from prices their
operator pushes on chain. Without a finer clock, a propAMM knows the slot of
its last price update but not whether that update is 50 ms or 150 ms old.
With Market Tick it can read how far into the slot the current tick is,
compare that with its last update, and adjust its output for staleness.

Adoption, per a discussion on X
([thread](https://x.com/atarashi/status/2118534297779126442)):

- **Who writes it.** Only BAM and Harmonic block producers write ticks.
- **Coverage.** Most epochs have ticks in roughly 70% of blocks. An epoch-1076
  reading of 50% was an anomaly.
- **Who reads it.** BisonFi actively uses it for quoting. Tessera passes the
  account into swaps, but its pools don't use it for quoting yet.

### Why only some block producers write it

The first `increment` in a slot claims it. The same signer then sends further
ticks at a fixed interval, each stamped with its local clock. Inside the block
this looks like:

```
[tick 0 @ +0ms] swaps... [tick 1 @ +50ms] swaps... [tick 2 @ +100ms] swaps...
```

That only works if the writer controls transaction order in the block.
Block-building setups such as BAM and Harmonic can put tick 0 first and
interleave later ticks with user transactions as the block is built. A leader
without that machinery would have to send ticks like any other user, with no
say over where they land. Slots from leaders that don't tick leave the account
on an older slot, and consumers treat the clock as unavailable for that slot.

The account holds only the latest tick. Coverage and updates-per-slot figures
come from scanning blocks for `increment` transactions and grouping them by
slot.

### How propAMMs read the market tick

On chain nothing subscribes to the tick. The propAMM reads the account during
the swap:

1. **The swap includes the account.** The swap instruction takes the Market
   Tick PDA as a read-only account. Programs can only read accounts passed to
   them.
2. **Validate it.**
   - The key is the canonical PDA and the owner is the Market Tick program.
   - It decodes as `MarketTickV1`.
   - `slot` equals the current `Clock` slot, so this slot has been ticked.
   - `signer` is on the AMM's allowlist.
3. **Measure staleness.** Compare the current tick (`sequence`, or
   `timestamp_ns - first_timestamp_ns`) with the tick recorded at the AMM's
   last price update.
4. **Adjust the quote.** For example, widen the spread or reduce output as the
   price ages, or refuse to quote past a limit. If the tick is unavailable,
   fall back to slot-level staleness.

Passing the account in is not enough; the pricing logic has to use it. Off
chain, an operator may subscribe to the account over RPC to monitor ticks, but
on-chain decisions read the account at swap time.

## Anchor vs Pinocchio

Measured by `programs/market-tick/tests/litesvm.rs`, which runs upstream's
integration scenario against both binaries in LiteSVM:

|                        | Anchor 1.1.2 | Pinocchio |
|------------------------|-------------:|----------:|
| binary size (bytes)    |      139,648 |    12,824 |
| initialize (fresh)     |        5,986 |     1,526 |
| initialize (prefunded) |        9,173 |     4,032 |
| increment (first ever) |        1,836 |       244 |
| increment (same slot)  |        1,851 |       264 |
| increment (new slot)   |        1,838 |       244 |

Compute units per instruction. Both binaries were built with `cargo build-sbf`
(platform-tools v1.57). The Anchor binary is checked in as
`programs/market-tick/tests/fixtures/market_tick_anchor.so`.

Where the savings come from:

- **Lazy entrypoint.** The input is walked once. Instruction data is read in
  place and trailing accounts are skipped without building `AccountInfo`s.
- **Zero-copy state.** `MarketTickV1State` is a `#[repr(C)]` view whose field
  offsets match the ABI. It is checked at compile time against
  `MarketTickV1::LEN` and the documented offsets. Increments mutate the
  account bytes directly; there is no decode/encode round trip.
- **Unaligned instruction reads.** The fields follow a one-byte tag, so they
  are read with `read_unaligned`.
- **Constant PDA and bump.** No `find_program_address` on chain, and a
  32-byte key comparison instead of a seeds constraint.
- **Raw clock syscall.** `increment` reads the clock straight into a stack
  slot.
- **Rent.** The rent-exempt minimum uses Pinocchio's
  `Rent::get()?.minimum_balance(..)`. The `rent-integer-math` branch replaces
  it with a bit-exact integer emulation of the f64 formula, which removes the
  soft-float code.
- **System CPIs.** Account creation uses the `pinocchio-system` instruction
  builders.

## Build and test

```sh
cargo build-sbf --manifest-path programs/market-tick/Cargo.toml   # target/deploy/market_tick.so

cargo test --workspace                                            # unit, ABI and LiteSVM tests
shank idl --crate-root programs/market-tick --out-dir idls         # regenerate idls/market_tick.json
cargo test -p market-tick --test litesvm -- --nocapture           # print the comparison table

rustup component add miri --toolchain nightly
MIRIFLAGS="-Zmiri-strict-provenance" cargo +nightly miri test -p market-tick --lib
MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo +nightly miri test -p market-tick --lib
```

The unit tests drive every `unsafe` path through Miri. `test_utils.rs` builds
a byte-exact copy of the runtime's aligned input buffer. The tests then run the
real parser over it:

- trailing and duplicate accounts;
- account data of odd lengths, which shifts every later offset;
- short, trailing and unknown instruction data;
- a randomized input sweep.

They also cast the account in place and apply increments.

## Account ABI v1

| off | size | field | type |
|-----|------|-------|------|
| 0 | 8 | discriminator = `MRKTKV01` | bytes |
| 8 | 32 | signer | Pubkey |
| 40 | 8 | slot | u64 |
| 48 | 8 | first_timestamp_ns | i64 |
| 56 | 8 | timestamp_ns | i64 |
| 64 | 8 | sequence | u64 |
| 72 | 8 | target_market_tick_interval_ns | u64 |
| 80 | 8 | observed_market_tick_interval_ns | u64 |

Program ID `DHEk6e7M37FXE7g8bThazZ3PuaR87MABb1TFJzWiREAD`. The V1 PDA is
`CGdiAQSwWMQfXGTXCzdqpY6s8hL3pVMqd5gj2BuFst2m`, from seeds
`[b"market_tick", b"v1"]` with bump 254.

## Instructions

All integers are little-endian. Trailing instruction data and trailing
accounts are ignored.

| Tag | Name | Data | Accounts |
|-----|------|------|----------|
| `0` | Initialize V1 | `u8 tag` | `payer (signer, writable)`, `pda (writable)`, `system_program` |
| `1` | Increment V1 | `u8 tag, u64 slot, i64 timestamp_ns, u64 target_market_tick_interval_ns` | `pda (writable)`, `signer (signer)` |

Increment semantics and the trust model are unchanged. See the
[upstream README](https://github.com/firedancer-io/market-tick#instructions).

## References

- **Market Tick (upstream).** Original Anchor program by Firedancer:
  [firedancer-io/market-tick](https://github.com/firedancer-io/market-tick)
- **Adoption thread.** Discussion of propAMM usage and block coverage:
  [x.com/atarashi/status/2118534297779126442](https://x.com/atarashi/status/2118534297779126442)
- **Pinocchio.** Zero-dependency Solana program library used here:
  [anza-xyz/pinocchio](https://github.com/anza-xyz/pinocchio)
- **BAM (Block Assembly Marketplace).** Jito's block-building system:
  [Helius overview](https://www.helius.dev/blog/block-assembly-marketplace-bam),
  [mainnet launch (Blockworks)](https://blockworks.co/news/jito-bam-solana-mainnet)
- **Harmonic.** Open block-building marketplace for Solana validators:
  [harmonic.gg](https://harmonic.gg/), [docs](https://docs.harmonic.gg/)
- **PropAMMs.** Proprietary AMMs that quote from operator-pushed prices:
  [Helius: Solana's proprietary AMM revolution](https://www.helius.dev/blog/solanas-proprietary-amm-revolution),
  [LimeChain: Making sense of prop AMMs](https://limechain.tech/blog/making-sense-of-prop-amms-on-solana),
  [DefiLlama: prop AMMs on Solana](https://defillama.com/protocols/prop-amm/solana)
- **BisonFi.** PropAMM that uses Market Tick for quoting:
  [DefiLlama](https://defillama.com/protocol/bisonfi)
- **Tessera V.** PropAMM that passes the Market Tick account into swaps:
  [DefiLlama](https://defillama.com/protocol/tessera-v)
