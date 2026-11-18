use anchor_lang::prelude::*;

use crate::{error::AmmError, state::Config};

#[derive(Accounts)]
pub struct SetLock<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority @ AmmError::Unauthorized,
        seeds = [b"config", config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump,
    )]
    pub config: Account<'info, Config>,
}

impl<'info> SetLock<'info> {
    pub fn set_lock(&mut self, locked: bool) -> Result<()> {
        self.config.locked = locked;
        Ok(())
    }
}
