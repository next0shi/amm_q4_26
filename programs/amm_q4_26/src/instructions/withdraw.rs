use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{burn, transfer, Burn, Mint, Token, TokenAccount, Transfer},
};

use crate::{error::AmmError, state::Config};

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub mint_x: Account<'info, Mint>,
    pub mint_y: Account<'info, Mint>,

    #[account(
        has_one = mint_x,
        has_one = mint_y,
        seeds = [b"config", config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump,
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"lp", config.key().as_ref()],
        bump = config.lp_bump,
    )]
    pub mint_lp: Account<'info, Mint>,

    #[account(mut, associated_token::mint = mint_x, associated_token::authority = config)]
    pub vault_x: Account<'info, TokenAccount>,
    #[account(mut, associated_token::mint = mint_y, associated_token::authority = config)]
    pub vault_y: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint_x,
        associated_token::authority = user,
    )]
    pub user_x: Account<'info, TokenAccount>,
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint_y,
        associated_token::authority = user,
    )]
    pub user_y: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_lp, associated_token::authority = user)]
    pub user_lp: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

// floor(a * b / c): rounding down favours the pool
fn mul_div_floor(a: u64, b: u64, c: u64) -> Result<u64> {
    let n = (a as u128).checked_mul(b as u128).ok_or(AmmError::Overflow)?;
    let r = n.checked_div(c as u128).ok_or(AmmError::Overflow)?;
    u64::try_from(r).map_err(|_| AmmError::Overflow.into())
}

impl<'info> Withdraw<'info> {
    pub fn withdraw(&mut self, lp_amount: u64, min_x: u64, min_y: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);
        require!(lp_amount > 0, AmmError::ZeroAmount);

        let supply = self.mint_lp.supply;
        require!(lp_amount <= supply, AmmError::Overflow);

        let x = mul_div_floor(self.vault_x.amount, lp_amount, supply)?;
        let y = mul_div_floor(self.vault_y.amount, lp_amount, supply)?;

        require!(x >= min_x && y >= min_y, AmmError::SlippageExceeded);

        self.burn_lp(lp_amount)?;
        self.receive(true, x)?;
        self.receive(false, y)
    }

    fn burn_lp(&self, amount: u64) -> Result<()> {
        burn(
            CpiContext::new(
                self.token_program.to_account_info(),
                Burn {
                    mint: self.mint_lp.to_account_info(),
                    from: self.user_lp.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
        )
    }

    fn receive(&self, is_x: bool, amount: u64) -> Result<()> {
        let (from, to) = if is_x {
            (&self.vault_x, &self.user_x)
        } else {
            (&self.vault_y, &self.user_y)
        };
        let seed_bytes = self.config.seed.to_le_bytes();
        let seeds: &[&[u8]] = &[b"config", &seed_bytes, &[self.config.config_bump]];
        let signer = &[seeds];
        transfer(
            CpiContext::new_with_signer(
                self.token_program.to_account_info(),
                Transfer {
                    from: from.to_account_info(),
                    to: to.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                signer,
            ),
            amount,
        )
    }
}
