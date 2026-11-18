use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct Config {
    pub seed: u64,
    pub authority: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub fee: u16,          // LP fee in bps: stays in the pool, rewards LPs
    pub protocol_fee: u16, // protocol fee in bps: sent to the treasury
    pub locked: bool,
    pub config_bump: u8,
    pub lp_bump: u8,
    pub treasury_bump: u8,
}
