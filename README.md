# Market Tick (Pinocchio)

A [Pinocchio](https://github.com/anza-xyz/pinocchio) port of
[firedancer-io/market-tick](https://github.com/firedancer-io/market-tick)
(`af3ea0a`), a Solana program that keeps a sub-slot clock which increments at a
regular interval throughout a slot.

The on-chain ABI is unchanged: program ID, PDA, seeds, the 88-byte V1 account
layout, instruction tags and data, account order, and the custom error numbers
`6000..=6006`. Upstream's `market-tick-abi` crate is vendored unmodified in
`crates/market-tick-abi`, and `idls/market_tick.json` still describes the
program.

## Anchor vs Pinocchio

Measured by `programs/market-tick/tests/litesvm.rs`, which runs upstream's
integration scenario against both binaries in LiteSVM:

|                        | Anchor 1.1.2 | Pinocchio |
|------------------------|-------------:|----------:|
| binary size (bytes)    |      139,648 |    11,144 |
| initialize (fresh)     |        5,986 |     1,536 |
| initialize (prefunded) |        9,173 |     3,814 |
| increment (first ever) |        1,836 |       242 |
| increment (same slot)  |        1,851 |       262 |
| increment (new slot)   |        1,838 |       242 |

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
  it with a bit-exact integer emulation of the f64 formula, which drops the
  binary to 7,888 bytes.
- **System CPIs.** All system CPIs go through one non-inlined
  `invoke_signed_unchecked` call site, with hand-encoded instruction data.

## Build and test

```sh
cargo build-sbf --manifest-path programs/market-tick/Cargo.toml   # target/deploy/market_tick.so

cargo test --workspace                                            # unit, ABI and LiteSVM tests
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

## Instructions

All integers are little-endian. Trailing instruction data and trailing
accounts are ignored.

| Tag | Name | Data | Accounts |
|-----|------|------|----------|
| `0` | Initialize V1 | `u8 tag` | `payer (signer, writable)`, `pda (writable)`, `system_program` |
| `1` | Increment V1 | `u8 tag, u64 slot, i64 timestamp_ns, u64 target_market_tick_interval_ns` | `pda (writable)`, `signer (signer)` |

Increment semantics and the trust model are unchanged. See the
[upstream README](https://github.com/firedancer-io/market-tick#instructions).
