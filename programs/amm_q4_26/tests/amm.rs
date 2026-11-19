// LiteSVM tests for amm_q4_26.
// Run:  anchor build && cargo test -p amm_q4_26
// (the tests load target/deploy/amm_q4_26.so, so build first)

use litesvm::LiteSVM;
use litesvm_token::{CreateAssociatedTokenAccount, CreateMint, MintTo};
use sha2::{Digest, Sha256};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use std::str::FromStr;

// Anchor custom error codes = 6000 + index in `AmmError`
const INVALID_FEE: u32 = 6000;
// const SAME_MINT: u32 = 6001;
const LOCKED: u32 = 6002;
const ZERO: u32 = 6003;
const SLIPPAGE: u32 = 6004;
const OVERFLOW: u32 = 6005;
const EMPTY_POOL: u32 = 6006;
const UNAUTHORIZED: u32 = 6007;

// ---------- small helpers ----------

fn token_program() -> Pubkey {
    Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap()
}
fn ata_program() -> Pubkey {
    Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL").unwrap()
}
fn ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[owner.as_ref(), token_program().as_ref(), mint.as_ref()],
        &ata_program(),
    )
    .0
}

/// Anchor instruction data = first 8 bytes of sha256("global:<name>") + borsh args.
fn ix_data(name: &str, args: &[&[u8]]) -> Vec<u8> {
    let h = Sha256::digest(format!("global:{name}"));
    let mut d = h[..8].to_vec();
    for a in args {
        d.extend_from_slice(a);
    }
    d
}

fn send(svm: &mut LiteSVM, ixs: &[Instruction], signer: &Keypair) -> Result<(), String> {
    svm.expire_blockhash(); // lets us send identical transactions twice
    let tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&signer.pubkey()),
        &[signer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{:?}", e.err))
}

fn assert_err(r: Result<(), String>, code: u32) {
    let e = r.expect_err("expected the transaction to fail");
    assert!(e.contains(&format!("Custom({code})")), "wrong error: {e}");
}

/// Same integer math as the program, written independently.
/// returns (protocol_cut, out)
fn quote(rin: u64, rout: u64, amount_in: u64, fee: u64, pf: u64) -> (u64, u64) {
    let cut = amount_in * pf / 10_000;
    let net = amount_in - cut;
    let lp_fee = net * fee / 10_000;
    let after = net - lp_fee;
    let out = (rout as u128 * after as u128 / (rin as u128 + after as u128)) as u64;
    (cut, out)
}

// ---------- test fixture ----------

struct Ctx {
    svm: LiteSVM,
    admin: Keypair,
    program_id: Pubkey,
    mint_x: Pubkey,
    mint_y: Pubkey,
    seed: u64,
    config: Pubkey,
    mint_lp: Pubkey,
    vault_x: Pubkey,
    vault_y: Pubkey,
    treasury: Pubkey,
    treasury_x: Pubkey,
    treasury_y: Pubkey,
}

impl Ctx {
    /// Program loaded, mints created, addresses derived. Pool NOT initialized.
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let program_id = Pubkey::new_from_array(amm_q4_26::ID.to_bytes());
        svm.add_program_from_file(program_id, "../../target/deploy/amm_q4_26.so")
            .unwrap();

        let admin = Keypair::new();
        svm.airdrop(&admin.pubkey(), 100_000_000_000).unwrap();

        let mint_x = CreateMint::new(&mut svm, &admin)
            .decimals(6)
            .send()
            .unwrap();
        let mint_y = CreateMint::new(&mut svm, &admin)
            .decimals(6)
            .send()
            .unwrap();

        let seed = 1u64;
        let (config, _) =
            Pubkey::find_program_address(&[b"config", &seed.to_le_bytes()], &program_id);
        let (mint_lp, _) = Pubkey::find_program_address(&[b"lp", config.as_ref()], &program_id);
        let (treasury, _) =
            Pubkey::find_program_address(&[b"treasury", config.as_ref()], &program_id);

        Self {
            vault_x: ata(&config, &mint_x),
            vault_y: ata(&config, &mint_y),
            treasury_x: ata(&treasury, &mint_x),
            treasury_y: ata(&treasury, &mint_y),
            svm,
            admin,
            program_id,
            mint_x,
            mint_y,
            seed,
            config,
            mint_lp,
            treasury,
        }
    }

    /// Fresh context with the pool initialized.
    fn setup(fee: u16, pf: u16) -> Self {
        let mut c = Self::new();
        let ix = c.init_ix(c.mint_x, c.mint_y, fee, pf);
        send(&mut c.svm, &[ix], &c.admin).unwrap();
        c
    }

    /// New wallet with SOL and `x` / `y` tokens in its ATAs.
    fn fund_user(&mut self, x: u64, y: u64) -> Keypair {
        let user = Keypair::new();
        self.svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();
        for (mint, amount) in [(self.mint_x, x), (self.mint_y, y)] {
            let a = CreateAssociatedTokenAccount::new(&mut self.svm, &self.admin, &mint)
                .owner(&user.pubkey())
                .send()
                .unwrap();
            if amount > 0 {
                MintTo::new(&mut self.svm, &self.admin, &mint, &a, amount)
                    .send()
                    .unwrap();
            }
        }
        user
    }

    /// A user with plenty of tokens who already put `amount` of each token in the pool.
    fn seed_pool(&mut self, amount: u64) -> Keypair {
        let user = self.fund_user(amount * 10, amount * 10);
        let ix = self.deposit_ix(&user.pubkey(), amount, amount, amount);
        send(&mut self.svm, &[ix], &user).unwrap();
        user
    }

    // ----- balances -----
    fn bal(&self, account: &Pubkey) -> u64 {
        self.svm
            .get_account(account)
            .map(|a| u64::from_le_bytes(a.data[64..72].try_into().unwrap()))
            .unwrap_or(0)
    }
    fn supply(&self) -> u64 {
        let a = self.svm.get_account(&self.mint_lp).unwrap();
        u64::from_le_bytes(a.data[36..44].try_into().unwrap())
    }
    fn user_x(&self, u: &Keypair) -> u64 {
        self.bal(&ata(&u.pubkey(), &self.mint_x))
    }
    fn user_y(&self, u: &Keypair) -> u64 {
        self.bal(&ata(&u.pubkey(), &self.mint_y))
    }
    fn user_lp(&self, u: &Keypair) -> u64 {
        self.bal(&ata(&u.pubkey(), &self.mint_lp))
    }

    // ----- instruction builders (account order == the Rust structs) -----
    fn progs() -> Vec<AccountMeta> {
        vec![
            AccountMeta::new_readonly(token_program(), false),
            AccountMeta::new_readonly(ata_program(), false),
            AccountMeta::new_readonly(system_program::ID, false),
        ]
    }

    fn init_ix(&self, mx: Pubkey, my: Pubkey, fee: u16, pf: u16) -> Instruction {
        let mut accounts = vec![
            AccountMeta::new(self.admin.pubkey(), true),
            AccountMeta::new_readonly(mx, false),
            AccountMeta::new_readonly(my, false),
            AccountMeta::new(self.config, false),
            AccountMeta::new(self.mint_lp, false),
            AccountMeta::new(self.vault_x, false),
            AccountMeta::new(self.vault_y, false),
            AccountMeta::new_readonly(self.treasury, false),
            AccountMeta::new(self.treasury_x, false),
            AccountMeta::new(self.treasury_y, false),
        ];
        accounts.extend(Self::progs());
        Instruction {
            program_id: self.program_id,
            accounts,
            data: ix_data(
                "initialize",
                &[
                    &self.seed.to_le_bytes()[..],
                    &fee.to_le_bytes()[..],
                    &pf.to_le_bytes()[..],
                ],
            ),
        }
    }

    // deposit and withdraw share the same account layout
    fn lp_accounts(&self, user: &Pubkey) -> Vec<AccountMeta> {
        let mut m = vec![
            AccountMeta::new(*user, true),
            AccountMeta::new_readonly(self.mint_x, false),
            AccountMeta::new_readonly(self.mint_y, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new(self.mint_lp, false),
            AccountMeta::new(self.vault_x, false),
            AccountMeta::new(self.vault_y, false),
            AccountMeta::new(ata(user, &self.mint_x), false),
            AccountMeta::new(ata(user, &self.mint_y), false),
            AccountMeta::new(ata(user, &self.mint_lp), false),
        ];
        m.extend(Self::progs());
        m
    }

    fn deposit_ix(&self, user: &Pubkey, lp: u64, max_x: u64, max_y: u64) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: self.lp_accounts(user),
            data: ix_data(
                "deposit",
                &[
                    &lp.to_le_bytes()[..],
                    &max_x.to_le_bytes()[..],
                    &max_y.to_le_bytes()[..],
                ],
            ),
        }
    }

    fn withdraw_ix(&self, user: &Pubkey, lp: u64, min_x: u64, min_y: u64) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: self.lp_accounts(user),
            data: ix_data(
                "withdraw",
                &[
                    &lp.to_le_bytes()[..],
                    &min_x.to_le_bytes()[..],
                    &min_y.to_le_bytes()[..],
                ],
            ),
        }
    }

    fn swap_ix(&self, user: &Pubkey, is_x: bool, amount_in: u64, min_out: u64) -> Instruction {
        let mut accounts = vec![
            AccountMeta::new(*user, true),
            AccountMeta::new_readonly(self.mint_x, false),
            AccountMeta::new_readonly(self.mint_y, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new(self.vault_x, false),
            AccountMeta::new(self.vault_y, false),
            AccountMeta::new_readonly(self.treasury, false),
            AccountMeta::new(self.treasury_x, false),
            AccountMeta::new(self.treasury_y, false),
            AccountMeta::new(ata(user, &self.mint_x), false),
            AccountMeta::new(ata(user, &self.mint_y), false),
        ];
        accounts.extend(Self::progs());
        Instruction {
            program_id: self.program_id,
            accounts,
            data: ix_data(
                "swap",
                &[
                    &[is_x as u8][..],
                    &amount_in.to_le_bytes()[..],
                    &min_out.to_le_bytes()[..],
                ],
            ),
        }
    }

    fn update_fee_ix(&self, authority: &Pubkey, fee: u16, pf: u16) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(*authority, true),
                AccountMeta::new(self.config, false),
            ],
            data: ix_data(
                "update_fee",
                &[&fee.to_le_bytes()[..], &pf.to_le_bytes()[..]],
            ),
        }
    }

    fn set_lock_ix(&self, authority: &Pubkey, locked: bool) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(*authority, true),
                AccountMeta::new(self.config, false),
            ],
            data: ix_data("set_lock", &[&[locked as u8][..]]),
        }
    }

    fn withdraw_treasury_ix(&self, authority: &Pubkey, ax: u64, ay: u64) -> Instruction {
        let mut accounts = vec![
            AccountMeta::new(*authority, true),
            AccountMeta::new_readonly(self.mint_x, false),
            AccountMeta::new_readonly(self.mint_y, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new_readonly(self.treasury, false),
            AccountMeta::new(self.treasury_x, false),
            AccountMeta::new(self.treasury_y, false),
            AccountMeta::new(ata(authority, &self.mint_x), false),
            AccountMeta::new(ata(authority, &self.mint_y), false),
        ];
        accounts.extend(Self::progs());
        Instruction {
            program_id: self.program_id,
            accounts,
            data: ix_data(
                "withdraw_treasury",
                &[&ax.to_le_bytes()[..], &ay.to_le_bytes()[..]],
            ),
        }
    }

    // ----- shortcuts that send as a given signer -----
    fn lock(&mut self, locked: bool) {
        let ix = self.set_lock_ix(&self.admin.pubkey(), locked);
        send(&mut self.svm, &[ix], &self.admin).unwrap();
    }
}

// =====================================================================
// initialize
// =====================================================================

#[test]
fn initialize_sets_state() {
    let c = Ctx::setup(30, 20);
    let d = c.svm.get_account(&c.config).unwrap().data;
    // layout: 8 disc | seed 8 | authority 32 | mint_x 32 | mint_y 32 | fee 2 | pf 2 | locked 1
    assert_eq!(u64::from_le_bytes(d[8..16].try_into().unwrap()), c.seed);
    assert_eq!(&d[16..48], c.admin.pubkey().as_ref());
    assert_eq!(&d[48..80], c.mint_x.as_ref());
    assert_eq!(&d[80..112], c.mint_y.as_ref());
    assert_eq!(u16::from_le_bytes(d[112..114].try_into().unwrap()), 30);
    assert_eq!(u16::from_le_bytes(d[114..116].try_into().unwrap()), 20);
    assert_eq!(d[116], 0); // not locked

    for a in [c.vault_x, c.vault_y, c.treasury_x, c.treasury_y, c.mint_lp] {
        assert!(c.svm.get_account(&a).is_some());
    }
    assert_eq!(c.bal(&c.vault_x), 0);
    assert_eq!(c.supply(), 0);
    // LP mint authority is the config PDA (bytes 4..36 of a Mint, after the COption tag)
    let lp = c.svm.get_account(&c.mint_lp).unwrap().data;
    assert_eq!(&lp[4..36], c.config.as_ref());
}

#[test]
fn initialize_rejects_same_mint() {
    let mut c = Ctx::new();
    let ix = c.init_ix(c.mint_x, c.mint_x, 30, 20);
    // Anchor's init of the duplicate vault fails before the SameMint
    // constraint is reached, so we only assert that it is rejected.
    assert!(send(&mut c.svm, &[ix], &c.admin).is_err());
    // and nothing was created
    assert!(c.svm.get_account(&c.config).is_none());
}

#[test]
fn initialize_rejects_fees_over_100_percent() {
    let mut c = Ctx::new();
    let ix = c.init_ix(c.mint_x, c.mint_y, 9_000, 2_000);
    assert_err(send(&mut c.svm, &[ix], &c.admin), INVALID_FEE);
}

#[test]
fn initialize_rejects_duplicate_seed() {
    let mut c = Ctx::setup(30, 20);
    let ix = c.init_ix(c.mint_x, c.mint_y, 30, 20);
    assert!(send(&mut c.svm, &[ix], &c.admin).is_err());
}

// =====================================================================
// deposit
// =====================================================================

#[test]
fn deposit_first_sets_price() {
    let mut c = Ctx::setup(30, 20);
    let u = c.fund_user(5_000_000, 5_000_000);
    let ix = c.deposit_ix(&u.pubkey(), 1_000_000, 1_000_000, 2_000_000);
    send(&mut c.svm, &[ix], &u).unwrap();

    assert_eq!(c.bal(&c.vault_x), 1_000_000);
    assert_eq!(c.bal(&c.vault_y), 2_000_000); // price 1 X = 2 Y
    assert_eq!(c.user_lp(&u), 1_000_000);
    assert_eq!(c.supply(), 1_000_000);
    assert_eq!(c.user_x(&u), 4_000_000);
}

#[test]
fn deposit_second_is_proportional() {
    let mut c = Ctx::setup(30, 20);
    let _u1 = c.seed_pool(1_000_000);
    let u2 = c.fund_user(10_000_000, 10_000_000);
    let ix = c.deposit_ix(&u2.pubkey(), 500_000, 10_000_000, 10_000_000);
    send(&mut c.svm, &[ix], &u2).unwrap();

    assert_eq!(c.bal(&c.vault_x), 1_500_000);
    assert_eq!(c.bal(&c.vault_y), 1_500_000);
    assert_eq!(c.supply(), 1_500_000);
    assert_eq!(c.user_x(&u2), 10_000_000 - 500_000);
    assert_eq!(c.user_lp(&u2), 500_000);
}

#[test]
fn deposit_slippage() {
    let mut c = Ctx::setup(30, 20);
    let _u1 = c.seed_pool(1_000_000);
    let u2 = c.fund_user(10_000_000, 10_000_000);
    let ix = c.deposit_ix(&u2.pubkey(), 500_000, 499_999, 10_000_000);
    assert_err(send(&mut c.svm, &[ix], &u2), SLIPPAGE);
}

#[test]
fn deposit_zero_lp() {
    let mut c = Ctx::setup(30, 20);
    let u = c.fund_user(1_000_000, 1_000_000);
    let ix = c.deposit_ix(&u.pubkey(), 0, 1_000_000, 1_000_000);
    assert_err(send(&mut c.svm, &[ix], &u), ZERO);
}

#[test]
fn deposit_locked() {
    let mut c = Ctx::setup(30, 20);
    c.lock(true);
    let u = c.fund_user(1_000_000, 1_000_000);
    let ix = c.deposit_ix(&u.pubkey(), 1_000, 1_000, 1_000);
    assert_err(send(&mut c.svm, &[ix], &u), LOCKED);
}

// =====================================================================
// withdraw
// =====================================================================

#[test]
fn withdraw_partial() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let ix = c.withdraw_ix(&u.pubkey(), 250_000, 250_000, 250_000);
    send(&mut c.svm, &[ix], &u).unwrap();

    assert_eq!(c.bal(&c.vault_x), 750_000);
    assert_eq!(c.bal(&c.vault_y), 750_000);
    assert_eq!(c.supply(), 750_000);
    assert_eq!(c.user_x(&u), 9_000_000 + 250_000);
    assert_eq!(c.user_lp(&u), 750_000);
}

#[test]
fn withdraw_full_drains_pool() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let ix = c.withdraw_ix(&u.pubkey(), 1_000_000, 1_000_000, 1_000_000);
    send(&mut c.svm, &[ix], &u).unwrap();

    assert_eq!(c.bal(&c.vault_x), 0);
    assert_eq!(c.bal(&c.vault_y), 0);
    assert_eq!(c.supply(), 0);
    assert_eq!(c.user_x(&u), 10_000_000);
}

#[test]
fn withdraw_slippage() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let ix = c.withdraw_ix(&u.pubkey(), 250_000, 250_001, 0);
    assert_err(send(&mut c.svm, &[ix], &u), SLIPPAGE);
}

#[test]
fn withdraw_zero_lp() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let ix = c.withdraw_ix(&u.pubkey(), 0, 0, 0);
    assert_err(send(&mut c.svm, &[ix], &u), ZERO);
}

#[test]
fn withdraw_more_than_total_supply() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let ix = c.withdraw_ix(&u.pubkey(), 1_000_001, 0, 0);
    assert_err(send(&mut c.svm, &[ix], &u), OVERFLOW);
}

#[test]
fn withdraw_more_than_user_holds_fails() {
    let mut c = Ctx::setup(30, 20);
    let u1 = c.seed_pool(1_000_000);
    // second LP so total supply is 2_000_000 and the supply check passes
    let u2 = c.fund_user(10_000_000, 10_000_000);
    let d = c.deposit_ix(&u2.pubkey(), 1_000_000, 10_000_000, 10_000_000);
    send(&mut c.svm, &[d], &u2).unwrap();
    // u1 only holds 1_000_000 LP: the burn must fail
    let ix = c.withdraw_ix(&u1.pubkey(), 1_500_000, 0, 0);
    assert!(send(&mut c.svm, &[ix], &u1).is_err());
}

#[test]
fn withdraw_locked() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    c.lock(true);
    let ix = c.withdraw_ix(&u.pubkey(), 1_000, 0, 0);
    assert_err(send(&mut c.svm, &[ix], &u), LOCKED);
}

// =====================================================================
// swap
// =====================================================================

#[test]
fn swap_x_to_y_without_fees_matches_hand_math() {
    let mut c = Ctx::setup(0, 0);
    let u = c.seed_pool(1_000_000);
    let (y0, x0) = (c.user_y(&u), c.user_x(&u));
    // 1_000_000 * 10_000 / 1_010_000 = 9900.99 -> 9900
    let ix = c.swap_ix(&u.pubkey(), true, 10_000, 9_900);
    send(&mut c.svm, &[ix], &u).unwrap();

    assert_eq!(c.user_y(&u) - y0, 9_900);
    assert_eq!(x0 - c.user_x(&u), 10_000);
    assert_eq!(c.bal(&c.vault_x), 1_010_000);
    assert_eq!(c.bal(&c.vault_y), 990_100);
    assert_eq!(c.bal(&c.treasury_x), 0);
}

#[test]
fn swap_x_to_y_splits_fees() {
    let (fee, pf) = (30u16, 20u16);
    let mut c = Ctx::setup(fee, pf);
    let u = c.seed_pool(1_000_000);
    let y0 = c.user_y(&u);

    let (cut, out) = quote(1_000_000, 1_000_000, 10_000, fee as u64, pf as u64);
    assert_eq!(cut, 20); // 10_000 * 20 / 10_000
    let ix = c.swap_ix(&u.pubkey(), true, 10_000, out);
    send(&mut c.svm, &[ix], &u).unwrap();

    assert_eq!(c.bal(&c.treasury_x), cut);
    // vault gets everything except the protocol cut (LP fee stays inside)
    assert_eq!(c.bal(&c.vault_x), 1_000_000 + 10_000 - cut);
    assert_eq!(c.bal(&c.vault_y), 1_000_000 - out);
    assert_eq!(c.user_y(&u) - y0, out);
}

#[test]
fn swap_y_to_x_splits_fees() {
    let (fee, pf) = (30u16, 20u16);
    let mut c = Ctx::setup(fee, pf);
    let u = c.seed_pool(1_000_000);
    let x0 = c.user_x(&u);

    let (cut, out) = quote(1_000_000, 1_000_000, 50_000, fee as u64, pf as u64);
    let ix = c.swap_ix(&u.pubkey(), false, 50_000, out);
    send(&mut c.svm, &[ix], &u).unwrap();

    assert_eq!(c.bal(&c.treasury_y), cut);
    assert_eq!(c.bal(&c.treasury_x), 0);
    assert_eq!(c.user_x(&u) - x0, out);
    assert_eq!(c.bal(&c.vault_y), 1_000_000 + 50_000 - cut);
}

#[test]
fn swap_min_out_enforced() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let (_, out) = quote(1_000_000, 1_000_000, 10_000, 30, 20);
    let ix = c.swap_ix(&u.pubkey(), true, 10_000, out + 1);
    assert_err(send(&mut c.svm, &[ix], &u), SLIPPAGE);
}

#[test]
fn swap_k_never_decreases() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    for (i, is_x) in [true, false, true, true, false].into_iter().enumerate() {
        let k0 = c.bal(&c.vault_x) as u128 * c.bal(&c.vault_y) as u128;
        let ix = c.swap_ix(&u.pubkey(), is_x, 20_000 + i as u64 * 7_777, 0);
        send(&mut c.svm, &[ix], &u).unwrap();
        let k1 = c.bal(&c.vault_x) as u128 * c.bal(&c.vault_y) as u128;
        assert!(k1 >= k0, "k went down: {k0} -> {k1}");
    }
}

#[test]
fn swap_zero_amount() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let ix = c.swap_ix(&u.pubkey(), true, 0, 0);
    assert_err(send(&mut c.svm, &[ix], &u), ZERO);
}

#[test]
fn swap_tiny_amount_that_rounds_to_zero_out_fails() {
    let mut c = Ctx::setup(0, 0);
    let u = c.seed_pool(1_000_000);
    // 1_000_000 * 1 / 1_000_001 = 0
    let ix = c.swap_ix(&u.pubkey(), true, 1, 0);
    assert_err(send(&mut c.svm, &[ix], &u), ZERO);
}

#[test]
fn swap_empty_pool() {
    let mut c = Ctx::setup(30, 20);
    let u = c.fund_user(1_000_000, 1_000_000);
    let ix = c.swap_ix(&u.pubkey(), true, 1_000, 0);
    assert_err(send(&mut c.svm, &[ix], &u), EMPTY_POOL);
}

#[test]
fn swap_locked() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    c.lock(true);
    let ix = c.swap_ix(&u.pubkey(), true, 1_000, 0);
    assert_err(send(&mut c.svm, &[ix], &u), LOCKED);
}

#[test]
fn swap_huge_trade_never_empties_the_vault() {
    let mut c = Ctx::setup(0, 0);
    let u = c.seed_pool(1_000_000); // user holds 9_000_000 X after depositing
    let ix = c.swap_ix(&u.pubkey(), true, 9_000_000, 0);
    send(&mut c.svm, &[ix], &u).unwrap();
    assert!(c.bal(&c.vault_y) > 0);
}

// =====================================================================
// admin: fees, lock, treasury
// =====================================================================

#[test]
fn update_fee_by_authority_changes_treasury_cut() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);

    let ix = c.update_fee_ix(&c.admin.pubkey(), 50, 10);
    send(&mut c.svm, &[ix], &c.admin).unwrap();

    let ix = c.swap_ix(&u.pubkey(), true, 100_000, 0);
    send(&mut c.svm, &[ix], &u).unwrap();
    assert_eq!(c.bal(&c.treasury_x), 100); // 100_000 * 10 / 10_000
}

#[test]
fn update_fee_rejects_non_authority() {
    let mut c = Ctx::setup(30, 20);
    let attacker = c.fund_user(0, 0);
    let ix = c.update_fee_ix(&attacker.pubkey(), 0, 0);
    assert_err(send(&mut c.svm, &[ix], &attacker), UNAUTHORIZED);
}

#[test]
fn update_fee_rejects_invalid_combo() {
    let mut c = Ctx::setup(30, 20);
    let ix = c.update_fee_ix(&c.admin.pubkey(), 9_000, 2_000);
    assert_err(send(&mut c.svm, &[ix], &c.admin), INVALID_FEE);
}

#[test]
fn set_lock_rejects_non_authority() {
    let mut c = Ctx::setup(30, 20);
    let attacker = c.fund_user(0, 0);
    let ix = c.set_lock_ix(&attacker.pubkey(), true);
    assert_err(send(&mut c.svm, &[ix], &attacker), UNAUTHORIZED);
}

#[test]
fn withdraw_treasury_by_authority() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let ix = c.swap_ix(&u.pubkey(), true, 10_000, 0);
    send(&mut c.svm, &[ix], &u).unwrap();
    assert_eq!(c.bal(&c.treasury_x), 20);

    let ix = c.withdraw_treasury_ix(&c.admin.pubkey(), 20, 0);
    send(&mut c.svm, &[ix], &c.admin).unwrap();

    assert_eq!(c.bal(&c.treasury_x), 0);
    assert_eq!(c.bal(&ata(&c.admin.pubkey(), &c.mint_x)), 20);
}

#[test]
fn withdraw_treasury_rejects_non_authority() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let ix = c.swap_ix(&u.pubkey(), true, 10_000, 0);
    send(&mut c.svm, &[ix], &u).unwrap();

    let ix = c.withdraw_treasury_ix(&u.pubkey(), 20, 0);
    assert_err(send(&mut c.svm, &[ix], &u), UNAUTHORIZED);
    assert_eq!(c.bal(&c.treasury_x), 20); // untouched
}

#[test]
fn withdraw_treasury_more_than_balance_fails() {
    let mut c = Ctx::setup(30, 20);
    let u = c.seed_pool(1_000_000);
    let ix = c.swap_ix(&u.pubkey(), true, 10_000, 0);
    send(&mut c.svm, &[ix], &u).unwrap();

    let ix = c.withdraw_treasury_ix(&c.admin.pubkey(), 21, 0);
    assert!(send(&mut c.svm, &[ix], &c.admin).is_err());
}
