use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{error::AmmError, state::Config};

#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    // Box<...> keeps big account structs on the heap. This instruction creates
    // many accounts and would otherwise risk the 4 KB stack limit.
    #[account(constraint = mint_x.key() != mint_y.key() @ AmmError::SameMint)]
    pub mint_x: Box<Account<'info, Mint>>,
    pub mint_y: Box<Account<'info, Mint>>,

    #[account(
        init,
        payer = admin,
        seeds = [b"config", seed.to_le_bytes().as_ref()],
        bump,
        space = 8 + Config::INIT_SPACE,
    )]
    pub config: Box<Account<'info, Config>>,

    #[account(
        init,
        payer = admin,
        seeds = [b"lp", config.key().as_ref()],
        bump,
        mint::decimals = 6,
        mint::authority = config,
    )]
    pub mint_lp: Box<Account<'info, Mint>>,

    #[account(
        init, payer = admin,
        associated_token::mint = mint_x,
        associated_token::authority = config,
    )]
    pub vault_x: Box<Account<'info, TokenAccount>>,
    #[account(
        init, payer = admin,
        associated_token::mint = mint_y,
        associated_token::authority = config,
    )]
    pub vault_y: Box<Account<'info, TokenAccount>>,

    /// CHECK: a PDA with no data. It only exists to be the *owner* of the
    /// treasury token accounts, so only this program can move that money.
    #[account(seeds = [b"treasury", config.key().as_ref()], bump)]
    pub treasury: UncheckedAccount<'info>,

    #[account(
        init, payer = admin,
        associated_token::mint = mint_x,
        associated_token::authority = treasury,
    )]
    pub treasury_x: Box<Account<'info, TokenAccount>>,
    #[account(
        init, payer = admin,
        associated_token::mint = mint_y,
        associated_token::authority = treasury,
    )]
    pub treasury_y: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> Initialize<'info> {
    pub fn init(
        &mut self,
        seed: u64,
        fee: u16,
        protocol_fee: u16,
        bumps: &InitializeBumps,
    ) -> Result<()> {
        require!(
            fee as u32 + protocol_fee as u32 <= 10_000,
            AmmError::InvalidFee
        );

        self.config.set_inner(Config {
            seed,
            authority: self.admin.key(),
            mint_x: self.mint_x.key(),
            mint_y: self.mint_y.key(),
            fee,
            protocol_fee,
            locked: false,
            config_bump: bumps.config,
            lp_bump: bumps.mint_lp,
            treasury_bump: bumps.treasury,
        });
        Ok(())
    }
}
