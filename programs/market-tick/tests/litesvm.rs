//! Runs the upstream integration scenario against both the Pinocchio build
//! (`target/deploy/market_tick.so`) and the upstream Anchor build
//! (`tests/fixtures/market_tick_anchor.so`, firedancer-io/market-tick@af3ea0a),
//! asserting identical observable behaviour and comparing cost.
//!
//! Requires `cargo build-sbf --manifest-path programs/market-tick/Cargo.toml`.

#![allow(clippy::result_large_err)]

use std::path::PathBuf;

use litesvm::{types::TransactionResult, LiteSVM};
use market_tick_abi::{error_code, MarketTickV1, PDA_SEED_MARKET_TICK, PDA_SEED_VERSION_V1};
use solana_account::Account;
use solana_address::Address;
use solana_instruction::{AccountMeta, Instruction};
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;

const START_SLOT: u64 = 1_000;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn pinocchio_so() -> PathBuf {
    manifest_dir().join("../../target/deploy/market_tick.so")
}

fn anchor_so() -> PathBuf {
    manifest_dir().join("tests/fixtures/market_tick_anchor.so")
}

fn program_id() -> Address {
    Address::from(market_tick::ID)
}

fn pda() -> Address {
    Address::find_program_address(&[PDA_SEED_MARKET_TICK, PDA_SEED_VERSION_V1], &program_id()).0
}

fn system_program() -> Address {
    Address::default()
}

fn initialize_ix_with(payer: Address, tick: Address, system: Address) -> Instruction {
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(tick, false),
            AccountMeta::new_readonly(system, false),
        ],
        data: vec![0],
    }
}

fn initialize_ix(payer: Address) -> Instruction {
    initialize_ix_with(payer, pda(), system_program())
}

fn increment_data(slot: u64, timestamp_ns: i64, interval_ns: u64) -> Vec<u8> {
    let mut data = vec![1];
    data.extend_from_slice(&slot.to_le_bytes());
    data.extend_from_slice(&timestamp_ns.to_le_bytes());
    data.extend_from_slice(&interval_ns.to_le_bytes());
    data
}

fn increment_ix_with(
    tick: Address,
    signer: Address,
    signer_required: bool,
    slot: u64,
    timestamp_ns: i64,
    interval_ns: u64,
) -> Instruction {
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(tick, false),
            AccountMeta::new_readonly(signer, signer_required),
        ],
        data: increment_data(slot, timestamp_ns, interval_ns),
    }
}

fn increment_ix(signer: Address, slot: u64, timestamp_ns: i64, interval_ns: u64) -> Instruction {
    increment_ix_with(pda(), signer, true, slot, timestamp_ns, interval_ns)
}

struct Harness {
    svm: LiteSVM,
    payer: Keypair,
}

impl Harness {
    fn new(so: &PathBuf) -> Self {
        assert!(
            so.exists(),
            "{} missing; run `cargo build-sbf --manifest-path programs/market-tick/Cargo.toml`",
            so.display()
        );
        let mut svm = LiteSVM::new();
        svm.add_program_from_file(program_id(), so).unwrap();
        svm.warp_to_slot(START_SLOT);
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        Self { svm, payer }
    }

    fn send(&mut self, ix: Instruction, extra: &[&Keypair]) -> TransactionResult {
        self.svm.expire_blockhash();
        let mut signers: Vec<&Keypair> = vec![&self.payer];
        signers.extend_from_slice(extra);
        let message = Message::new(&[ix], Some(&self.payer.pubkey()));
        let tx = Transaction::new(&signers, message, self.svm.latest_blockhash());
        self.svm.send_transaction(tx)
    }

    fn ok(&mut self, ix: Instruction, extra: &[&Keypair]) -> u64 {
        match self.send(ix, extra) {
            Ok(meta) => meta.compute_units_consumed,
            Err(failed) => panic!(
                "transaction failed: {:?}\n{:#?}",
                failed.err, failed.meta.logs
            ),
        }
    }

    fn custom(&mut self, ix: Instruction, extra: &[&Keypair], code: u32) {
        let before = self.raw_tick();
        let error = self
            .send(ix, extra)
            .expect_err("transaction unexpectedly succeeded")
            .err;
        assert_eq!(
            error,
            TransactionError::InstructionError(0, InstructionError::Custom(code))
        );
        assert_eq!(
            self.raw_tick(),
            before,
            "rejected instruction changed the account"
        );
    }

    fn rejected(&mut self, ix: Instruction, extra: &[&Keypair], what: &str) {
        let before = self.raw_tick();
        assert!(
            self.send(ix, extra).is_err(),
            "{what} unexpectedly succeeded"
        );
        assert_eq!(self.raw_tick(), before, "{what} changed the account");
    }

    fn raw_tick(&self) -> Option<Account> {
        self.svm.get_account(&pda())
    }

    fn tick(&self) -> MarketTickV1 {
        let account = self.svm.get_account(&pda()).expect("tick account missing");
        assert_eq!(account.owner, program_id());
        assert_eq!(
            account.lamports,
            self.svm
                .minimum_balance_for_rent_exemption(MarketTickV1::LEN)
        );
        MarketTickV1::decode(&account.data).unwrap()
    }

    fn warp(&mut self, slot: u64) {
        self.svm.warp_to_slot(slot);
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Costs {
    initialize_fresh: u64,
    initialize_prefunded: u64,
    increment_first: u64,
    increment_same_slot: u64,
    increment_new_slot: u64,
}

/// Port of upstream `programs/market-tick/tests/integration.rs`.
fn scenario(so: PathBuf) -> (Costs, Vec<MarketTickV1>) {
    let mut costs = Costs::default();
    let mut states = Vec::new();

    let mut fresh = Harness::new(&so);
    let payer = fresh.payer.pubkey();
    costs.initialize_fresh = fresh.ok(initialize_ix(payer), &[]);
    assert_eq!(fresh.tick(), MarketTickV1::default());

    let mut h = Harness::new(&so);
    let payer = h.payer.pubkey();
    let prefund = h.svm.minimum_balance_for_rent_exemption(0);
    h.ok(
        solana_system_interface::instruction::transfer(&payer, &pda(), prefund),
        &[],
    );
    assert_eq!(h.svm.get_balance(&pda()), Some(prefund));

    let signer = Keypair::new();
    let slot = START_SLOT;
    h.rejected(
        increment_ix(signer.pubkey(), slot, 1_000, 50),
        &[&signer],
        "increment with a System-owned PDA",
    );

    let mut ix = initialize_ix(payer);
    ix.data.push(0);
    ix.accounts
        .push(AccountMeta::new_readonly(Address::new_unique(), false));
    costs.initialize_prefunded = h.ok(ix, &[]);
    let account = h.tick();
    assert_eq!(account, MarketTickV1::default());
    states.push(account);

    h.rejected(initialize_ix(payer), &[], "reinitialization");
    h.rejected(
        initialize_ix_with(payer, Address::new_unique(), system_program()),
        &[],
        "initialization with an incorrect PDA",
    );
    h.rejected(
        initialize_ix_with(payer, pda(), Address::new_unique()),
        &[],
        "initialization with a false System Program",
    );

    h.custom(
        increment_ix(signer.pubkey(), slot + 1, 1_000, 50),
        &[&signer],
        error_code::SLOT_MISMATCH,
    );
    h.rejected(
        increment_ix_with(
            Address::new_unique(),
            signer.pubkey(),
            true,
            slot,
            1_000,
            50,
        ),
        &[&signer],
        "increment with an unknown tick account",
    );
    h.rejected(
        increment_ix_with(pda(), signer.pubkey(), false, slot, 1_000, 50),
        &[],
        "increment without signer privilege",
    );
    let mut short = increment_ix(signer.pubkey(), slot, 1_000, 50);
    short.data.pop();
    h.rejected(short, &[&signer], "truncated increment data");
    h.rejected(
        Instruction {
            data: vec![7],
            ..increment_ix(signer.pubkey(), slot, 1_000, 50)
        },
        &[&signer],
        "unknown instruction tag",
    );
    h.custom(
        increment_ix(signer.pubkey(), slot, -1, 50),
        &[&signer],
        error_code::INVALID_TIMESTAMP,
    );
    h.custom(
        increment_ix(signer.pubkey(), slot, 0, 50),
        &[&signer],
        error_code::INVALID_TIMESTAMP,
    );
    h.custom(
        increment_ix(signer.pubkey(), slot, 1_000, 0),
        &[&signer],
        error_code::INVALID_INTERVAL,
    );

    let mut ix = increment_ix(signer.pubkey(), slot, 1_000, 50);
    ix.data.push(0);
    ix.accounts
        .push(AccountMeta::new_readonly(Address::new_unique(), false));
    h.ok(ix, &[&signer]);
    let account = h.tick();
    assert_eq!(
        account,
        MarketTickV1 {
            signer: signer.pubkey().to_bytes(),
            slot,
            first_timestamp_ns: 1_000,
            timestamp_ns: 1_000,
            sequence: 0,
            target_market_tick_interval_ns: 50,
            observed_market_tick_interval_ns: 0,
        }
    );
    states.push(account);

    h.custom(
        increment_ix(signer.pubkey(), slot, 999, 50),
        &[&signer],
        error_code::NON_MONOTONIC,
    );
    h.custom(
        increment_ix(signer.pubkey(), slot, 1_000, 50),
        &[&signer],
        error_code::NON_MONOTONIC,
    );
    h.custom(
        increment_ix(signer.pubkey(), slot, 1_050, 51),
        &[&signer],
        error_code::INTERVAL_MISMATCH,
    );
    let foreign = Keypair::new();
    h.custom(
        increment_ix(foreign.pubkey(), slot, 1_050, 50),
        &[&foreign],
        error_code::NOT_SLOT_SIGNER,
    );

    costs.increment_same_slot = h.ok(increment_ix(signer.pubkey(), slot, 1_050, 50), &[&signer]);
    let account = h.tick();
    assert_eq!(account.sequence, 1);
    assert_eq!(account.timestamp_ns, 1_050);
    assert_eq!(account.first_timestamp_ns, 1_000);
    assert_eq!(account.observed_market_tick_interval_ns, 50);
    states.push(account);

    let attacker = Keypair::new();
    let contested = slot + 1;
    h.warp(contested);
    h.ok(
        increment_ix(attacker.pubkey(), contested, i64::MAX, u64::MAX),
        &[&attacker],
    );
    let captured = h.tick();
    assert_eq!(captured.signer, attacker.pubkey().to_bytes());
    assert_eq!(captured.timestamp_ns, i64::MAX);
    states.push(captured);
    h.custom(
        increment_ix(attacker.pubkey(), contested, i64::MAX, u64::MAX),
        &[&attacker],
        error_code::NON_MONOTONIC,
    );
    h.custom(
        increment_ix(signer.pubkey(), contested, i64::MAX, u64::MAX),
        &[&signer],
        error_code::NOT_SLOT_SIGNER,
    );

    let next_slot = contested + 1;
    h.warp(next_slot);
    let next_signer = Keypair::new();
    costs.increment_new_slot = h.ok(
        increment_ix(next_signer.pubkey(), next_slot, 500, 100),
        &[&next_signer],
    );
    let account = h.tick();
    assert_eq!(
        account,
        MarketTickV1 {
            signer: next_signer.pubkey().to_bytes(),
            slot: next_slot,
            first_timestamp_ns: 500,
            timestamp_ns: 500,
            sequence: 0,
            target_market_tick_interval_ns: 100,
            observed_market_tick_interval_ns: 0,
        }
    );
    states.push(account);

    let mut fresh_signer_state = Harness::new(&so);
    let payer = fresh_signer_state.payer.pubkey();
    fresh_signer_state.ok(initialize_ix(payer), &[]);
    let first = Keypair::new();
    costs.increment_first =
        fresh_signer_state.ok(increment_ix(first.pubkey(), START_SLOT, 1, 1), &[&first]);

    (costs, states)
}

#[test]
fn pinocchio_matches_anchor_and_is_cheaper() {
    let (pinocchio, pinocchio_states) = scenario(pinocchio_so());
    let (anchor, anchor_states) = scenario(anchor_so());

    let normalize = |states: Vec<MarketTickV1>| {
        states
            .into_iter()
            .map(|mut state| {
                state.signer = [0; 32];
                state
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(normalize(pinocchio_states), normalize(anchor_states));

    let pinocchio_size = std::fs::metadata(pinocchio_so()).unwrap().len();
    let anchor_size = std::fs::metadata(anchor_so()).unwrap().len();

    println!();
    println!("{:<24}{:>12}{:>12}", "", "anchor", "pinocchio");
    println!(
        "{:<24}{:>12}{:>12}",
        "binary size (bytes)", anchor_size, pinocchio_size
    );
    let rows = [
        (
            "initialize (fresh)",
            anchor.initialize_fresh,
            pinocchio.initialize_fresh,
        ),
        (
            "initialize (prefunded)",
            anchor.initialize_prefunded,
            pinocchio.initialize_prefunded,
        ),
        (
            "increment (first ever)",
            anchor.increment_first,
            pinocchio.increment_first,
        ),
        (
            "increment (same slot)",
            anchor.increment_same_slot,
            pinocchio.increment_same_slot,
        ),
        (
            "increment (new slot)",
            anchor.increment_new_slot,
            pinocchio.increment_new_slot,
        ),
    ];
    for (name, a, p) in rows {
        println!("{name:<24}{a:>12}{p:>12}");
    }

    assert!(pinocchio_size < anchor_size);
    for (name, a, p) in rows {
        assert!(p < a, "{name}: pinocchio {p} CU >= anchor {a} CU");
    }
}
