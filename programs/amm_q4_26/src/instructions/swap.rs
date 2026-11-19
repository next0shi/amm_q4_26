use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{transfer, Mint, Token, TokenAccount, Transfer},
};

use crate::{curve, error::AmmError, state::Config};

#[derive(Accounts)]
pub struct Swap<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub mint_x: Box<Account<'info, Mint>>,
    pub mint_y: Box<Account<'info, Mint>>,

    #[account(
        has_one = mint_x,
        has_one = mint_y,
        seeds = [b"config", config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump,
    )]
    pub config: Box<Account<'info, Config>>,

    #[account(mut, associated_token::mint = mint_x, associated_token::authority = config)]
    pub vault_x: Box<Account<'info, TokenAccount>>,
    #[account(mut, associated_token::mint = mint_y, associated_token::authority = config)]
    pub vault_y: Box<Account<'info, TokenAccount>>,

    /// CHECK: PDA that owns the treasury token accounts; address checked by seeds.
    #[account(seeds = [b"treasury", config.key().as_ref()], bump = config.treasury_bump)]
    pub treasury: UncheckedAccount<'info>,
    #[account(mut, associated_token::mint = mint_x, associated_token::authority = treasury)]
    pub treasury_x: Box<Account<'info, TokenAccount>>,
    #[account(mut, associated_token::mint = mint_y, associated_token::authority = treasury)]
    pub treasury_y: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed, payer = user,
        associated_token::mint = mint_x,
        associated_token::authority = user,
    )]
    pub user_x: Box<Account<'info, TokenAccount>>,
    #[account(
        init_if_needed, payer = user,
        associated_token::mint = mint_y,
        associated_token::authority = user,
    )]
    pub user_y: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> Swap<'info> {
    /// `is_x == true`: the user pays X and receives Y.
    pub fn swap(&mut self, is_x: bool, amount_in: u64, min_out: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);
        require!(amount_in > 0, AmmError::ZeroAmount);

        let (reserve_in, reserve_out) = if is_x {
            (self.vault_x.amount, self.vault_y.amount)
        } else {
            (self.vault_y.amount, self.vault_x.amount)
        };
        require!(reserve_in > 0 && reserve_out > 0, AmmError::EmptyPool);

        // 1) protocol fee: taken off the top, goes to the treasury
        let (protocol_cut, net_in) =
            curve::take_fee(amount_in, self.config.protocol_fee).ok_or(AmmError::Overflow)?;

        // 2) LP fee: stays in the vault but is NOT fed to the curve, so k grows
        let (_lp_fee, in_for_curve) =
            curve::take_fee(net_in, self.config.fee).ok_or(AmmError::Overflow)?;

        // 3) the curve decides how much the user gets
        let out =
            curve::amount_out(reserve_in, reserve_out, in_for_curve).ok_or(AmmError::Overflow)?;
        require!(out > 0, AmmError::ZeroAmount);
        require!(out >= min_out, AmmError::SlippageExceeded);

        // 4) move the money: user -> vault (net_in), user -> treasury (cut), vault -> user (out)
        self.pay_in(is_x, false, net_in)?;
        self.pay_in(is_x, true, protocol_cut)?;
        self.pay_out(is_x, out)
    }

    fn pay_in(&self, is_x: bool, to_treasury: bool, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        let from = if is_x { &self.user_x } else { &self.user_y };
        let to = match (is_x, to_treasury) {
            (true, false) => &self.vault_x,
            (false, false) => &self.vault_y,
            (true, true) => &self.treasury_x,
            (false, true) => &self.treasury_y,
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

    fn pay_out(&self, is_x: bool, amount: u64) -> Result<()> {
        // paying X in means Y comes out, and vice versa
        let (from, to) = if is_x {
            (&self.vault_y, &self.user_y)
        } else {
            (&self.vault_x, &self.user_x)
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
