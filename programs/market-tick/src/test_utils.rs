//! Host-side builder for the aligned SBF program input, byte-for-byte the
//! layout produced by the runtime's `serialize_parameters_aligned`.

use std::{vec, vec::Vec};

use pinocchio::{account_info::MAX_PERMITTED_DATA_INCREASE, pubkey::Pubkey};

const NON_DUP_MARKER: u8 = u8::MAX;
const ACCOUNT_HEADER_LEN: usize = 88;

#[derive(Clone)]
pub struct TestAccount {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub lamports: u64,
    pub data: Vec<u8>,
    pub is_signer: bool,
    pub is_writable: bool,
    pub executable: bool,
}

impl TestAccount {
    pub fn new(key: Pubkey, owner: Pubkey, data: Vec<u8>) -> Self {
        Self {
            key,
            owner,
            lamports: 1,
            data,
            is_signer: false,
            is_writable: false,
            executable: false,
        }
    }

    pub fn signer(mut self) -> Self {
        self.is_signer = true;
        self
    }

    pub fn writable(mut self) -> Self {
        self.is_writable = true;
        self
    }

    pub fn lamports(mut self, lamports: u64) -> Self {
        self.lamports = lamports;
        self
    }
}

#[derive(Clone)]
pub enum InputAccount {
    Account(TestAccount),
    Duplicate(u8),
}

/// Serialized program input backed by `u64` words so the buffer start is
/// 8-byte aligned, as the runtime's input region is.
pub struct SerializedInput {
    words: Vec<u64>,
    len: usize,
    data_offsets: Vec<Option<usize>>,
}

impl SerializedInput {
    pub fn new(accounts: &[InputAccount], instruction_data: &[u8], program_id: &Pubkey) -> Self {
        let mut bytes: Vec<u8> = Vec::new();
        let mut data_offsets = Vec::with_capacity(accounts.len());
        bytes.extend_from_slice(&(accounts.len() as u64).to_le_bytes());

        for account in accounts {
            match account {
                InputAccount::Duplicate(index) => {
                    bytes.push(*index);
                    bytes.extend_from_slice(&[0; 7]);
                    data_offsets.push(None);
                }
                InputAccount::Account(account) => {
                    let header_start = bytes.len();
                    bytes.push(NON_DUP_MARKER);
                    bytes.push(account.is_signer as u8);
                    bytes.push(account.is_writable as u8);
                    bytes.push(account.executable as u8);
                    bytes.extend_from_slice(&[0; 4]);
                    bytes.extend_from_slice(&account.key);
                    bytes.extend_from_slice(&account.owner);
                    bytes.extend_from_slice(&account.lamports.to_le_bytes());
                    bytes.extend_from_slice(&(account.data.len() as u64).to_le_bytes());
                    assert_eq!(bytes.len() - header_start, ACCOUNT_HEADER_LEN);
                    data_offsets.push(Some(bytes.len()));
                    bytes.extend_from_slice(&account.data);
                    bytes.resize(bytes.len() + MAX_PERMITTED_DATA_INCREASE, 0);
                    bytes.resize(bytes.len().next_multiple_of(8), 0);
                    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
                }
            }
        }

        bytes.extend_from_slice(&(instruction_data.len() as u64).to_le_bytes());
        bytes.extend_from_slice(instruction_data);
        bytes.extend_from_slice(program_id);

        let len = bytes.len();
        let mut words = vec![0u64; len.div_ceil(8)];
        // SAFETY: `words` spans at least `len` bytes; u64 has no invalid bit
        // patterns and is at least as aligned as u8.
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), words.as_mut_ptr() as *mut u8, len);
        }

        Self {
            words,
            len,
            data_offsets,
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.words.as_mut_ptr() as *mut u8
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `words` owns at least `len` initialized bytes.
        unsafe { core::slice::from_raw_parts(self.words.as_ptr() as *const u8, self.len) }
    }

    pub fn account_data(&self, index: usize, len: usize) -> &[u8] {
        let offset = self.data_offsets[index].expect("duplicate account has no data");
        &self.bytes()[offset..offset + len]
    }
}
