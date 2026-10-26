use super::*;
const T22: Address = address!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

fn external(
    v: &mut Vm,
    program: Address,
    data: Vec<u8>,
    accounts: Vec<AccountMeta>,
    signers: &[&Keypair],
) -> u64 {
    v.svm.expire_blockhash();
    let mut keys = vec![&v.payer];
    for k in signers {
        if k.pubkey() != v.payer.pubkey() {
            keys.push(*k);
        }
    }
    let tx = Transaction::new(
        &keys,
        Message::new(
            &[Instruction {
                program_id: program,
                accounts,
                data,
            }],
            Some(&v.payer.pubkey()),
        ),
        v.svm.latest_blockhash(),
    );
    v.svm.send_transaction(tx).unwrap().compute_units_consumed
}
fn transfer(
    v: &mut Vm,
    token: Address,
    mint: Address,
    source: Address,
    destination: Address,
    owner: &Keypair,
    amount: u64,
) {
    let mut p = vec![12];
    p.extend_from_slice(&amount.to_le_bytes());
    p.push(v.data(mint)[44]);
    external(
        v,
        token,
        p,
        vec![w(source), r(mint), w(destination), s(owner.pubkey())],
        &[owner],
    );
}
fn terms(fee: u16, share: u16) -> Vec<u8> {
    [fee.to_le_bytes(), share.to_le_bytes()].concat()
}
fn mint_account(v: &mut Vm, mint: Address, owner: Address) -> Address {
    let account = key();
    v.put(account, T22, vec![0; 178]);
    let mut data = vec![18];
    data.extend_from_slice(owner.as_ref());
    external(v, T22, data, vec![w(account), r(mint)], &[]);
    account
}
fn configure(v: &mut Vm, f: &Fixture, treasury: Address, treasury2: Address) -> Vec<u8> {
    let mut p = vec![0; 137];
    p[..32].copy_from_slice(treasury.as_ref());
    p[32..64].copy_from_slice(treasury2.as_ref());
    v.send(
        35,
        &p,
        vec![
            w(f.market),
            s(v.payer.pubkey()),
            ws(v.payer.pubkey()),
            r(SYSTEM),
        ],
        &[],
        true,
    );
    p
}
pub fn run() {
    listing_payments();
    let mut v = Vm::new(false, false);
    let f = setup(&mut v);
    let admin = v.payer.pubkey();
    let treasury = Keypair::new();
    let treasury2 = Keypair::new();
    v.svm.airdrop(&treasury.pubkey(), 1_000_000_000).unwrap();
    v.svm.airdrop(&treasury2.pubkey(), 1_000_000_000).unwrap();
    member(&mut v, &f, f.providers[0].pubkey(), true, true);
    let legacy = Keypair::new();
    v.blank(&legacy, BOOKING_LEN);
    let ba = booking_accounts(&v, &f, legacy.pubkey(), true);
    v.send(6, &request(&f, None), ba, &[&legacy, &f.owner], true);
    let mut config = vec![0; 137];
    config[..32].copy_from_slice(treasury.pubkey().as_ref());
    config[32..64].copy_from_slice(treasury2.pubkey().as_ref());
    v.send(
        35,
        &config,
        vec![w(f.market), s(f.owner.pubkey()), ws(admin), r(SYSTEM)],
        &[&f.owner],
        false,
    );
    configure(&mut v, &f, treasury.pubkey(), treasury2.pubkey());
    let next_admin = Keypair::new();
    v.svm.airdrop(&next_admin.pubkey(), 1_000_000).unwrap();
    v.send(
        43,
        &[],
        vec![w(f.market), s(admin), r(next_admin.pubkey())],
        &[],
        false,
    );
    let original = v.data(f.market);
    v.send(
        43,
        &[],
        vec![w(f.market), s(admin), s(next_admin.pubkey())],
        &[&next_admin],
        true,
    );
    assert_eq!(&v.data(f.market)[40..], &original[40..]);
    v.send(
        43,
        &[],
        vec![w(f.market), s(admin), s(next_admin.pubkey())],
        &[&next_admin],
        false,
    );
    v.send(
        43,
        &[],
        vec![w(f.market), s(next_admin.pubkey()), s(admin)],
        &[&next_admin],
        true,
    );
    let mut usage = vec![0; 12];
    wr32(&mut usage, 0, 1_000_000);
    wr32(&mut usage, 8, 1_000_000);
    v.send(
        7,
        &usage,
        claim_accounts(&f, legacy.pubkey(), 0),
        &[&f.providers[0]],
        true,
    );
    let l = Keypair::new();
    v.blank(&l, provider_list_len(4));
    v.send(
        28,
        &4u32.to_le_bytes(),
        vec![r(f.market), ws(l.pubkey()), s(f.manager.pubkey())],
        &[&l, &f.manager],
        false,
    );
    let list_cfg = vec![
        r(f.market),
        w(f.list.pubkey()),
        s(f.manager.pubkey()),
        ws(admin),
        r(SYSTEM),
    ];
    for (fee, share) in [(9, 5000), (301, 5000), (100, 4999), (100, 10001)] {
        v.send(
            36,
            &terms(fee, share),
            list_cfg.clone(),
            &[&f.manager],
            false,
        );
    }
    v.send(36, &terms(100, 5000), list_cfg.clone(), &[&f.manager], true);
    let b = Keypair::new();
    v.blank(&b, FEE_BOOKING_LEN);
    let ba = booking_accounts(&v, &f, b.pubkey(), true);
    let mut p = request(&f, Some(1));
    wr64(&mut p, 55, 304);
    v.send(6, &p, ba.clone(), &[&b, &f.owner], false);
    wr64(&mut p, 55, 305);
    let reserve_cu = v.send(6, &p, ba, &[&b, &f.owner], true);
    assert_eq!(rd64(&v.data(b.pubkey()), 104), 305);
    let creator_dest = v.token(f.mint, f.manager.pubkey(), 0);
    let protocol_dest = v.token(f.mint, treasury.pubkey(), 0);
    let mut ca = claim_accounts(&f, b.pubkey(), 0);
    ca.extend([w(creator_dest), w(protocol_dest)]);
    v.send(
        36,
        &terms(300, 10000),
        list_cfg.clone(),
        &[&f.manager],
        true,
    );
    configure(&mut v, &f, admin, treasury2.pubkey());
    let mut remove = f.providers[0].pubkey().to_bytes().to_vec();
    remove.push(0);
    v.send(
        29,
        &remove,
        vec![r(f.market), w(f.list.pubkey()), s(f.manager.pubkey())],
        &[&f.manager],
        true,
    );
    let metadata = list_metadata_address(&v, f.list.pubkey());
    v.send(
        30,
        &[],
        vec![w(f.list.pubkey()), ws(f.manager.pubkey()), w(metadata)],
        &[&f.manager],
        true,
    );
    v.blank(&f.list, provider_list_len(4) + LIST_FEE_LEN);
    let mut list_payload = 4u32.to_le_bytes().to_vec();
    list_payload.extend_from_slice(&terms(100, 5000));
    list_payload.extend_from_slice(&0u64.to_le_bytes());
    v.send(
        28,
        &list_payload,
        vec![r(f.market), ws(f.list.pubkey()), s(f.owner.pubkey())],
        &[&f.list, &f.owner],
        true,
    );
    let claim_data = claim(&v, &f, b.pubkey(), [1_000_000, 0, 1_000_000]);
    v.send(
        7,
        &claim_data,
        claim_accounts(&f, b.pubkey(), 0),
        &[&f.providers[0]],
        false,
    );
    let wrong = v.token(f.mint, f.owner.pubkey(), 0);
    let mut invalid = ca.clone();
    invalid[12] = w(wrong);
    v.send(7, &claim_data, invalid, &[&f.providers[0]], false);
    let claim_cu = v.send(7, &claim_data, ca, &[&f.providers[0]], true);
    assert_eq!(rd64(&v.data(creator_dest), 64), 1);
    assert_eq!(rd64(&v.data(protocol_dest), 64), 2);
    assert_eq!(rd64(&v.data(b.pubkey()), 160), 303);
    v.send(
        30,
        &[],
        vec![w(f.list.pubkey()), ws(f.owner.pubkey()), w(metadata)],
        &[&f.owner],
        true,
    );
    v.blank(&f.list, provider_list_len(4) + LIST_FEE_LEN);
    v.send(
        28,
        &list_payload,
        vec![r(f.market), ws(f.list.pubkey()), s(f.manager.pubkey())],
        &[&f.list, &f.manager],
        true,
    );
    member(&mut v, &f, f.providers[0].pubkey(), true, true);
    configure(&mut v, &f, treasury.pubkey(), treasury2.pubkey());
    v.send(36, &terms(100, 5000), list_cfg.clone(), &[&f.manager], true);

    let mint = Address::find_program_address(&[b"reserve_mint", f.model.as_ref()], &v.program).0;
    let backing = Keypair::new();
    let allocation = Keypair::new();
    v.put(backing.pubkey(), TOKEN, vec![0; 165]);
    v.put(allocation.pubkey(), T22, vec![0; 178]);
    let dex = key();
    v.svm
        .add_program_from_file(
            dex,
            support::artifact("FOSSA_DEX_SBF", "dex/target/deploy/fossa_dex.so"),
        )
        .expect("build dex SBF first");
    let dex_cfg = Keypair::new();
    v.put(dex_cfg.pubkey(), dex, vec![0; 192]);
    let global = Address::find_program_address(&[b"protocol"], &dex).0;
    let program_data = Address::new_from_array(v.data(dex)[4..36].try_into().unwrap());
    let mut deployed = v.data(program_data);
    deployed[12] = 1;
    deployed[13..45].copy_from_slice(admin.as_ref());
    v.replace(program_data, deployed);
    let mut bootstrap = vec![14];
    bootstrap.extend_from_slice(treasury.pubkey().as_ref());
    bootstrap.extend_from_slice(treasury2.pubkey().as_ref());
    external(
        &mut v,
        dex,
        bootstrap,
        vec![ws(admin), w(global), r(dex), r(program_data), r(SYSTEM)],
        &[],
    );
    let mut dex_config = vec![0];
    dex_config.extend_from_slice(treasury.pubkey().as_ref());
    for x in [125u16, 10000, 0] {
        dex_config.extend_from_slice(&x.to_le_bytes());
    }
    dex_config.extend_from_slice(treasury2.pubkey().as_ref());
    external(
        &mut v,
        dex,
        dex_config,
        vec![
            s(admin),
            ws(dex_cfg.pubkey()),
            r(f.mint),
            r(TOKEN),
            r(global),
        ],
        &[&dex_cfg],
    );
    v.send(
        42,
        &[],
        vec![w(f.market), s(admin), r(dex), r(dex_cfg.pubkey())],
        &[],
        true,
    );
    let pool = Keypair::new();
    v.put(pool.pubkey(), dex, vec![0; 512]);
    let pool_auth = Address::find_program_address(&[b"authority", pool.pubkey().as_ref()], &dex).0;
    let pool_base = key();
    v.put(pool_base, T22, vec![0; 178]);
    let pool_quote = v.token(f.mint, pool_auth, 0);
    let source_quote = v.token(f.mint, treasury.pubkey(), 0);
    let lp_mint = key();
    v.put(lp_mint, TOKEN, vec![0; 82]);
    let mut lp_init = vec![20, 9];
    lp_init.extend_from_slice(pool_auth.as_ref());
    lp_init.push(0);
    external(&mut v, TOKEN, lp_init, vec![w(lp_mint)], &[]);
    let user_lp = v.token(lp_mint, treasury.pubkey(), 0);
    let curve = vec![
        ws(treasury.pubkey()),
        r(dex_cfg.pubkey()),
        ws(pool.pubkey()),
        r(pool_auth),
        r(mint),
        r(f.mint),
        w(allocation.pubkey()),
        w(source_quote),
        w(pool_base),
        w(pool_quote),
        w(lp_mint),
        w(user_lp),
        r(T22),
        r(TOKEN),
        r(TOKEN),
    ];
    let mut activate_accounts = vec![
        r(f.market),
        w(f.model),
        s(admin),
        ws(admin),
        r(SYSTEM),
        w(mint),
        w(backing.pubkey()),
        w(allocation.pubkey()),
        r(f.mint),
        r(TOKEN),
        r(T22),
        r(dex),
    ];
    activate_accounts.extend(curve);
    activate_accounts.push(r(global));
    let mut activate = 1_000_000u64.to_le_bytes().to_vec();
    activate.push(6);
    activate.extend_from_slice(&100u16.to_le_bytes());
    activate.extend_from_slice(&100_000u64.to_le_bytes());
    for n in [100_000u64, 0, 25_000_000_000, 2_202_970_298, 10_000] {
        activate.extend_from_slice(&n.to_le_bytes());
    }
    let mut invalid = activate_accounts.clone();
    invalid[2] = s(f.providers[0].pubkey());
    v.send(
        37,
        &activate,
        invalid,
        &[&treasury, &pool, &f.providers[0]],
        false,
    );
    for index in [8, 11] {
        let mut invalid = activate_accounts.clone();
        invalid[index] = r(if index == 11 { TOKEN } else { key() });
        v.send(37, &activate, invalid, &[&treasury, &pool], false);
    }
    let bad_mint = key();
    let mut invalid = activate_accounts.clone();
    invalid[5] = w(bad_mint);
    invalid[16] = r(bad_mint);
    v.send(37, &activate, invalid, &[&treasury, &pool], false);
    let mut bad = activate.clone();
    wr64(&mut bad, 43, u64::MAX);
    v.send(
        37,
        &bad,
        activate_accounts.clone(),
        &[&treasury, &pool],
        false,
    );
    assert!(v.svm.get_account(&mint).is_none_or(|a| a.data.is_empty()));
    assert!(v.data(backing.pubkey()).iter().all(|x| *x == 0));
    let rent_payer = Keypair::new();
    let transaction_payer = Keypair::new();
    for payer in [&rent_payer, &transaction_payer] {
        v.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    }
    let mut distinct = activate_accounts.clone();
    distinct[3] = ws(rent_payer.pubkey());
    let mut data = vec![37];
    data.extend_from_slice(&activate);
    let mut budget = vec![2];
    budget.extend_from_slice(&1_400_000u32.to_le_bytes());
    let instructions = [
        Instruction {
            program_id: address!("ComputeBudget111111111111111111111111111111"),
            accounts: vec![],
            data: budget,
        },
        Instruction {
            program_id: v.program,
            accounts: distinct,
            data,
        },
    ];
    v.svm.expire_blockhash();
    let tx = Transaction::new(
        &[&transaction_payer, &v.payer, &rent_payer, &treasury, &pool],
        Message::new(&instructions, Some(&transaction_payer.pubkey())),
        v.svm.latest_blockhash(),
    );
    let packet = 1 + 64 * tx.signatures.len() + tx.message_data().len();
    assert!(
        packet > 1232,
        "distinct-payer fixture should exercise lookup serialization"
    );
    let lookup = key();
    let addresses = vec![
        f.market,
        f.model,
        SYSTEM,
        mint,
        backing.pubkey(),
        allocation.pubkey(),
        f.mint,
        TOKEN,
        T22,
        dex,
        dex_cfg.pubkey(),
        pool_auth,
        pool_base,
        pool_quote,
        source_quote,
        lp_mint,
        user_lp,
        global,
    ];
    let mut table = vec![0; 56 + 32 * addresses.len()];
    wr32(&mut table, 0, 1);
    wr64(&mut table, 4, u64::MAX);
    for (i, address) in addresses.iter().enumerate() {
        table[56 + i * 32..88 + i * 32].copy_from_slice(address.as_ref());
    }
    v.put(
        lookup,
        address!("AddressLookupTab1e1111111111111111111111111"),
        table,
    );
    v.svm.warp_to_slot(1);
    let message = solana_message::v0::Message::try_compile(
        &transaction_payer.pubkey(),
        &instructions,
        &[solana_message::AddressLookupTableAccount {
            key: lookup,
            addresses,
        }],
        v.svm.latest_blockhash(),
    )
    .unwrap();
    let tx = solana_transaction::versioned::VersionedTransaction::try_new(
        solana_message::VersionedMessage::V0(message),
        &[&transaction_payer, &v.payer, &rent_payer, &treasury, &pool],
    )
    .unwrap();
    let v0_packet = 1 + 64 * tx.signatures.len() + tx.message.serialize().len();
    assert!(v0_packet <= 1232);
    let activate_cu = v.svm.send_transaction(tx).unwrap().compute_units_consumed;
    v.executions += 1;
    println!("atomic model activation: legacy={packet}, v0_lookup={v0_packet} serialized bytes with distinct admin, treasury, rent payer and transaction payer");
    v.send(37, &activate, activate_accounts, &[&treasury, &pool], false);
    assert_eq!(rd64(&v.data(pool_base), 64), 99_000);
    assert_eq!(rd64(&v.data(pool_quote), 64), 0);
    assert_eq!(rd64(&v.data(backing.pubkey()), 64), 0);
    // isolate redemption end-of-supply boundaries from inventory locked in the separate dex.
    let mut allocated = v.data(allocation.pubkey());
    wr64(&mut allocated, 64, 1_000_000);
    v.replace(allocation.pubkey(), allocated);
    let mut pool_tokens = v.data(pool_base);
    wr64(&mut pool_tokens, 64, 0);
    wr64(&mut pool_tokens, 170, 0);
    v.replace(pool_base, pool_tokens);
    let minted = v.data(mint);
    assert_eq!(&minted[..4], &[0; 4]);
    assert_eq!(&minted[46..50], &[0; 4]);
    assert_eq!(rd64(&minted, 36), 1_000_000);
    let holder = mint_account(&mut v, mint, f.owner.pubkey());
    let tax_dest = mint_account(&mut v, mint, treasury2.pubkey());
    transfer(
        &mut v,
        T22,
        mint,
        allocation.pubkey(),
        holder,
        &treasury,
        10_000,
    );
    assert_eq!(rd64(&v.data(holder), 64), 9900);
    let redeem_accounts = vec![
        r(f.market),
        r(f.model),
        w(mint),
        w(holder),
        s(f.owner.pubkey()),
        w(backing.pubkey()),
        w(wrong),
        r(f.mint),
        r(TOKEN),
        r(T22),
    ];
    let mut redemption = 9900u64.to_le_bytes().to_vec();
    redemption.extend_from_slice(&0u64.to_le_bytes());
    v.send(39, &redemption, redeem_accounts.clone(), &[&f.owner], false);
    let mut policy = vec![1];
    policy.extend_from_slice(&500u16.to_le_bytes());
    policy.extend_from_slice(&100_000u64.to_le_bytes());
    let policy_accounts = vec![r(f.market), w(f.model), s(admin), w(mint), r(T22)];
    v.send(38, &policy, policy_accounts.clone(), &[], true);
    transfer(
        &mut v,
        T22,
        mint,
        allocation.pubkey(),
        holder,
        &treasury,
        1000,
    );
    assert_eq!(rd64(&v.data(holder), 64), 10890);
    let mut clock = v.svm.get_sysvar::<Clock>();
    clock.epoch += 2;
    v.svm.set_sysvar(&clock);
    transfer(
        &mut v,
        T22,
        mint,
        allocation.pubkey(),
        holder,
        &treasury,
        1000,
    );
    assert_eq!(rd64(&v.data(holder), 64), 11840);
    let funding = v.token(f.mint, treasury.pubkey(), 1_000_000);
    transfer(
        &mut v,
        TOKEN,
        f.mint,
        funding,
        backing.pubkey(),
        &treasury,
        1_000_000,
    );
    let ba = booking_accounts(&v, &f, b.pubkey(), false);
    let routed_reserve_cu = v.send(6, &request(&f, Some(2)), ba, &[&f.owner], true);
    assert_eq!(&v.data(b.pubkey())[400..432], backing.pubkey().as_ref());
    policy[0] = 0;
    v.send(38, &policy, policy_accounts.clone(), &[], true);
    let mut ca = claim_accounts(&f, b.pubkey(), 0);
    ca.extend([w(creator_dest), w(backing.pubkey())]);
    let cp = claim(&v, &f, b.pubkey(), [1_000_000, 0, 1_000_000]);
    let routed_claim_cu = v.send(7, &cp, ca.clone(), &[&f.providers[0]], true);
    assert_eq!(rd64(&v.data(backing.pubkey()), 64), 1_000_002);
    let ba = booking_accounts(&v, &f, b.pubkey(), false);
    v.send(6, &request(&f, Some(3)), ba, &[&f.owner], true);
    let cp = claim(&v, &f, b.pubkey(), [0, 0, 0]);
    let mut zero_accounts = claim_accounts(&f, b.pubkey(), 0);
    zero_accounts.extend([w(creator_dest), w(protocol_dest)]);
    let before = v.data(f.vault);
    v.send(7, &cp, zero_accounts, &[&f.providers[0]], true);
    assert_eq!(v.data(f.vault), before);
    assert_eq!(rd64(&v.data(b.pubkey()), 160), 0);
    wr64(&mut redemption, 0, 11840);
    wr64(&mut redemption, 8, 11841);
    v.send(39, &redemption, redeem_accounts.clone(), &[&f.owner], false);
    wr64(&mut redemption, 8, 11840);
    let redeem_cu = v.send(39, &redemption, redeem_accounts, &[&f.owner], true);
    assert_eq!(rd64(&v.data(wrong), 64), 11840);
    assert_eq!(rd64(&v.data(holder), 64), 0);
    external(&mut v, T22, vec![26, 4], vec![w(mint), w(holder)], &[]);
    let tax_owner = Keypair::new();
    v.svm.airdrop(&tax_owner.pubkey(), 1_000_000_000).unwrap();
    configure(&mut v, &f, treasury.pubkey(), tax_owner.pubkey());
    v.send(
        40,
        &[],
        vec![r(f.market), r(f.model), w(mint), w(tax_dest), r(T22)],
        &[],
        false,
    );
    let tax_dest = mint_account(&mut v, mint, tax_owner.pubkey());
    let collect_cu = v.send(
        40,
        &[],
        vec![r(f.market), r(f.model), w(mint), w(tax_dest), r(T22)],
        &[],
        true,
    );
    assert_eq!(rd64(&v.data(tax_dest), 64), 160);
    let sweep = vec![
        r(f.market),
        r(f.model),
        r(mint),
        w(backing.pubkey()),
        w(protocol_dest),
        r(f.mint),
        r(TOKEN),
    ];
    v.send(41, &[], sweep.clone(), &[], false);
    policy[0] = 1;
    v.send(38, &policy, policy_accounts.clone(), &[], true);
    let ba = booking_accounts(&v, &f, b.pubkey(), false);
    v.send(6, &request(&f, Some(4)), ba, &[&f.owner], true);
    for (source, owner) in [(allocation.pubkey(), &treasury), (tax_dest, &tax_owner)] {
        let destination = v.token(f.mint, owner.pubkey(), 0);
        let amount = rd64(&v.data(source), 64);
        let mut p = amount.to_le_bytes().to_vec();
        p.extend_from_slice(&1u64.to_le_bytes());
        v.send(
            39,
            &p,
            vec![
                r(f.market),
                r(f.model),
                w(mint),
                w(source),
                s(owner.pubkey()),
                w(backing.pubkey()),
                w(destination),
                r(f.mint),
                r(TOKEN),
                r(T22),
            ],
            &[owner],
            true,
        );
    }
    assert_eq!(rd64(&v.data(mint), 36), 0);
    assert_eq!(rd64(&v.data(backing.pubkey()), 64), 0);
    let cp = claim(&v, &f, b.pubkey(), [1_000_000, 0, 1_000_000]);
    v.send(7, &cp, ca, &[&f.providers[0]], true);
    assert_eq!(rd64(&v.data(backing.pubkey()), 64), 2);
    v.send(41, &[], sweep, &[], true);
    assert_eq!(rd64(&v.data(backing.pubkey()), 64), 0);
    v.send(38, &policy, policy_accounts.clone(), &[], false);
    policy[0] = 0;
    v.send(38, &policy, policy_accounts, &[], true);
    configure(&mut v, &f, f.manager.pubkey(), tax_owner.pubkey());
    let ba = booking_accounts(&v, &f, b.pubkey(), false);
    v.send(6, &request(&f, Some(5)), ba, &[&f.owner], true);
    let cp = claim(&v, &f, b.pubkey(), [1_000_000, 0, 1_000_000]);
    let mut ca = claim_accounts(&f, b.pubkey(), 0);
    ca.extend([w(creator_dest), w(creator_dest)]);
    let before = rd64(&v.data(creator_dest), 64);
    let merged_claim_cu = v.send(7, &cp, ca, &[&f.providers[0]], true);
    assert_eq!(rd64(&v.data(creator_dest), 64) - before, 3);
    println!("coincident creator/treasury destination: {merged_claim_cu} CU; exact fee conserved with one combined fee transfer");
    println!("economics passed: executions={} booking={reserve_cu} split_claim={claim_cu} reserve_booking={routed_reserve_cu} reserve_claim={routed_claim_cu} activate={activate_cu} redeem={redeem_cu} collect={collect_cu}; fee conservation, snapshots, permission checks, fixed supply, tax epochs, redemption and final burn",v.executions);
}

fn listing_payments() {
    let mut v = Vm::new(false, false);
    let f = setup(&mut v);
    let admin = v.payer.pubkey();
    let treasury = key();
    configure(&mut v, &f, treasury, key());
    let payment = key();
    let mut md = vec![0; 278];
    md[..4].copy_from_slice(&1u32.to_le_bytes());
    md[4..36].copy_from_slice(admin.as_ref());
    wr64(&mut md, 36, 1_000_000);
    md[44] = 9;
    md[45] = 1;
    md[165] = 1;
    md[166..168].copy_from_slice(&1u16.to_le_bytes());
    md[168..170].copy_from_slice(&108u16.to_le_bytes());
    md[170..202].copy_from_slice(admin.as_ref());
    md[202..234].copy_from_slice(admin.as_ref());
    wr64(&mut md, 250, 10);
    md[258..260].copy_from_slice(&500u16.to_le_bytes());
    wr64(&mut md, 260, 3);
    wr64(&mut md, 268, 1000);
    md[276..278].copy_from_slice(&5000u16.to_le_bytes());
    v.put(payment, T22, md.clone());
    let source = mint_account(&mut v, payment, f.manager.pubkey());
    let destination = mint_account(&mut v, payment, treasury);
    let mut sd = v.data(source);
    wr64(&mut sd, 64, 10_000);
    v.replace(source, sd);
    let mut cfg = vec![0; 137];
    cfg[..32].copy_from_slice(treasury.as_ref());
    cfg[32..64].copy_from_slice(key().as_ref());
    cfg[64..96].copy_from_slice(payment.as_ref());
    cfg[96..128].copy_from_slice(T22.as_ref());
    wr64(&mut cfg, 128, 1000);
    cfg[136] = 1;
    let config_accounts = vec![
        w(f.market),
        s(admin),
        ws(admin),
        r(SYSTEM),
        r(payment),
        r(T22),
    ];
    let mut unsupported = md.clone();
    unsupported[166..168].copy_from_slice(&12u16.to_le_bytes());
    v.replace(payment, unsupported);
    v.send(35, &cfg, config_accounts.clone(), &[], false);
    v.replace(payment, md);
    v.send(35, &cfg, config_accounts, &[], true);
    let mut cu = 0;
    for (epoch, gross) in [(0, 1010u64), (3, 2000)] {
        let mut clock = v.svm.get_sysvar::<Clock>();
        clock.epoch = epoch;
        v.svm.set_sysvar(&clock);
        let list = Keypair::new();
        v.blank(&list, provider_list_len(4) + LIST_FEE_LEN);
        let mut payload = 4u32.to_le_bytes().to_vec();
        payload.extend_from_slice(&terms(10, 5000));
        payload.extend_from_slice(&(gross - 1).to_le_bytes());
        let accounts = vec![
            r(f.market),
            ws(list.pubkey()),
            s(f.manager.pubkey()),
            w(source),
            w(destination),
            r(payment),
            r(T22),
        ];
        v.send(28, &payload, accounts.clone(), &[&list, &f.manager], false);
        wr64(&mut payload, 8, gross);
        let before = rd64(&v.data(source), 64);
        let mut invalid = accounts.clone();
        invalid[4] = r(destination);
        v.send(28, &payload, invalid, &[&list, &f.manager], false);
        cu = v.send(28, &payload, accounts, &[&list, &f.manager], true);
        assert_eq!(before - rd64(&v.data(source), 64), gross);
        assert_eq!(
            rd64(&v.data(destination), 64),
            if epoch == 0 { 1000 } else { 2000 }
        );
        let mut membership = f.providers[0].pubkey().to_bytes().to_vec();
        membership.push(1);
        v.send(
            29,
            &membership,
            vec![r(f.market), w(list.pubkey()), s(f.manager.pubkey())],
            &[&f.manager],
            true,
        );
        assert_eq!(
            &v.data(list.pubkey())[provider_list_len(4)..provider_list_len(4) + 4],
            &terms(10, 5000)
        );
    }
    let legacy = key();
    let mut legacy_mint = vec![0; 82];
    legacy_mint[44] = 9;
    legacy_mint[45] = 1;
    wr64(&mut legacy_mint, 36, 10_000);
    v.put(legacy, TOKEN, legacy_mint);
    let source = v.token(legacy, f.manager.pubkey(), 5000);
    let destination = v.token(legacy, treasury, 0);
    cfg[64..96].copy_from_slice(legacy.as_ref());
    cfg[96..128].copy_from_slice(TOKEN.as_ref());
    wr64(&mut cfg, 128, 1234);
    v.send(
        35,
        &cfg,
        vec![
            w(f.market),
            s(admin),
            ws(admin),
            r(SYSTEM),
            r(legacy),
            r(TOKEN),
        ],
        &[],
        true,
    );
    let list = Keypair::new();
    v.blank(&list, provider_list_len(1) + LIST_FEE_LEN);
    let mut payload = 1u32.to_le_bytes().to_vec();
    payload.extend_from_slice(&terms(300, 10000));
    payload.extend_from_slice(&1234u64.to_le_bytes());
    let legacy_cu = v.send(
        28,
        &payload,
        vec![
            r(f.market),
            ws(list.pubkey()),
            s(f.manager.pubkey()),
            w(source),
            w(destination),
            r(legacy),
            r(TOKEN),
        ],
        &[&list, &f.manager],
        true,
    );
    assert_eq!(rd64(&v.data(destination), 64), 1234);
    println!("list creation payments passed: token2022={cu} legacy={legacy_cu} CU; exact net credit, debit cap, epoch/cap handling, arbitrary decimals and unsupported extensions");
}
