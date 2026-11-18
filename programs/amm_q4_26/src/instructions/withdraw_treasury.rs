use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{transfer, Mint, Token, TokenAccount, Transfer},
};

use crate::{error::AmmError, state::Config};

#[derive(Accounts)]
pub struct WithdrawTreasury<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub mint_x: Box<Account<'info, Mint>>,
    pub mint_y: Box<Account<'info, Mint>>,

    #[account(
        has_one = mint_x,
        has_one = mint_y,
        has_one = authority @ AmmError::Unauthorized,
        seeds = [b"config", config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump,
    )]
    pub config: Box<Account<'info, Config>>,

    /// CHECK: PDA that owns the treasury token accounts; address checked by seeds.
    #[account(seeds = [b"treasury", config.key().as_ref()], bump = config.treasury_bump)]
    pub treasury: UncheckedAccount<'info>,
    #[account(mut, associated_token::mint = mint_x, associated_token::authority = treasury)]
    pub treasury_x: Box<Account<'info, TokenAccount>>,
    #[account(mut, associated_token::mint = mint_y, associated_token::authority = treasury)]
    pub treasury_y: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed, payer = authority,
        associated_token::mint = mint_x,
        associated_token::authority = authority,
    )]
    pub authority_x: Box<Account<'info, TokenAccount>>,
    #[account(
        init_if_needed, payer = authority,
        associated_token::mint = mint_y,
        associated_token::authority = authority,
    )]
    pub authority_y: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> WithdrawTreasury<'info> {
    pub fn withdraw_treasury(&mut self, amount_x: u64, amount_y: u64) -> Result<()> {
        require!(amount_x > 0 || amount_y > 0, AmmError::ZeroAmount);

        // the treasury PDA signs, not the authority: it owns the token accounts
        let config_key = self.config.key();
        let seeds: &[&[u8]] = &[
            b"treasury",
            config_key.as_ref(),
            &[self.config.treasury_bump],
        ];
        let signer = &[seeds];

        if amount_x > 0 {
            transfer(
                CpiContext::new_with_signer(
                    self.token_program.to_account_info(),
                    Transfer {
                        from: self.treasury_x.to_account_info(),
                        to: self.authority_x.to_account_info(),
                        authority: self.treasury.to_account_info(),
                    },
                    signer,
                ),
                amount_x,
            )?;
        }
        if amount_y > 0 {
            transfer(
                CpiContext::new_with_signer(
                    self.token_program.to_account_info(),
                    Transfer {
                        from: self.treasury_y.to_account_info(),
                        to: self.authority_y.to_account_info(),
                        authority: self.treasury.to_account_info(),
                    },
                    signer,
                ),
                amount_y,
            )?;
        }
        Ok(())
    }
}
