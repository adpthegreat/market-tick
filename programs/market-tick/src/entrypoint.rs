use pinocchio::{
    entrypoint::InstructionContext, lazy_program_entrypoint, no_allocator, ProgramResult,
};

use crate::instructions::{parse, process_increment, process_initialize, MarketTickInstruction};

lazy_program_entrypoint!(process_instruction);
no_allocator!();

#[cfg(target_os = "solana")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe { pinocchio::syscalls::abort() }
}

#[inline(always)]
fn process_instruction(context: InstructionContext) -> ProgramResult {
    match parse(context)? {
        MarketTickInstruction::Initialize(accounts) => process_initialize(&accounts),
        MarketTickInstruction::Increment(accounts, data) => process_increment(&accounts, data),
    }
}
