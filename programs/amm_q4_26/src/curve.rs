//! Constant-product (x * y = k) market-maker math written from scratch.
//! Plain Rust only: no Anchor, no constant-product-curve crate.
//!
//! Naming: `x`, `y` = reserves in the pool, `dx` = amount of X paid in,
//! `dy` = amount of Y taken out.

/// The invariant. For a fee-free swap it must be preserved (it can only grow
/// when rounding or fees favour the pool).
pub fn k(x: u64, y: u64) -> u128 {
    x as u128 * y as u128
}

/// Y received for paying `dx` of X (fee already removed from `dx`).
/// Derivation: (x + dx) * (y - dy) = x * y  =>  dy = y * dx / (x + dx).
/// Rounded DOWN so the pool never pays out more than the math allows.
pub fn amount_out(x: u64, y: u64, dx: u64) -> Option<u64> {
    let num = (y as u128).checked_mul(dx as u128)?;
    let den = (x as u128).checked_add(dx as u128)?;
    if den == 0 {
        return None;
    }
    u64::try_from(num / den).ok()
}

/// X needed to receive exactly `dy` of Y (inverse of `amount_out`).
/// Derivation: dx = x * dy / (y - dy). Rounded UP so the pool is never short.
pub fn amount_in_for_out(x: u64, y: u64, dy: u64) -> Option<u64> {
    if dy >= y {
        return None; // can never drain the whole reserve
    }
    let num = (x as u128).checked_mul(dy as u128)?;
    let den = (y - dy) as u128;
    let up = num.checked_add(den - 1)? / den;
    u64::try_from(up).ok()
}

/// Remove a fee in basis points (1 bps = 0.01%). Returns (fee, amount_after_fee).
pub fn take_fee(amount: u64, bps: u16) -> Option<(u64, u64)> {
    let fee = u64::try_from((amount as u128).checked_mul(bps as u128)? / 10_000).ok()?;
    Some((fee, amount.checked_sub(fee)?))
}

/// Integer square root (Newton's method), floor.
pub fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// LP tokens minted for the FIRST deposit: sqrt(x * y). This makes the LP
/// amount independent of which token you count in (the "geometric mean").
pub fn initial_lp(x: u64, y: u64) -> u64 {
    isqrt(k(x, y)) as u64
}

/// Tokens a depositor must add to mint `lp` more LP tokens. Rounded UP.
pub fn deposit_amounts(rx: u64, ry: u64, supply: u64, lp: u64) -> Option<(u64, u64)> {
    let up = |reserve: u64| -> Option<u64> {
        let n = (reserve as u128).checked_mul(lp as u128)?;
        let s = supply as u128;
        u64::try_from(n.checked_add(s.checked_sub(1)?)? / s).ok()
    };
    Some((up(rx)?, up(ry)?))
}

/// Tokens returned for burning `lp` LP tokens. Rounded DOWN.
pub fn withdraw_amounts(rx: u64, ry: u64, supply: u64, lp: u64) -> Option<(u64, u64)> {
    let down = |reserve: u64| -> Option<u64> {
        let n = (reserve as u128).checked_mul(lp as u128)?;
        u64::try_from(n / supply as u128).ok()
    };
    Some((down(rx)?, down(ry)?))
}

/// How much worse than the pre-trade price you got, in bps.
/// price_before = y / x, execution price = dy / dx.
pub fn price_impact_bps(x: u64, y: u64, dx: u64, dy: u64) -> Option<u64> {
    // 1 - (dy/dx) / (y/x) = 1 - dy*x / (dx*y)
    let ideal = (dx as u128).checked_mul(y as u128)?;
    if ideal == 0 {
        return None;
    }
    let got = (dy as u128).checked_mul(x as u128)?;
    let diff = ideal.checked_sub(got.min(ideal))?;
    u64::try_from(diff.checked_mul(10_000)? / ideal).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_checked_swap() {
        // 1_000_000 * 10_000 / 1_010_000 = 9900.99 -> 9900
        assert_eq!(amount_out(1_000_000, 1_000_000, 10_000), Some(9_900));
    }

    #[test]
    fn k_never_decreases_on_a_swap() {
        let (x, y) = (1_000_000u64, 2_500_000u64);
        for dx in [1u64, 7, 1_000, 123_456, 5_000_000] {
            if let Some(dy) = amount_out(x, y, dx) {
                assert!(k(x + dx, y - dy) >= k(x, y));
            }
        }
    }

    #[test]
    fn inverse_gives_at_least_the_requested_output() {
        let (x, y) = (1_000_000u64, 1_000_000u64);
        let dx = amount_in_for_out(x, y, 9_900).unwrap();
        assert!(amount_out(x, y, dx).unwrap() >= 9_900);
    }

    #[test]
    fn cannot_drain_the_reserve() {
        assert_eq!(amount_in_for_out(1_000, 1_000, 1_000), None);
        assert!(amount_out(1_000, 1_000, u64::MAX).unwrap() < 1_000);
    }

    #[test]
    fn fee_split() {
        assert_eq!(take_fee(10_000, 20), Some((20, 9_980)));
        assert_eq!(take_fee(9_980, 30), Some((29, 9_951)));
    }

    #[test]
    fn deposit_rounds_up_withdraw_rounds_down() {
        // 3 LP out of 7 supply on a 10-token reserve = 4.28...
        assert_eq!(deposit_amounts(10, 10, 7, 3), Some((5, 5)));
        assert_eq!(withdraw_amounts(10, 10, 7, 3), Some((4, 4)));
    }

    #[test]
    fn sqrt_and_initial_lp() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(initial_lp(1_000_000, 4_000_000), 2_000_000);
    }

    #[test]
    fn bigger_trades_have_more_price_impact() {
        let (x, y) = (1_000_000u64, 1_000_000u64);
        let small = amount_out(x, y, 1_000).unwrap();
        let big = amount_out(x, y, 100_000).unwrap();
        assert!(price_impact_bps(x, y, 100_000, big) > price_impact_bps(x, y, 1_000, small));
    }
}
