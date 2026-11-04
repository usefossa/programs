use super::*;

fn taxed_mint(f: &mut DexFixture, bps: u16) -> Address {
    use anchor_spl::token_interface::spl_token_2022 as token;
    let mint = Keypair::new().pubkey();
    let len = token::extension::ExtensionType::try_calculate_account_len::<token::state::Mint>(&[
        token::extension::ExtensionType::TransferFeeConfig,
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
    let fee = token::extension::transfer_fee::instruction::initialize_transfer_fee_config(
        &anchor_spl::token_2022::ID,
        &mint,
        None,
        Some(&f.payer.pubkey()),
        bps,
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
    send(&mut f.svm, &[fee, initialize], &f.payer, &[]).unwrap();
    mint
}

fn funded_pool(f: &mut DexFixture, quote: Address, config: Address, taxed: bool) -> PoolFixture {
    use anchor_spl::token_interface::spl_token_2022::instruction as token;
    let mint = if taxed {
        taxed_mint(f, 300)
    } else {
        create_mint(&mut f.svm, &f.payer, f.payer.pubkey(), 6)
    };
    let p = pool_for_mint(f, mint, quote, config);
    prepare_mint(f, &p, 10_000_000_000_000, p.source_a, None);
    let mint_quote = token::mint_to(
        &p.token_b,
        &quote,
        &p.source_b,
        &f.payer.pubkey(),
        &[],
        10_000_000_000_000,
    )
    .unwrap();
    send(&mut f.svm, &[mint_quote], &f.payer, &[]).unwrap();
    let initialize = instruction(
        fossa_dex::ID,
        accounts::InitializePool {
            launch: p.launch(f),
            council: Some(f.payer.pubkey()),
        },
        ix::InitializePool {
            config: args::PoolInit {
                amount_a: 1_000_000_000,
                amount_b: 2_000_000_000,
                minimum_lp: 1,
            },
        },
    );
    let measured = send(&mut f.svm, &[initialize], &f.payer, &[&p.user_lp])
        .unwrap()
        .compute_units_consumed;
    assert!(measured <= 55_000, "initialize pool: {measured}");
    p
}

#[test]
fn native_compute_profiles_cover_taxed_trades_liquidity_and_two_hop_routes() {
    for taxed_quote in [false, true] {
        for taxed_base in [false, true] {
            for _ in 0..8 {
                let mut f = setup_dex();
                let quote = if taxed_quote {
                    taxed_mint(&mut f, 200)
                } else {
                    f.quote
                };
                let config = if taxed_quote {
                    let config =
                        Address::find_program_address(&[b"config", quote.as_ref()], &fossa_dex::ID)
                            .0;
                    let initialize = instruction(
                        fossa_dex::ID,
                        accounts::InitializeConfig {
                            global: f.global,
                            council: f.payer.pubkey(),
                            token: anchor_spl::token_2022::ID,
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
                                    protocol_share: 10000,
                                    creator_share: 0,
                                },
                                distributor: Some(f.distributor),
                            },
                        },
                    );
                    send(&mut f.svm, &[initialize], &f.payer, &[]).unwrap();
                    config
                } else {
                    f.config
                };
                let p = funded_pool(&mut f, quote, config, taxed_base);
                for a_to_b in [false, true] {
                    for exact_out in [false, true] {
                        for amount in [1_000, 10_000, 1_000_000] {
                            let args = args::SwapArgs {
                                a_to_b,
                                amount,
                                limit: if exact_out { u64::MAX } else { 0 },
                            };
                            let swap = if exact_out {
                                instruction(
                                    fossa_dex::ID,
                                    accounts::SwapExactOut { trade: p.trade(&f) },
                                    ix::SwapExactOut { args },
                                )
                            } else {
                                instruction(
                                    fossa_dex::ID,
                                    accounts::Swap { trade: p.trade(&f) },
                                    ix::Swap { args },
                                )
                            };
                            let measured = send(&mut f.svm, &[swap], &f.payer, &[])
                                .unwrap()
                                .compute_units_consumed;
                            assert!(
                                measured <= if exact_out { 38_000 } else { 36_000 },
                                "swap taxed={taxed_base}/{taxed_quote}: {measured}"
                            );
                        }
                    }
                }
                let add = instruction(
                    fossa_dex::ID,
                    accounts::AddLiquidity {
                        liquidity: p.liquidity(&f),
                    },
                    ix::AddLiquidity {
                        args: args::LiquidityArgs {
                            amount: 1_000_000,
                            bound_a: u64::MAX,
                            bound_b: u64::MAX,
                        },
                    },
                );
                let measured = send(&mut f.svm, &[add], &f.payer, &[])
                    .unwrap()
                    .compute_units_consumed;
                assert!(measured <= 45_000, "add liquidity: {measured}");
                let second = funded_pool(&mut f, quote, config, taxed_base);
                let sell = instruction(
                    fossa_dex::ID,
                    accounts::Swap { trade: p.trade(&f) },
                    ix::Swap {
                        args: args::SwapArgs {
                            a_to_b: true,
                            amount: 1_000_000,
                            limit: 0,
                        },
                    },
                );
                let buy = second.swap(&f, 100_000, 0, false);
                let measured = send(&mut f.svm, &[sell, buy], &f.payer, &[])
                    .unwrap()
                    .compute_units_consumed;
                assert!(measured <= 72_000, "two-hop route: {measured}");
                let route = [
                    p.swap(&f, 100_000, 0, false),
                    second.swap(&f, 100_000, 0, false),
                    p.swap(&f, 100_000, 0, false),
                    second.swap(&f, 100_000, 0, false),
                ];
                for hops in [3, 4] {
                    let measured = send(&mut f.svm, &route[..hops], &f.payer, &[])
                        .unwrap()
                        .compute_units_consumed;
                    assert!(
                        measured <= 36_000 * hops as u64,
                        "{hops}-hop route: {measured}"
                    );
                }
                let treasury = Keypair::new();
                f.svm.airdrop(&treasury.pubkey(), 1_000_000).unwrap();
                let update = instruction(
                    fossa_dex::ID,
                    accounts::UpdateGlobal {
                        global: f.global,
                        council: f.payer.pubkey(),
                        new_council: f.payer.pubkey(),
                    },
                    ix::UpdateGlobal {
                        config: args::GlobalConfig {
                            treasury: treasury.pubkey(),
                            distributor: f.distributor,
                            usdc_mint: f.quote,
                        },
                    },
                );
                assert!(
                    send(&mut f.svm, &[update], &f.payer, &[])
                        .unwrap()
                        .compute_units_consumed
                        <= 10_000
                );
                f.treasury = treasury.pubkey();
                for enabled in [false, true] {
                    let update = instruction(
                        fossa_dex::ID,
                        accounts::UpdateConfig {
                            global: f.global,
                            council: f.payer.pubkey(),
                            token: p.token_b,
                            mint: quote,
                            config,
                        },
                        ix::UpdateConfig { enabled },
                    );
                    assert!(
                        send(&mut f.svm, &[update], &f.payer, &[])
                            .unwrap()
                            .compute_units_consumed
                            <= 10_000
                    );
                }
                let update = instruction(
                    fossa_dex::ID,
                    accounts::UpdateRecipients {
                        global: f.global,
                        council: f.payer.pubkey(),
                        pool: p.pool,
                    },
                    ix::UpdateRecipients {},
                );
                assert!(
                    send(&mut f.svm, &[update], &f.payer, &[])
                        .unwrap()
                        .compute_units_consumed
                        <= 10_000
                );
                let destination_a =
                    CreateAssociatedTokenAccount::new(&mut f.svm, &f.payer, &p.mint)
                        .token_program_id(&p.token_a)
                        .owner(&treasury.pubkey())
                        .send()
                        .unwrap();
                let destination_b =
                    CreateAssociatedTokenAccount::new(&mut f.svm, &f.payer, &p.quote)
                        .token_program_id(&p.token_b)
                        .owner(&treasury.pubkey())
                        .send()
                        .unwrap();
                let collect = instruction(
                    fossa_dex::ID,
                    accounts::CollectFees {
                        owner: treasury.pubkey(),
                        pool: p.pool,
                        authority: p.authority,
                        token_a: p.token_a,
                        token_b: p.token_b,
                        mint_a: p.mint,
                        mint_b: quote,
                        destination_a,
                        destination_b,
                        vault_a: p.vault_a,
                        vault_b: p.vault_b,
                    },
                    ix::CollectFees {
                        args: args::CollectArgs {
                            protocol: true,
                            minimum_a: 0,
                            minimum_b: 0,
                        },
                    },
                );
                assert!(
                    send(&mut f.svm, &[collect], &f.payer, &[&treasury])
                        .unwrap()
                        .compute_units_consumed
                        <= 40_000
                );
                let remove = instruction(
                    fossa_dex::ID,
                    accounts::RemoveLiquidity {
                        liquidity: p.liquidity(&f),
                    },
                    ix::RemoveLiquidity {
                        args: args::LiquidityArgs {
                            amount: token_account(&f.svm, p.user_lp.pubkey()).amount,
                            bound_a: 0,
                            bound_b: 0,
                        },
                    },
                );
                let measured = send(&mut f.svm, &[remove], &f.payer, &[])
                    .unwrap()
                    .compute_units_consumed;
                assert!(measured <= 45_000, "full withdrawal: {measured}");
            }
        }
    }
}
