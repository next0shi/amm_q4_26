use anchor_lang::prelude::*;

mod error;
mod instructions;
mod state;

pub use instructions::*;

// Replace with your own program id (`anchor keys list`)
declare_id!("CwTqtnyRARLtWNXNY6NaBYJDdntLEY9iHp52ikyteHca");

#[program]
pub mod amm_fresh {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        seed: u64,
        fee: u16,
        protocol_fee: u16,
    ) -> Result<()> {
        ctx.accounts.init(seed, fee, protocol_fee, &ctx.bumps)
    }

    pub fn deposit(ctx: Context<Deposit>, lp_amount: u64, max_x: u64, max_y: u64) -> Result<()> {
        ctx.accounts.deposit(lp_amount, max_x, max_y)
    }

    pub fn withdraw(ctx: Context<Withdraw>, lp_amount: u64, min_x: u64, min_y: u64) -> Result<()> {
        ctx.accounts.withdraw(lp_amount, min_x, min_y)
    }

    pub fn swap(ctx: Context<Swap>, is_x: bool, amount_in: u64, min_out: u64) -> Result<()> {
        ctx.accounts.swap(is_x, amount_in, min_out)
    }

    pub fn update_fee(ctx: Context<UpdateFee>, fee: u16, protocol_fee: u16) -> Result<()> {
        ctx.accounts.update_fee(fee, protocol_fee)
    }

    pub fn set_lock(ctx: Context<SetLock>, locked: bool) -> Result<()> {
        ctx.accounts.set_lock(locked)
    }

    pub fn withdraw_treasury(
        ctx: Context<WithdrawTreasury>,
        amount_x: u64,
        amount_y: u64,
    ) -> Result<()> {
        ctx.accounts.withdraw_treasury(amount_x, amount_y)
    }
}
