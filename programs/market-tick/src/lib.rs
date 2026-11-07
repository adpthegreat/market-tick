#![no_std]

#[cfg(target_endian = "big")]
compile_error!("market-tick reinterprets little-endian account and instruction bytes in place");

#[cfg(test)]
extern crate std;

#[cfg(not(feature = "no-entrypoint"))]
mod entrypoint;
pub mod error;
pub mod instructions;
pub mod state;

#[cfg(test)]
mod test_utils;

pub use market_tick_abi as abi;

use pinocchio::pubkey::Pubkey;

pinocchio_pubkey::declare_id!("tickUcsEQegChaAuo9VYQQztB4ZGApY6ZT4FkULWY6N");

/// Canonical V1 PDA, `find_program_address([b"market_tick", b"v1"], ID)`.
pub const PDA_V1: Pubkey =
    pinocchio_pubkey::pubkey!("4cG31VNF9TzFinNc7BmnjhFvGjxkY3sCETVMtMgbrhPs");

/// Canonical bump of [`PDA_V1`].
pub const PDA_V1_BUMP: u8 = 255;
