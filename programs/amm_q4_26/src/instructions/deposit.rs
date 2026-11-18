use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{mint_to, transfer, Mint, MintTo, Token, TokenAccount, Transfer},
};

use crate::{error::AmmError, state::Config};

#[derive(Accounts)]
pub struct Deposit<'info> {
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

    #[account(mut, associated_token::mint = mint_x, associated_token::authority = user)]
    pub user_x: Account<'info, TokenAccount>,
    #[account(mut, associated_token::mint = mint_y, associated_token::authority = user)]
    pub user_y: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint_lp,
        associated_token::authority = user,
    )]
    pub user_lp: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

fn mul_div_ceil(a: u64, b: u64, c: u64) -> Result<u64> {
    let n = (a as u128).checked_mul(b as u128).ok_or(AmmError::Overflow)?;
    let c = c as u128;
    let r = n
        .checked_add(c.checked_sub(1).ok_or(AmmError::Overflow)?)
        .ok_or(AmmError::Overflow)?
        .checked_div(c)
        .ok_or(AmmError::Overflow)?;
    u64::try_from(r).map_err(|_| AmmError::Overflow.into())
}

impl<'info> Deposit<'info> {
    pub fn deposit(&mut self, lp_amount: u64, max_x: u64, max_y: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);
        require!(lp_amount > 0, AmmError::ZeroAmount);

        let supply = self.mint_lp.supply;

        // First deposit sets the price; later deposits must be proportional.
        let (x, y) = if supply == 0 {
            (max_x, max_y)
        } else {
            (
                mul_div_ceil(self.vault_x.amount, lp_amount, supply)?,
                mul_div_ceil(self.vault_y.amount, lp_amount, supply)?,
            )
        };

        require!(x > 0 && y > 0, AmmError::ZeroAmount);
        require!(x <= max_x && y <= max_y, AmmError::SlippageExceeded);

        self.send(true, x)?;
        self.send(false, y)?;
        self.mint_lp_tokens(lp_amount)
    }

    fn send(&self, is_x: bool, amount: u64) -> Result<()> {
        let (from, to) = if is_x {
            (&self.user_x, &self.vault_x)
        } else {
            (&self.user_y, &self.vault_y)
        };
        transfer(
            CpiContext::new(
                self.token_program.to_account_info(),
                Transfer {
                    from: from.to_account_info(),
                    to: to.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
        )
    }

    fn mint_lp_tokens(&self, amount: u64) -> Result<()> {
        let seed_bytes = self.config.seed.to_le_bytes();
        let seeds: &[&[u8]] = &[b"config", &seed_bytes, &[self.config.config_bump]];
        let signer = &[seeds];
        mint_to(
            CpiContext::new_with_signer(
                self.token_program.to_account_info(),
                MintTo {
                    mint: self.mint_lp.to_account_info(),
                    to: self.user_lp.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                signer,
            ),
            amount,
        )
    }
}
