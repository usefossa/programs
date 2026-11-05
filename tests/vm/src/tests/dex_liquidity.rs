use super::*;
use anchor_spl::token_interface::spl_token_2022::instruction as token_ix;

fn direct_pool(f: &mut DexFixture, reserve_a: u64, reserve_b: u64) -> PoolFixture {
    let (quote, config) = (f.quote, f.config);
    let p = pool_fixture(f, quote, config, false);
    mint_to(&mut f.svm, &f.payer, quote, p.source_b, 2_000_000_000);
    let create = instruction(
        fossa_dex::ID,
        accounts::InitializePool {
            launch: p.launch(f),
            council: Some(f.payer.pubkey()),
        },
        ix::InitializePool {
            config: args::PoolInit {
                amount_a: reserve_a,
                amount_b: reserve_b,
                minimum_lp: 1,
            },
        },
    );
    send(&mut f.svm, &[create], &f.payer, &[&p.user_lp]).unwrap();
    p
}

fn liquidity(
    accounts: fossa_dex::contexts::__client_accounts_liquidity::Liquidity,
    amount: u64,
    bounds: [u64; 2],
    remove: bool,
) -> solana_instruction::Instruction {
    let args = args::LiquidityArgs {
        amount,
        bound_a: bounds[0],
        bound_b: bounds[1],
    };
    if remove {
        instruction(
            fossa_dex::ID,
            accounts::RemoveLiquidity {
                liquidity: accounts,
            },
            ix::RemoveLiquidity { args },
        )
    } else {
        instruction(
            fossa_dex::ID,
            accounts::AddLiquidity {
                liquidity: accounts,
            },
            ix::AddLiquidity { args },
        )
    }
}

fn burn_lp(f: &mut DexFixture, p: &PoolFixture, owner: &Keypair, source: Address) {
    let amount = token_account(&f.svm, source).amount;
    let burn = token_ix::burn(
        &anchor_spl::token::ID,
        &source,
        &p.lp_mint,
        &owner.pubkey(),
        &[],
        amount,
    )
    .unwrap();
    send(&mut f.svm, &[burn], owner, &[]).unwrap();
    let mint: litesvm_token::spl_token::state::Mint =
        litesvm_token::get_spl_account(&f.svm, &p.lp_mint).unwrap();
    assert_eq!(mint.supply, 0);
}

fn snapshot(svm: &LiteSVM, addresses: &[Address]) -> Vec<solana_account::Account> {
    addresses
        .iter()
        .map(|key| svm.get_account(key).unwrap())
        .collect()
}

#[test]
fn external_lp_burn_allows_unrelated_depositor_without_capturing_orphan_reserves() {
    let mut f = setup_dex();
    let p = direct_pool(&mut f, 100, 100);
    let founder = f.payer.insecure_clone();
    burn_lp(&mut f, &p, &founder, p.user_lp.pubkey());
    let swap = p.swap(&f, 20, 1, false);
    send(&mut f.svm, &[swap], &f.payer, &[]).unwrap();
    let orphan: state_types::Pool = account(&f.svm, p.pool);
    assert!(orphan.protocol_b.get() > 0);
    assert_eq!(orphan.locked_lp.get(), 0);
    let reserves = [orphan.reserve_a.get(), orphan.reserve_b.get()];
    let locked = fossa_dex::math::sqrt(u128::from(reserves[0]) * u128::from(reserves[1]));

    let depositor = Keypair::new();
    f.svm.airdrop(&depositor.pubkey(), 1_000_000_000).unwrap();
    let user_a = create_token_account(&mut f.svm, &f.payer, p.mint, depositor.pubkey());
    let user_b = create_token_account(&mut f.svm, &f.payer, p.quote, depositor.pubkey());
    let user_lp = create_token_account(&mut f.svm, &f.payer, p.lp_mint, depositor.pubkey());
    for (mint, source, destination) in [
        (p.mint, p.source_a, user_a),
        (p.quote, p.source_b, user_b),
        (p.mint, p.source_a, p.vault_a),
    ] {
        let transfer = token_ix::transfer_checked(
            &anchor_spl::token::ID,
            &source,
            &mint,
            &destination,
            &f.payer.pubkey(),
            &[],
            1_000,
            6,
        )
        .unwrap();
        send(&mut f.svm, &[transfer], &f.payer, &[]).unwrap();
    }
    let accounts = || {
        let mut accounts = p.liquidity(&f);
        accounts.user = depositor.pubkey();
        accounts.user_a = user_a;
        accounts.user_b = user_b;
        accounts.user_lp = user_lp;
        accounts
    };
    let remove_without_lp = liquidity(accounts(), 1, [0, 0], true);
    let bound_a_failure = liquidity(accounts(), locked, [reserves[0] - 1, reserves[1]], false);
    let bound_b_failure = liquidity(accounts(), locked, [reserves[0], reserves[1] - 1], false);
    let zero_failure = liquidity(accounts(), 0, reserves, false);
    let overflow_failure = liquidity(accounts(), u64::MAX, [u64::MAX; 2], false);
    let deposit = liquidity(accounts(), locked, reserves, false);
    let withdraw = liquidity(accounts(), locked, reserves, true);
    let watched = [
        p.pool, p.lp_mint, p.vault_a, p.vault_b, user_a, user_b, user_lp, f.treasury,
    ];
    let before = snapshot(&f.svm, &watched);
    for rejected in [
        remove_without_lp,
        bound_a_failure,
        bound_b_failure,
        zero_failure,
        overflow_failure,
    ] {
        assert!(send(&mut f.svm, &[rejected], &depositor, &[]).is_err());
        assert_eq!(snapshot(&f.svm, &watched), before);
    }
    let mut poor = f.svm.get_account(&depositor.pubkey()).unwrap();
    poor.lamports = 1_000_000;
    f.svm.set_account(depositor.pubkey(), poor).unwrap();
    assert!(send(&mut f.svm, &[deposit.clone()], &depositor, &[]).is_err());
    assert_eq!(snapshot(&f.svm, &watched), before);
    f.svm.airdrop(&depositor.pubkey(), 1_000_000_000).unwrap();

    send(&mut f.svm, &[deposit.clone()], &depositor, &[]).unwrap();
    let recovered: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(recovered.locked_lp.get(), locked);
    assert_eq!(recovered.reserve_a.get(), reserves[0] * 2);
    assert_eq!(recovered.reserve_b.get(), reserves[1] * 2);
    assert_eq!(recovered.protocol_b, orphan.protocol_b);
    assert_eq!(recovered.mode, orphan.mode);
    assert_eq!(token_account(&f.svm, user_lp).amount, locked);
    assert_eq!(
        token_account(&f.svm, p.vault_a).amount,
        recovered.reserve_a.get() + 1_000
    );
    send(&mut f.svm, &[withdraw.clone()], &depositor, &[]).unwrap();
    let remaining: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(
        [remaining.reserve_a.get(), remaining.reserve_b.get()],
        reserves
    );
    assert_eq!(remaining.locked_lp.get(), locked);
    assert_eq!(token_account(&f.svm, user_a).amount, 1_000);
    assert_eq!(token_account(&f.svm, user_b).amount, 1_000);
    assert_eq!(
        token_account(&f.svm, p.vault_b).amount,
        reserves[1] + orphan.protocol_b.get()
    );

    send(&mut f.svm, &[deposit], &depositor, &[]).unwrap();
    burn_lp(&mut f, &p, &depositor, user_lp);
    let swap = p.swap(&f, 20, 1, false);
    send(&mut f.svm, &[swap], &f.payer, &[]).unwrap();
    let donated: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(donated.locked_lp.get(), locked);
    let mut accounts = p.liquidity(&f);
    accounts.user = depositor.pubkey();
    accounts.user_a = user_a;
    accounts.user_b = user_b;
    accounts.user_lp = user_lp;
    let deposit = liquidity(accounts, locked, [u64::MAX; 2], false);
    send(&mut f.svm, &[deposit], &depositor, &[]).unwrap();
    let repeated: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(repeated.locked_lp.get(), locked);
    assert_eq!(repeated.reserve_a.get(), donated.reserve_a.get() * 2);
    assert_eq!(repeated.reserve_b.get(), donated.reserve_b.get() * 2);
    send(&mut f.svm, &[withdraw], &depositor, &[]).unwrap();
    let remaining: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(remaining.reserve_a, donated.reserve_a);
    assert_eq!(remaining.reserve_b, donated.reserve_b);
    assert_eq!(remaining.protocol_b, donated.protocol_b);
}

#[test]
fn orphan_lp_recovery_rounding_preserves_tiny_imbalanced_reserves() {
    for (reserve_a, reserve_b) in [(1, 2), (2, 17), (17, 2), (1, 1_000_000_000)] {
        let mut f = setup_dex();
        let p = direct_pool(&mut f, reserve_a, reserve_b);
        let owner = f.payer.insecure_clone();
        burn_lp(&mut f, &p, &owner, p.user_lp.pubkey());
        let locked = fossa_dex::math::sqrt(u128::from(reserve_a) * u128::from(reserve_b));
        let required = [reserve_a.div_ceil(locked), reserve_b.div_ceil(locked)];
        let add = liquidity(p.liquidity(&f), 1, required, false);
        send(&mut f.svm, &[add], &f.payer, &[]).unwrap();
        let state: state_types::Pool = account(&f.svm, p.pool);
        assert_eq!(state.locked_lp.get(), locked);
        let before = [
            token_account(&f.svm, p.source_a).amount,
            token_account(&f.svm, p.source_b).amount,
        ];
        let remove = liquidity(p.liquidity(&f), 1, [0, 0], true);
        let result = send(&mut f.svm, &[remove], &f.payer, &[]);
        if state.reserve_a.get() / (locked + 1) == 0 || state.reserve_b.get() / (locked + 1) == 0 {
            assert!(result.is_err());
        } else {
            result.unwrap();
            assert!(token_account(&f.svm, p.source_a).amount - before[0] <= required[0]);
            assert!(token_account(&f.svm, p.source_b).amount - before[1] <= required[1]);
        }
        let remaining: state_types::Pool = account(&f.svm, p.pool);
        assert!(remaining.reserve_a.get() >= reserve_a);
        assert!(remaining.reserve_b.get() >= reserve_b);
        assert_eq!(remaining.locked_lp.get(), locked);
    }
}

#[test]
fn graduated_founding_lock_survives_external_burns_and_last_holder_withdrawal() {
    let mut f = setup_dex();
    let p = model_curve(&mut f);
    graduate(&mut f, &p);
    let founding: state_types::Pool = account(&f.svm, p.pool);
    let locked = founding.locked_lp.get();
    let amount = locked / 100;
    let add = liquidity(p.liquidity(&f), amount, [u64::MAX; 2], false);
    send(&mut f.svm, &[add.clone()], &f.payer, &[]).unwrap();
    let owner = f.payer.insecure_clone();
    burn_lp(&mut f, &p, &owner, p.user_lp.pubkey());
    let orphan: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(orphan.locked_lp.get(), locked);
    send(&mut f.svm, &[add], &f.payer, &[]).unwrap();
    let remove = liquidity(p.liquidity(&f), amount, [1, 1], true);
    send(&mut f.svm, &[remove], &f.payer, &[]).unwrap();
    let remaining: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(remaining.locked_lp.get(), locked);
    assert!(remaining.reserve_a >= orphan.reserve_a);
    assert!(remaining.reserve_b >= orphan.reserve_b);
    assert_eq!(token_account(&f.svm, p.user_lp.pubkey()).amount, 0);
}

#[test]
fn partial_external_burn_keeps_direct_pool_fully_withdrawable() {
    let mut f = setup_dex();
    let p = direct_pool(&mut f, 100, 100);
    let burn = token_ix::burn(
        &anchor_spl::token::ID,
        &p.user_lp.pubkey(),
        &p.lp_mint,
        &f.payer.pubkey(),
        &[],
        99,
    )
    .unwrap();
    send(&mut f.svm, &[burn], &f.payer, &[]).unwrap();
    let add = liquidity(p.liquidity(&f), 1, [100, 100], false);
    send(&mut f.svm, &[add], &f.payer, &[]).unwrap();
    let remove = liquidity(p.liquidity(&f), 2, [200, 200], true);
    send(&mut f.svm, &[remove], &f.payer, &[]).unwrap();
    let empty: state_types::Pool = account(&f.svm, p.pool);
    assert_eq!(
        (
            empty.reserve_a.get(),
            empty.reserve_b.get(),
            empty.locked_lp.get()
        ),
        (0, 0, 0)
    );
    assert_eq!(token_account(&f.svm, p.user_lp.pubkey()).amount, 0);
}
