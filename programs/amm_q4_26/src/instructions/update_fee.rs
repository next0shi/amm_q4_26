use anchor_lang::prelude::*;

use crate::{error::AmmError, state::Config};

#[derive(Accounts)]
pub struct UpdateFee<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority @ AmmError::Unauthorized,
        seeds = [b"config", config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump,
    )]
    pub config: Account<'info, Config>,
}

impl<'info> UpdateFee<'info> {
    pub fn update_fee(&mut self, fee: u16, protocol_fee: u16) -> Result<()> {
        require!(
            fee as u32 + protocol_fee as u32 <= 10_000,
            AmmError::InvalidFee
        );
        self.config.fee = fee;
        self.config.protocol_fee = protocol_fee;
        Ok(())
    }
}
