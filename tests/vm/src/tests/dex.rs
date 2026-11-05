use super::*;
use fossa_dex::{accounts, args, instruction as ix, state_types};

const ACTION_FEE: u64 = 25_000_000;
const SUPPLY: u64 = 1_000_000_000;

#[path = "dex_compute.rs"]
mod compute;

#[path = "dex_liquidity.rs"]
mod liquidity;

fn now(svm: &LiteSVM) -> u64 {
    svm.get_sysvar::<solana_clock::Clock>().unix_timestamp as u64
}

fn advance(svm: &mut LiteSVM, seconds: i64) {
    let mut clock = svm.get_sysvar::<solana_clock::Clock>();
    clock.unix_timestamp += seconds;
    svm.set_sysvar(&clock);
}

struct PoolFixture {
    mint: Address,
    quote: Address,
    token_a: Address,
    token_b: Address,
    config: Address,
    pool: Address,
    authority: Address,
    source_a: Address,
    source_b: Address,
    vault_a: Address,
    vault_b: Address,
    lp_mint: Address,
    user_lp: Keypair,
}

impl PoolFixture {
    fn launch(&self, f: &DexFixture) -> fossa_dex::contexts::__client_accounts_launch::Launch {
        fossa_dex::contexts::__client_accounts_launch::Launch {
            payer: f.payer.pubkey(),
            global: f.global,
            treasury: f.treasury,
            config: self.config,
            token_a: self.token_a,
            token_b: self.token_b,
            token_program: anchor_spl::token::ID,
            mint_a: self.mint,
            mint_b: self.quote,
            pool: self.pool,
            authority: self.authority,
            source_a: self.source_a,
            source_b: self.source_b,
            vault_a: self.vault_a,
            vault_b: self.vault_b,
            lp_mint: self.lp_mint,
            user_lp: self.user_lp.pubkey(),
            system_program: solana_sdk_ids::system_program::ID,
        }
    }

    fn trade(&self, f: &DexFixture) -> fossa_dex::contexts::__client_accounts_trade::Trade {
        fossa_dex::contexts::__client_accounts_trade::Trade {
            user: f.payer.pubkey(),
            pool: self.pool,
            authority: self.authority,
            token_a: self.token_a,
            token_b: self.token_b,
            mint_a: self.mint,
            mint_b: self.quote,
            user_a: self.source_a,
            user_b: self.source_b,
            vault_a: self.vault_a,
            vault_b: self.vault_b,
        }
    }

    fn liquidity(
        &self,
        f: &DexFixture,
    ) -> fossa_dex::contexts::__client_accounts_liquidity::Liquidity {
        fossa_dex::contexts::__client_accounts_liquidity::Liquidity {
            user: f.payer.pubkey(),
            global: f.global,
            treasury: f.treasury,
            system_program: solana_sdk_ids::system_program::ID,
            pool: self.pool,
            authority: self.authority,
            token_a: self.token_a,
            token_b: self.token_b,
            token_program: anchor_spl::token::ID,
            mint_a: self.mint,
            mint_b: self.quote,
            user_a: self.source_a,
            user_b: self.source_b,
            vault_a: self.vault_a,
            vault_b: self.vault_b,
            lp_mint: self.lp_mint,
            user_lp: self.user_lp.pubkey(),
        }
    }

    fn swap(
        &self,
        f: &DexFixture,
        amount: u64,
        limit: u64,
        exact_out: bool,
    ) -> solana_instruction::Instruction {
        let args = args::SwapArgs {
            a_to_b: false,
            amount,
            limit,
        };
        if exact_out {
            instruction(
                fossa_dex::ID,
                accounts::SwapExactOut {
                    trade: self.trade(f),
                },
                ix::SwapExactOut { args },
            )
        } else {
            instruction(
                fossa_dex::ID,
                accounts::Swap {
                    trade: self.trade(f),
                },
                ix::Swap { args },
            )
        }
    }
}

fn pool_fixture(
    f: &mut DexFixture,
    quote: Address,
    config: Address,
    public_curve: bool,
) -> PoolFixture {
    let mint = create_mint(&mut f.svm, &f.payer, f.payer.pubkey(), 6);
    let p = pool_for_mint(f, mint, quote, config);
    prepare_mint(
        f,
        &p,
        if public_curve { 0 } else { SUPPLY * 2 },
        p.source_a,
        public_curve.then_some(p.authority),
    );
    p
}

fn pool_for_mint(
    f: &mut DexFixture,
    mint: Address,
    quote: Address,
    config: Address,
) -> PoolFixture {
    let token_a = f.svm.get_account(&mint).unwrap().owner;
    let token_b = f.svm.get_account(&quote).unwrap().owner;
    let pool = pair_address(mint, quote);
    let authority = Address::find_program_address(&[b"authority", pool.as_ref()], &fossa_dex::ID).0;
    let lp_mint = Address::find_program_address(&[b"lp_mint", pool.as_ref()], &fossa_dex::ID).0;
    let source_a = CreateAssociatedTokenAccount::new(&mut f.svm, &f.payer, &mint)
        .token_program_id(&token_a)
        .send()
        .unwrap();
    let source_b = CreateAssociatedTokenAccount::new(&mut f.svm, &f.payer, &quote)
        .token_program_id(&token_b)
        .send()
        .unwrap();
    let vault_a = CreateAssociatedTokenAccount::new(&mut f.svm, &f.payer, &mint)
        .owner(&authority)
        .token_program_id(&token_a)
        .send()
        .unwrap();
    let vault_b = CreateAssociatedTokenAccount::new(&mut f.svm, &f.payer, &quote)
        .owner(&authority)
        .token_program_id(&token_b)
        .send()
        .unwrap();
    PoolFixture {
        mint,
        quote,
        token_a,
        token_b,
        config,
        pool,
        authority,
        source_a,
        source_b,
        vault_a,
        vault_b,
        lp_mint,
        user_lp: Keypair::new(),
    }
}

fn prepare_mint(
    f: &mut DexFixture,
    p: &PoolFixture,
    amount: u64,
    destination: Address,
    authority: Option<Address>,
) {
    use anchor_spl::token_interface::spl_token_2022::instruction as token_ix;
    if amount > 0 {
        let mint_tokens = token_ix::mint_to(
            &p.token_a,
            &p.mint,
            &destination,
            &f.payer.pubkey(),
            &[],
            amount,
        )
        .unwrap();
        send(&mut f.svm, &[mint_tokens], &f.payer, &[]).unwrap();
    }
    let set_authority = token_ix::set_authority(
        &p.token_a,
        &p.mint,
        authority.as_ref(),
        token_ix::AuthorityType::MintTokens,
        &f.payer.pubkey(),
        &[],
    )
    .unwrap();
    send(&mut f.svm, &[set_authority], &f.payer, &[]).unwrap();
}

fn model_curve(f: &mut DexFixture) -> PoolFixture {
    let (quote, config) = (f.quote, f.config);
    let mint = create_mint(&mut f.svm, &f.payer, f.payer.pubkey(), 6);
    let p = pool_for_mint(f, mint, quote, config);
    prepare_model_mint(f, &p);
    mint_to(&mut f.svm, &f.payer, f.quote, p.source_b, 100_000_000_000);
    let before = f.svm.get_balance(&f.treasury).unwrap();
    let create = instruction(
        fossa_dex::ID,
        accounts::InitializeModelCurve {
            launch: p.launch(f),
            council: f.payer.pubkey(),
        },
        ix::InitializeModelCurve {},
    );
    send(&mut f.svm, &[create], &f.payer, &[&p.user_lp]).unwrap();
    assert_eq!(f.svm.get_balance(&f.treasury).unwrap() - before, ACTION_FEE);
    assert_eq!(
        token_account(&f.svm, p.source_a).amount,
        fossa_dex::MODEL_TREASURY_ATOMS
    );
    assert_eq!(
        token_account(&f.svm, p.vault_a).amount,
        fossa_dex::MODEL_CURVE_SUPPLY_ATOMS
    );
    p
}

fn prepare_model_mint(f: &mut DexFixture, p: &PoolFixture) {
    let treasury = anchor_spl::token_interface::spl_token_2022::instruction::mint_to(
        &p.token_a,
        &p.mint,
        &p.source_a,
        &f.payer.pubkey(),
        &[],
        fossa_dex::MODEL_TREASURY_ATOMS,
    )
    .unwrap();
    send(&mut f.svm, &[treasury], &f.payer, &[]).unwrap();
    prepare_mint(f, p, fossa_dex::MODEL_CURVE_SUPPLY_ATOMS, p.vault_a, None);
}

fn graduate(f: &mut DexFixture, p: &PoolFixture) {
    let state: state_types::Pool = account(&f.svm, p.pool);
    if state.reserve_b < state.target_b {
        let buy = p.swap(f, u64::MAX, 1, false);
        send(&mut f.svm, &[buy], &f.payer, &[]).unwrap();
    }
    let graduate = instruction(
        fossa_dex::ID,
        accounts::Graduate {
            pool: p.pool,
            authority: p.authority,
            token_a: p.token_a,
            token_b: p.token_b,
            token_program: anchor_spl::token::ID,
            mint_a: p.mint,
            mint_b: p.quote,
            vault_a: p.vault_a,
            vault_b: p.vault_b,
            lp_mint: p.lp_mint,
            operator: f.payer.pubkey(),
        },
        ix::Graduate {},
    );
    send(&mut f.svm, &[graduate], &f.payer, &[]).unwrap();
    let state: state_types::Pool = account(&f.svm, p.pool);
    assert!(state.locked_lp.get() > 0);
    assert_eq!(state.status, 1);
}

fn approve_quote(f: &mut DexFixture, p: &PoolFixture) -> Address {
    let config = Address::find_program_address(&[b"config", p.mint.as_ref()], &fossa_dex::ID).0;
    let create = instruction(
        fossa_dex::ID,
        accounts::InitializeConfig {
            global: f.global,
            council: f.payer.pubkey(),
            token: p.token_a,
            mint: p.mint,
            reference_pool: Some(p.pool),
            config,
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::InitializeConfig {
            config: args::ConfigInit {
                treasury: f.treasury,
                rates: args::Rates {
                    trade_bps: 125,
                    protocol_share: 10_000,
                    creator_share: 0,
                },
                distributor: Some(f.distributor),
            },
        },
    );
    send(&mut f.svm, &[create], &f.payer, &[]).unwrap();
    config
}

fn refresh(config: Address, reference_pool: Address) -> solana_instruction::Instruction {
    instruction(
        fossa_dex::ID,
        accounts::RefreshQuote {
            config,
            reference_pool,
        },
        ix::RefreshQuote {},
    )
}

fn public_launch(f: &DexFixture, p: &PoolFixture) -> solana_instruction::Instruction {
    instruction(
        fossa_dex::ID,
        accounts::InitializeCurve {
            launch: p.launch(f),
        },
        ix::InitializeCurve {
            config: args::CurveInit {
                mode: 0,
                tax_bps: 0,
            },
        },
    )
}

#[test]
fn council_can_seed_trade_and_reseed_wrapped_sol_without_model_markets() {
    let mut f = setup_dex();
    litesvm_token::create_native_mint(&mut f.svm);
    let (quote, config) = (f.quote, f.config);
    let p = pool_for_mint(
        &mut f,
        litesvm_token::spl_token::native_mint::ID,
        quote,
        config,
    );
    f.svm.airdrop(&p.source_a, 10_000_000_000).unwrap();
    litesvm_token::SyncNative::new(&mut f.svm, &f.payer, &p.source_a)
        .send()
        .unwrap();
    mint_to(&mut f.svm, &f.payer, quote, p.source_b, 1_000_000_000);
    let create = |council| {
        instruction(
            fossa_dex::ID,
            accounts::InitializePool {
                launch: p.launch(&f),
                council,
            },
            ix::InitializePool {
                config: args::PoolInit {
                    amount_a: 1_000_000_000,
                    amount_b: 100_000_000,
                    minimum_lp: 1,
                },
            },
        )
    };
    let missing_council = create(None);
    let wrong_council = Keypair::new();
    let unauthorized = create(Some(wrong_council.pubkey()));
    let authorized = create(Some(f.payer.pubkey()));
    assert!(send(&mut f.svm, &[missing_council], &f.payer, &[&p.user_lp]).is_err());
    assert!(send(
        &mut f.svm,
        &[unauthorized],
        &f.payer,
        &[&p.user_lp, &wrong_council]
    )
    .is_err());
    assert!(f.svm.get_account(&p.pool).is_none());
    send(&mut f.svm, &[authorized], &f.payer, &[&p.user_lp]).unwrap();
    let sell = instruction(
        fossa_dex::ID,
        accounts::Swap { trade: p.trade(&f) },
        ix::Swap {
            args: args::SwapArgs {
                a_to_b: true,
                amount: 100_000_000,
                limit: 1,
            },
        },
    );
    send(&mut f.svm, &[sell], &f.payer, &[]).unwrap();
    let buy = p.swap(&f, 1_000_000, 1_000_000, true);
    send(&mut f.svm, &[buy], &f.payer, &[]).unwrap();
    let initial_lp = token_account(&f.svm, p.user_lp.pubkey()).amount;
    let add = instruction(
        fossa_dex::ID,
        accounts::AddLiquidity {
            liquidity: p.liquidity(&f),
        },
        ix::AddLiquidity {
            args: args::LiquidityArgs {
                amount: initial_lp / 10,
                bound_a: u64::MAX,
                bound_b: u64::MAX,
            },
        },
    );
    send(&mut f.svm, &[add], &f.payer, &[]).unwrap();
    let remove = instruction(
        fossa_dex::ID,
        accounts::RemoveLiquidity {
            liquidity: p.liquidity(&f),
        },
        ix::RemoveLiquidity {
            args: args::LiquidityArgs {
                amount: token_account(&f.svm, p.user_lp.pubkey()).amount,
                bound_a: 1,
                bound_b: 1,
            },
        },
    );
    send(&mut f.svm, &[remove], &f.payer, &[]).unwrap();
    let empty: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!((empty.reserve_a.get(), empty.reserve_b.get()), (0, 0));
    assert_eq!(token_account(&f.svm, p.user_lp.pubkey()).amount, 0);
    let native = token_account(&f.svm, p.vault_a);
    assert_eq!(native.amount, empty.protocol_a.get());
    assert_eq!(
        f.svm.get_balance(&p.vault_a).unwrap(),
        native.is_native.unwrap() + native.amount
    );
    let reseed = instruction(
        fossa_dex::ID,
        accounts::AddLiquidity {
            liquidity: p.liquidity(&f),
        },
        ix::AddLiquidity {
            args: args::LiquidityArgs {
                amount: 100_000,
                bound_a: 100_000,
                bound_b: 100_000,
            },
        },
    );
    send(&mut f.svm, &[reseed], &f.payer, &[]).unwrap();
    let state: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(
        (state.reserve_a.get(), state.reserve_b.get()),
        (100_000, 100_000)
    );
}

#[test]
fn council_only_quote_configuration_cannot_enable_public_launches() {
    let mut f = setup_dex();
    let quote = create_mint(&mut f.svm, &f.payer, f.payer.pubkey(), 6);
    let config = Address::find_program_address(&[b"config", quote.as_ref()], &fossa_dex::ID).0;
    let configure = instruction(
        fossa_dex::ID,
        accounts::InitializeConfig {
            global: f.global,
            council: f.payer.pubkey(),
            token: anchor_spl::token::ID,
            mint: quote,
            reference_pool: None,
            config,
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::InitializeConfig {
            config: args::ConfigInit {
                treasury: f.treasury,
                rates: args::Rates {
                    trade_bps: 125,
                    protocol_share: 10_000,
                    creator_share: 0,
                },
                distributor: Some(f.distributor),
            },
        },
    );
    send(&mut f.svm, &[configure], &f.payer, &[]).unwrap();
    let state: state_types::Config = account(&f.svm, config);
    assert_eq!(state.reference_pool, Address::default());
    let p = pool_fixture(&mut f, quote, config, false);
    mint_to(&mut f.svm, &f.payer, quote, p.source_b, 9_000_000);
    let create = |council| {
        instruction(
            fossa_dex::ID,
            accounts::InitializePool {
                launch: p.launch(&f),
                council,
            },
            ix::InitializePool {
                config: args::PoolInit {
                    amount_a: 4_000_000,
                    amount_b: 9_000_000,
                    minimum_lp: 6_000_000,
                },
            },
        )
    };
    let public = create(None);
    let authorized = create(Some(f.payer.pubkey()));
    assert!(send(&mut f.svm, &[public], &f.payer, &[&p.user_lp]).is_err());
    assert!(f.svm.get_account(&p.pool).is_none());
    send(&mut f.svm, &[authorized], &f.payer, &[&p.user_lp]).unwrap();
    let public_curve = pool_fixture(&mut f, quote, config, true);
    let create_curve = public_launch(&f, &public_curve);
    assert!(send(
        &mut f.svm,
        &[create_curve],
        &f.payer,
        &[&public_curve.user_lp]
    )
    .is_err());
    assert!(f.svm.get_account(&public_curve.pool).is_none());
}

#[test]
fn direct_pool_fees_full_withdrawal_reseed_and_pair_identity() {
    let mut f = setup_dex();
    let (quote, config) = (f.quote, f.config);
    let p = pool_fixture(&mut f, quote, config, false);
    mint_to(&mut f.svm, &f.payer, f.quote, p.source_b, 20_000_000);
    let create = instruction(
        fossa_dex::ID,
        accounts::InitializePool {
            launch: p.launch(&f),
            council: None,
        },
        ix::InitializePool {
            config: args::PoolInit {
                amount_a: 4_000_000,
                amount_b: 9_000_000,
                minimum_lp: 6_000_000,
            },
        },
    );
    assert!(send(&mut f.svm, &[create], &f.payer, &[&p.user_lp]).is_err());
    assert!(f.svm.get_account(&p.pool).is_none());
    let before = f.svm.get_balance(&f.treasury).unwrap();
    let create = instruction(
        fossa_dex::ID,
        accounts::InitializePool {
            launch: p.launch(&f),
            council: Some(f.payer.pubkey()),
        },
        ix::InitializePool {
            config: args::PoolInit {
                amount_a: 4_000_000,
                amount_b: 9_000_000,
                minimum_lp: 6_000_000,
            },
        },
    );
    send(&mut f.svm, &[create], &f.payer, &[&p.user_lp]).unwrap();
    assert_eq!(f.svm.get_balance(&f.treasury).unwrap() - before, ACTION_FEE);
    assert_eq!(pair_address(p.mint, p.quote), pair_address(p.quote, p.mint));
    let state: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(state.locked_lp.get(), 0);
    let remove = instruction(
        fossa_dex::ID,
        accounts::RemoveLiquidity {
            liquidity: p.liquidity(&f),
        },
        ix::RemoveLiquidity {
            args: args::LiquidityArgs {
                amount: 6_000_000,
                bound_a: 4_000_000,
                bound_b: 9_000_000,
            },
        },
    );
    send(&mut f.svm, &[remove], &f.payer, &[]).unwrap();
    let state: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!((state.reserve_a.get(), state.reserve_b.get()), (0, 0));
    assert_eq!(token_account(&f.svm, p.user_lp.pubkey()).amount, 0);
    let reseed = instruction(
        fossa_dex::ID,
        accounts::AddLiquidity {
            liquidity: p.liquidity(&f),
        },
        ix::AddLiquidity {
            args: args::LiquidityArgs {
                amount: 6_000_000,
                bound_a: 4_000_000,
                bound_b: 9_000_000,
            },
        },
    );
    send(&mut f.svm, &[reseed], &f.payer, &[]).unwrap();
    assert_eq!(
        f.svm.get_balance(&f.treasury).unwrap() - before,
        3 * ACTION_FEE
    );
    assert_eq!(token_account(&f.svm, p.user_lp.pubkey()).amount, 6_000_000);
    let other_quote = create_mint(&mut f.svm, &f.payer, f.payer.pubkey(), 6);
    assert_ne!(p.pool, pair_address(p.mint, other_quote));
}

#[test]
fn fixed_model_curve_validates_inventory_and_graduates_at_twice_start_price() {
    let mut f = setup_dex();
    let wrong_payer = Keypair::new();
    f.svm.airdrop(&wrong_payer.pubkey(), 100_000_000).unwrap();
    for (decimals, amount, wrong_owner) in [
        (9, fossa_dex::MODEL_CURVE_SUPPLY_ATOMS, false),
        (6, fossa_dex::MODEL_CURVE_SUPPLY_ATOMS - 1, false),
        (6, fossa_dex::MODEL_CURVE_SUPPLY_ATOMS, true),
    ] {
        let mint = create_mint(&mut f.svm, &f.payer, f.payer.pubkey(), decimals);
        let (quote, config) = (f.quote, f.config);
        let p = pool_for_mint(&mut f, mint, quote, config);
        prepare_mint(&mut f, &p, fossa_dex::MODEL_SUPPLY_ATOMS, p.source_a, None);
        let transfer = anchor_spl::token_interface::spl_token_2022::instruction::transfer_checked(
            &p.token_a,
            &p.source_a,
            &mint,
            &p.vault_a,
            &f.payer.pubkey(),
            &[],
            amount,
            decimals,
        )
        .unwrap();
        send(&mut f.svm, &[transfer], &f.payer, &[]).unwrap();
        let mut launch = p.launch(&f);
        let mut signers = vec![&p.user_lp];
        if wrong_owner {
            use anchor_spl::token_interface::spl_token_2022::instruction as token_ix;
            let change_owner = token_ix::set_authority(
                &p.token_a,
                &p.source_a,
                Some(&wrong_payer.pubkey()),
                token_ix::AuthorityType::AccountOwner,
                &f.payer.pubkey(),
                &[],
            )
            .unwrap();
            send(&mut f.svm, &[change_owner], &f.payer, &[]).unwrap();
            launch.payer = wrong_payer.pubkey();
            launch.source_b =
                create_token_account(&mut f.svm, &f.payer, f.quote, wrong_payer.pubkey());
            signers.push(&wrong_payer);
        }
        let create = instruction(
            fossa_dex::ID,
            accounts::InitializeModelCurve {
                launch,
                council: f.payer.pubkey(),
            },
            ix::InitializeModelCurve {},
        );
        let treasury = f.svm.get_balance(&f.treasury).unwrap();
        assert!(send(&mut f.svm, &[create], &f.payer, &signers).is_err());
        assert!(f.svm.get_account(&p.pool).is_none());
        assert_eq!(f.svm.get_balance(&f.treasury).unwrap(), treasury);
    }
    let p = model_curve(&mut f);
    let initial: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(initial.initial_base.get(), 90_000_000_000_000);
    assert_eq!(initial.target_b.get(), 50_000_000_000);
    let sell = instruction(
        fossa_dex::ID,
        accounts::Swap { trade: p.trade(&f) },
        ix::Swap {
            args: args::SwapArgs {
                a_to_b: true,
                amount: fossa_dex::MODEL_TREASURY_ATOMS,
                limit: 1,
            },
        },
    );
    assert!(send(&mut f.svm, &[sell.clone()], &f.payer, &[]).is_err());
    assert_eq!(
        token_account(&f.svm, p.source_a).amount,
        fossa_dex::MODEL_TREASURY_ATOMS
    );
    let normal = p.swap(&f, 10_000_000_000, 1, false);
    let result = send(&mut f.svm, &[normal], &f.payer, &[]).unwrap();
    eprintln!("curve normal buy: {} cu", result.compute_units_consumed);
    let before_sell = token_account(&f.svm, p.source_a).amount;
    send(&mut f.svm, &[sell], &f.payer, &[]).unwrap();
    assert_eq!(
        before_sell - token_account(&f.svm, p.source_a).amount,
        fossa_dex::MODEL_TREASURY_ATOMS
    );
    let after_sell: state_types::Pool = account(&f.svm, p.pool);
    assert!(after_sell.reserve_b.get() > 0 && after_sell.reserve_b.get() < 10_000_000_000);
    let before = token_account(&f.svm, p.source_b).amount;
    let clipped = p.swap(&f, u64::MAX, 1, false);
    let result = send(&mut f.svm, &[clipped], &f.payer, &[]).unwrap();
    eprintln!("curve clipped buy: {} cu", result.compute_units_consumed);
    let after = token_account(&f.svm, p.source_b).amount;
    let state: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(state.reserve_b.get(), 50_000_000_000);
    assert!(before - after < 51_000_000_000 && after > 0);
    assert_eq!(
        token_account(&f.svm, p.vault_b).amount,
        state.reserve_b.get() + state.protocol_b.get() + state.creator_b.get()
    );
    assert_eq!(
        token_account(&f.svm, p.vault_a).amount,
        state.reserve_a.get() + state.protocol_a.get() + state.creator_a.get()
    );
    let tokens_before_graduation = token_account(&f.svm, p.source_a).amount;
    graduate(&mut f, &p);
    assert_eq!(
        token_account(&f.svm, p.source_a).amount,
        tokens_before_graduation
    );
    let amm: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(amm.reserve_b.get(), 50_000_000_000);
    let actual = u128::from(amm.reserve_b.get())
        * u128::from(initial.reserve_a.get() + initial.virtual_a.get());
    let twice_start = 2 * u128::from(initial.virtual_b.get()) * u128::from(amm.reserve_a.get());
    assert!(actual.abs_diff(twice_start) <= twice_start / 1_000_000_000);
}

#[test]
fn oracle_requires_graduation_history_and_resists_refresh_spam_and_atomic_spot() {
    let mut f = setup_dex();
    advance(&mut f.svm, 10_000);
    let reference = model_curve(&mut f);
    graduate(&mut f, &reference);
    let quote_config = approve_quote(&mut f, &reference);
    let start: state_types::Config = account(&f.svm, quote_config);
    let p = pool_fixture(&mut f, reference.mint, quote_config, true);
    let create = public_launch(&f, &p);
    assert!(send(&mut f.svm, &[create.clone()], &f.payer, &[&p.user_lp]).is_err());
    for _ in 0..3 {
        advance(&mut f.svm, 500);
        let _ = send(
            &mut f.svm,
            &[refresh(quote_config, reference.pool)],
            &f.payer,
            &[],
        );
        let state: state_types::Config = account(&f.svm, quote_config);
        assert_eq!(state.oracle_timestamp, start.oracle_timestamp);
        assert_eq!(state.quote_per_usdc_x32.get(), 0);
    }
    advance(&mut f.svm, 300);
    let before: state_types::Pool = account(&f.svm, reference.pool);
    let expected =
        (u128::from(before.reserve_a.get()) << 32).div_ceil(u128::from(before.reserve_b.get()));
    let attacker = reference.swap(&f, 100_000, 1, false);
    send(
        &mut f.svm,
        &[attacker, refresh(quote_config, reference.pool)],
        &f.payer,
        &[],
    )
    .unwrap();
    let quoted: state_types::Config = account(&f.svm, quote_config);
    assert_eq!(quoted.quote_per_usdc_x32.get(), expected);
    assert_eq!(quoted.quote_price_timestamp.get(), now(&f.svm));
    let bad_mint = create_mint(&mut f.svm, &f.payer, f.payer.pubkey(), 9);
    let bad_decimals = pool_for_mint(&mut f, bad_mint, reference.mint, quote_config);
    prepare_mint(
        &mut f,
        &bad_decimals,
        0,
        bad_decimals.source_a,
        Some(bad_decimals.authority),
    );
    let rejected = public_launch(&f, &bad_decimals);
    assert!(send(&mut f.svm, &[rejected], &f.payer, &[&bad_decimals.user_lp]).is_err());
    assert!(f.svm.get_account(&bad_decimals.pool).is_none());
    let mut unauthorized_pair = p.launch(&f);
    unauthorized_pair.mint_a = f.quote;
    unauthorized_pair.mint_b = reference.mint;
    unauthorized_pair.pool = reference.pool;
    unauthorized_pair.authority = reference.authority;
    unauthorized_pair.source_a = reference.source_b;
    unauthorized_pair.source_b = reference.source_a;
    unauthorized_pair.vault_a = reference.vault_b;
    unauthorized_pair.vault_b = reference.vault_a;
    unauthorized_pair.lp_mint = reference.lp_mint;
    let reversed = instruction(
        fossa_dex::ID,
        accounts::InitializePool {
            launch: unauthorized_pair,
            council: Some(f.payer.pubkey()),
        },
        ix::InitializePool {
            config: args::PoolInit {
                amount_a: 1_000_000,
                amount_b: 1_000_000,
                minimum_lp: 1,
            },
        },
    );
    assert!(send(&mut f.svm, &[reversed], &f.payer, &[&p.user_lp]).is_err());
    let treasury_before = f.svm.get_balance(&f.treasury).unwrap();
    send(&mut f.svm, &[create], &f.payer, &[&p.user_lp]).unwrap();
    assert_eq!(
        f.svm.get_balance(&f.treasury).unwrap() - treasury_before,
        ACTION_FEE
    );
    let launched: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(
        launched.target_b.get(),
        (u128::from(fossa_dex::PUBLIC_RAISE_USDC_ATOMS) * expected).div_ceil(1u128 << 32) as u64
    );
    assert_eq!(launched.initial_base.get(), fossa_dex::PUBLIC_SUPPLY_ATOMS);
    assert_eq!(launched.minimum_a.get(), fossa_dex::PUBLIC_LIQUIDITY_ATOMS);
    let before = token_account(&f.svm, p.source_b).amount;
    for exact_out in [false, true] {
        let buy = p.swap(
            &f,
            if exact_out {
                fossa_dex::PUBLIC_SUPPLY_ATOMS * 3 / 4 + 1
            } else {
                u64::MAX
            },
            if exact_out { u64::MAX } else { 1 },
            exact_out,
        );
        assert!(send(&mut f.svm, &[buy], &f.payer, &[]).is_err());
        assert_eq!(token_account(&f.svm, p.source_b).amount, before);
    }
    let first_buy = p.swap(&f, 600_000_000_000_000, u64::MAX, true);
    send(&mut f.svm, &[first_buy], &f.payer, &[]).unwrap();
    let last_buy = p.swap(&f, u64::MAX, 1, false);
    send(&mut f.svm, &[last_buy], &f.payer, &[]).unwrap();
    let terminal: state_types::Pool = account(&f.svm, p.pool);
    assert!(terminal.reserve_a.get() >= fossa_dex::PUBLIC_LIQUIDITY_ATOMS);
    assert_eq!(terminal.reserve_b, terminal.target_b);
    graduate(&mut f, &p);
    let graduated: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(graduated.reserve_b, terminal.target_b);
    assert!(graduated.reserve_a <= terminal.reserve_a);
    let quote = u128::from(terminal.reserve_b.get() + terminal.virtual_b.get());
    let before_price = u128::from(terminal.reserve_b.get())
        * u128::from(terminal.reserve_a.get() + terminal.virtual_a.get());
    let after_price = u128::from(graduated.reserve_a.get()) * quote;
    assert!(after_price >= before_price && after_price - before_price < quote);
    let original_council = f.payer;
    f.payer = Keypair::new();
    f.svm.airdrop(&f.payer.pubkey(), 1_000_000_000).unwrap();
    let public = pool_fixture(&mut f, reference.mint, quote_config, true);
    let create = public_launch(&f, &public);
    send(&mut f.svm, &[create], &f.payer, &[&public.user_lp]).unwrap();
    assert_ne!(
        account::<state_types::Pool>(&f.svm, public.pool).creator,
        original_council.pubkey()
    );
    advance(&mut f.svm, 3_601);
    let stale = pool_fixture(&mut f, reference.mint, quote_config, true);
    let create = public_launch(&f, &stale);
    assert!(send(&mut f.svm, &[create], &f.payer, &[&stale.user_lp]).is_err());
    send(
        &mut f.svm,
        &[refresh(quote_config, reference.pool)],
        &f.payer,
        &[],
    )
    .unwrap();
    let reset: state_types::Config = account(&f.svm, quote_config);
    assert_eq!(reset.quote_per_usdc_x32.get(), 0);
    assert_eq!(reset.oracle_timestamp.get(), now(&f.svm));
    let wrong = refresh(quote_config, p.pool);
    assert!(send(&mut f.svm, &[wrong], &f.payer, &[]).is_err());
    advance(&mut f.svm, 1_800);
    send(
        &mut f.svm,
        &[refresh(quote_config, reference.pool)],
        &f.payer,
        &[],
    )
    .unwrap();
    assert!(
        account::<state_types::Config>(&f.svm, quote_config)
            .quote_per_usdc_x32
            .get()
            > 0
    );
}

#[test]
fn clipped_public_buy_preserves_remainder_with_transfer_tax_and_separate_fee_rounding() {
    use anchor_spl::token_interface::spl_token_2022 as token;
    use token::extension::ExtensionType;
    let mut f = setup_dex();
    advance(&mut f.svm, 10_000);
    let mint = Keypair::new().pubkey();
    let len = ExtensionType::try_calculate_account_len::<token::state::Mint>(&[
        ExtensionType::TransferFeeConfig,
    ])
    .unwrap();
    f.svm
        .set_account(
            mint,
            solana_account::Account {
                lamports: f.svm.minimum_balance_for_rent_exemption(len),
                data: vec![0; len],
                owner: anchor_spl::token_2022::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let initialize_fee =
        token::extension::transfer_fee::instruction::initialize_transfer_fee_config(
            &anchor_spl::token_2022::ID,
            &mint,
            None,
            Some(&f.payer.pubkey()),
            100,
            u64::MAX,
        )
        .unwrap();
    let initialize = token::instruction::initialize_mint2(
        &anchor_spl::token_2022::ID,
        &mint,
        &f.payer.pubkey(),
        None,
        6,
    )
    .unwrap();
    send(&mut f.svm, &[initialize_fee, initialize], &f.payer, &[]).unwrap();
    let (quote, config) = (f.quote, f.config);
    let reference = pool_for_mint(&mut f, mint, quote, config);
    prepare_model_mint(&mut f, &reference);
    mint_to(
        &mut f.svm,
        &f.payer,
        f.quote,
        reference.source_b,
        100_000_000_000,
    );
    let create = instruction(
        fossa_dex::ID,
        accounts::InitializeModelCurve {
            launch: reference.launch(&f),
            council: f.payer.pubkey(),
        },
        ix::InitializeModelCurve {},
    );
    send(&mut f.svm, &[create], &f.payer, &[&reference.user_lp]).unwrap();
    graduate(&mut f, &reference);
    let config = approve_quote(&mut f, &reference);
    advance(&mut f.svm, 1_800);
    send(
        &mut f.svm,
        &[refresh(config, reference.pool)],
        &f.payer,
        &[],
    )
    .unwrap();
    let public = pool_fixture(&mut f, mint, config, true);
    let create = public_launch(&f, &public);
    send(&mut f.svm, &[create], &f.payer, &[&public.user_lp]).unwrap();
    let oversized = public.swap(&f, u64::MAX, 1, false);
    assert!(send(&mut f.svm, &[oversized], &f.payer, &[]).is_err());
    let first_buy = public.swap(&f, 600_000_000_000_000, u64::MAX, true);
    send(&mut f.svm, &[first_buy], &f.payer, &[]).unwrap();
    let previous: state_types::Pool = account(&f.svm, public.pool);
    let before = token_account(&f.svm, public.source_b).amount;
    let buy = public.swap(&f, u64::MAX, 1, false);
    let result = send(&mut f.svm, &[buy], &f.payer, &[]).unwrap();
    eprintln!(
        "curve clipped taxed buy: {} cu",
        result.compute_units_consumed
    );
    let state: state_types::Pool = account(&f.svm, public.pool);
    let spent = before - token_account(&f.svm, public.source_b).amount;
    let received = spent - spent.div_ceil(100);
    assert!(spent < before && token_account(&f.svm, public.source_b).amount > 0);
    assert_eq!(state.reserve_b, state.target_b);
    assert!(state.reserve_a.get() >= fossa_dex::PUBLIC_LIQUIDITY_ATOMS);
    assert_eq!(
        state.protocol_b.get() - previous.protocol_b.get(),
        (received * 125).div_ceil(10_000)
    );
    assert_eq!(
        state.creator_b.get() - previous.creator_b.get(),
        (received * 50).div_ceil(10_000)
    );
    assert_eq!(
        received,
        state.reserve_b.get() - previous.reserve_b.get() + state.protocol_b.get()
            - previous.protocol_b.get()
            + state.creator_b.get()
            - previous.creator_b.get()
    );
}
