use anchor_lang::prelude::*;

// Anchor numbers custom errors from 6000 in declaration order.
#[error_code]
pub enum AmmError {
    #[msg("fee + protocol_fee must be at most 10_000 bps")]
    InvalidFee, // 6000
    #[msg("mint_x and mint_y must be different")]
    SameMint, // 6001
    #[msg("pool is locked")]
    PoolLocked, // 6002
    #[msg("amount must be greater than zero")]
    ZeroAmount, // 6003
    #[msg("slippage limit exceeded")]
    SlippageExceeded, // 6004
    #[msg("math overflow")]
    Overflow, // 6005
    #[msg("pool has no liquidity")]
    EmptyPool, // 6006
    #[msg("signer is not the pool authority")]
    Unauthorized, // 6007
}
