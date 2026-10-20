use super::*;

fn create_account(v: &Vm, payer: Address, account: Address, len: usize) -> Instruction {
    let mut data = 0u32.to_le_bytes().to_vec();
    data.extend_from_slice(&v.svm.minimum_balance_for_rent_exemption(len).to_le_bytes());
    data.extend_from_slice(&(len as u64).to_le_bytes());
    data.extend_from_slice(v.program.as_ref());
    Instruction {
        program_id: SYSTEM,
        accounts: vec![ws(payer), ws(account)],
        data,
    }
}
fn profile_address(v: &Vm, market: Address, provider: Address) -> Address {
    Address::find_program_address(
        &[b"provider", market.as_ref(), provider.as_ref()],
        &v.program,
    )
    .0
}
fn mutated(v: &mut Vm, key: Address, offset: usize, value: u8, run: impl FnOnce(&mut Vm)) {
    let original = v.svm.get_account(&key).unwrap();
    let mut altered = original.clone();
    altered.data[offset] = value;
    v.svm.set_account(key, altered).unwrap();
    run(v);
    v.svm.set_account(key, original).unwrap();
}
fn check_record(v: &Vm, key: Address, hash: usize, uri: usize, payload: &[u8]) {
    let d = v.data(key);
    assert_eq!(&d[hash..hash + 32], &payload[..32]);
    assert_eq!(
        u16::from_le_bytes(d[hash + 32..hash + 34].try_into().unwrap()) as usize,
        payload.len() - 32
    );
    assert_eq!(&d[uri..uri + payload.len() - 32], &payload[32..]);
    assert!(d[uri + payload.len() - 32..].iter().all(|b| *b == 0));
}
pub(super) fn check(v: &mut Vm, f: &Fixture) -> serde_json::Value {
    let start = v.executions;
    let admin = v.payer.pubkey();
    let provider = Keypair::new();
    let outsider = Keypair::new();
    let profile = profile_address(v, f.market, provider.pubkey());
    let payload = metadata_payload(2, "https://example.test/provider.json");
    let provider_accounts = vec![
        r(f.market),
        w(profile),
        s(provider.pubkey()),
        ws(admin),
        r(SYSTEM),
    ];
    let mut missing_signature = provider_accounts.clone();
    missing_signature[2] = r(provider.pubkey());
    v.send(19, &payload, missing_signature, &[], false);
    error(v, 4);
    for invalid in [
        vec![0; 32],
        metadata_payload(0, &"a".repeat(257)),
        metadata_payload(0, "https://x/\0"),
    ] {
        v.send(19, &invalid, provider_accounts.clone(), &[&provider], false);
        error(v, 3);
    }
    // donors cannot reserve a pda or prevent its canonical initialization.
    v.svm.airdrop(&profile, 10_000_000).unwrap();
    let donated = v.svm.get_account(&profile).unwrap();
    for (owner, data) in [
        (v.program, vec![0; PROVIDER_LEN]),
        (SYSTEM, vec![0; 1]),
        (TOKEN, vec![]),
    ] {
        v.put(profile, owner, data);
        v.send(19, &payload, provider_accounts.clone(), &[&provider], false);
        error(v, 2);
    }
    v.svm.set_account(profile, donated).unwrap();
    let mut wrong = provider_accounts.clone();
    wrong[1] = w(key());
    v.send(19, &payload, wrong, &[&provider], false);
    error(v, 2);
    v.send(19, &payload, provider_accounts.clone(), &[&provider], true);
    assert_eq!(v.data(profile)[106], 0);
    check_record(v, profile, 72, 108, &payload);
    v.send(19, &payload, provider_accounts, &[&provider], false);
    error(v, 2);
    let approval = vec![r(f.market), w(profile), s(admin)];
    v.send(
        20,
        &[1],
        vec![r(f.market), w(profile), s(provider.pubkey())],
        &[&provider],
        false,
    );
    error(v, 4);
    // fee payer is implicitly a signer; use a distinct nonsigner for the missing signature case.
    v.send(
        20,
        &[1],
        vec![r(f.market), w(profile), r(outsider.pubkey())],
        &[],
        false,
    );
    error(v, 4);
    for bad in [&[][..], &[2][..], &[1, 0][..]] {
        v.send(20, bad, approval.clone(), &[], false);
        error(v, 3);
    }
    let id = [91; 32];
    let model = Address::find_program_address(&[b"model", f.market.as_ref(), &id], &v.program).0;
    let model_metadata =
        Address::find_program_address(&[b"model_metadata", model.as_ref()], &v.program).0;
    let terms = Keypair::new();
    let capacity = Keypair::new();
    let payer = Keypair::new();
    v.svm.airdrop(&payer.pubkey(), 100_000_000).unwrap();
    v.svm.airdrop(&model, 1_000_000).unwrap();
    v.svm.airdrop(&model_metadata, 1_000_000).unwrap();
    let max_uri = format!("https://example.test/{}", "a".repeat(URI_MAX - 21));
    assert_eq!(max_uri.len(), URI_MAX);
    let mut longest = (0..32).collect::<Vec<u8>>();
    longest.extend_from_slice(max_uri.as_bytes());
    let mut init = id.to_vec();
    init.extend_from_slice(&2u32.to_le_bytes());
    init.extend_from_slice(&longest);
    let model_accounts = vec![
        r(f.market),
        w(model),
        ws(terms.pubkey()),
        ws(capacity.pubkey()),
        s(provider.pubkey()),
        ws(payer.pubkey()),
        r(SYSTEM),
        r(profile),
        w(model_metadata),
        s(admin),
    ];
    let prefix = [
        create_account(v, payer.pubkey(), terms.pubkey(), terms_book_len(2)),
        create_account(v, payer.pubkey(), capacity.pubkey(), capacity_book_len(2)),
    ];
    let signers = [&terms, &capacity, &provider, &payer];
    v.send_with(18, &init, model_accounts.clone(), &signers, false, &prefix);
    error(v, 4);
    assert!(v.svm.get_account(&terms.pubkey()).is_none());
    let offer = &f.offers[0];
    let mut registration = [0; 24];
    for at in [4, 8, 12, 20] {
        u32at(&mut registration, at, 1);
    }
    registration[16] = 100;
    registration[19] = 1;
    let unapproved_offer = vec![
        r(f.market),
        r(f.model),
        w(f.terms),
        w(f.capacity),
        r(profile),
        s(provider.pubkey()),
        r(offer.payout),
        r(offer.bond),
    ];
    v.send(2, &registration, unapproved_offer, &[&provider], false);
    error(v, 4);
    v.send(20, &[1], approval.clone(), &[], true);
    // approved providers still need explicit admin authorization for canonical model creation.
    v.send_with(18, &init, model_accounts[..9].to_vec(), &signers, false, &prefix);
    error(v, 4);
    for invalid in [&init[..0], &init[..35], &init[..36], &init[..68]] {
        v.send_with(
            18,
            invalid,
            model_accounts.clone(),
            &signers,
            false,
            &prefix,
        );
        error(v, 3);
    }
    for at in [0, 8, 40, 107] {
        let old = v.data(profile)[at];
        mutated(v, profile, at, old.wrapping_add(1), |v| {
            v.send_with(18, &init, model_accounts.clone(), &signers, false, &prefix);
            error(v, 2);
        });
    }
    let original = v.svm.get_account(&profile).unwrap();
    let mut bad = original.clone();
    bad.owner = TOKEN;
    v.svm.set_account(profile, bad).unwrap();
    v.send_with(18, &init, model_accounts.clone(), &signers, false, &prefix);
    error(v, 2);
    v.svm.set_account(profile, original).unwrap();
    let counterfeit = key();
    v.put(counterfeit, v.program, v.data(profile));
    let mut wrong = model_accounts.clone();
    wrong[7] = r(counterfeit);
    v.send_with(18, &init, wrong, &signers, false, &prefix);
    error(v, 2);
    let mut wrong = model_accounts.clone();
    wrong[7] = r(f.profile);
    v.send_with(18, &init, wrong, &signers, false, &prefix);
    error(v, 4);
    let mut wrong = model_accounts.clone();
    wrong[4] = r(provider.pubkey());
    v.send_with(
        18,
        &init,
        wrong,
        &[&terms, &capacity, &payer],
        false,
        &prefix,
    );
    error(v, 4);
    // second pda failure rolls back first pda creation, book creation and rent transfers.
    let original = v.svm.get_account(&model_metadata).unwrap();
    for (owner, data) in [
        (v.program, vec![0; MODEL_METADATA_LEN]),
        (SYSTEM, vec![0; 1]),
        (TOKEN, vec![]),
    ] {
        v.put(model_metadata, owner, data);
        v.send_with(18, &init, model_accounts.clone(), &signers, false, &prefix);
        error(v, 2);
    }
    v.svm.set_account(model_metadata, original).unwrap();
    let model_init_cu = v.send_with(18, &init, model_accounts.clone(), &signers, true, &prefix);
    check_record(v, model_metadata, 104, 140, &longest);
    assert_eq!(&v.data(model_metadata)[72..104], provider.pubkey().as_ref());
    assert_eq!(v.data(model).len(), MODEL_LEN);
    // canonical model id rejects replacement even with fresh, valid books.
    let duplicate_terms = Keypair::new();
    let duplicate_capacity = Keypair::new();
    v.blank(&duplicate_terms, terms_book_len(2));
    v.blank(&duplicate_capacity, capacity_book_len(2));
    let mut duplicate = model_accounts.clone();
    duplicate[2] = ws(duplicate_terms.pubkey());
    duplicate[3] = ws(duplicate_capacity.pubkey());
    v.send(
        18,
        &init,
        duplicate,
        &[&duplicate_terms, &duplicate_capacity, &provider, &payer],
        false,
    );
    error(v, 2);
    let provider_update = vec![r(f.market), w(profile), s(provider.pubkey())];
    let model_update = vec![
        r(f.market),
        r(model),
        w(model_metadata),
        s(provider.pubkey()),
    ];
    for (op, accounts, hash, uri, record) in [
        (21, provider_update.clone(), 72, 108, profile),
        (22, model_update.clone(), 104, 140, model_metadata),
    ] {
        v.send(op, &longest, accounts.clone(), &[&provider], true);
        check_record(v, record, hash, uri, &longest);
        let mut same_uri = longest.clone();
        for b in &mut same_uri[..32] {
            *b ^= 255;
        }
        let before = v.data(record);
        v.send(op, &same_uri, accounts.clone(), &[&provider], true);
        let after = v.data(record);
        assert_eq!(&before[..hash], &after[..hash]);
        assert_eq!(&before[hash + 32..], &after[hash + 32..]);
        check_record(v, record, hash, uri, &same_uri);
        for uri_value in ["x".to_owned(), "é".repeat(128)] {
            let p = metadata_payload(0, &uri_value);
            v.send(op, &p, accounts.clone(), &[&provider], true);
            check_record(v, record, hash, uri, &p);
        }
        let unicode = metadata_payload(0, "https://example.test/模型.json");
        v.send(op, &unicode, accounts.clone(), &[&provider], true);
        check_record(v, record, hash, uri, &unicode);
        let mut invalid_utf8 = vec![0; 32];
        invalid_utf8.push(255);
        let invalid = [
            vec![],
            vec![0; 31],
            vec![0; 32],
            metadata_payload(0, &"a".repeat(257)),
            invalid_utf8,
            metadata_payload(0, "https://x/\n"),
            metadata_payload(0, "https://x/a b"),
        ];
        for p in invalid {
            v.send(op, &p, accounts.clone(), &[&provider], false);
            error(v, 3);
        }
        let mut wrong = accounts.clone();
        *wrong.last_mut().unwrap() = s(outsider.pubkey());
        v.send(op, &payload, wrong, &[&outsider], false);
        error(v, 4);
        let mut wrong = accounts.clone();
        *wrong.last_mut().unwrap() = r(provider.pubkey());
        v.send(op, &payload, wrong, &[], false);
        error(v, 4);
        let mut wrong = accounts.clone();
        wrong[if op == 21 { 1 } else { 2 }].is_writable = false;
        v.send(op, &payload, wrong, &[&provider], false);
        error(v, 2);
    }
    for at in [0, 8, 40, 138] {
        let old = v.data(model_metadata)[at];
        mutated(v, model_metadata, at, old.wrapping_add(1), |v| {
            v.send(22, &payload, model_update.clone(), &[&provider], false);
            error(v, 2);
        });
    }
    let original = v.svm.get_account(&model_metadata).unwrap();
    let mut altered = original.clone();
    altered.owner = TOKEN;
    v.svm.set_account(model_metadata, altered).unwrap();
    v.send(22, &payload, model_update.clone(), &[&provider], false);
    error(v, 2);
    v.svm.set_account(model_metadata, original).unwrap();
    let counterfeit = key();
    v.put(counterfeit, v.program, v.data(model_metadata));
    let mut wrong = model_update.clone();
    wrong[2] = w(counterfeit);
    v.send(22, &payload, wrong, &[&provider], false);
    error(v, 2);
    let second = Keypair::new();
    let second_profile = onboard(v, f.market, &second);
    // a separate canonical id may describe the same human model document.
    let mut second_init = init.clone();
    second_init[0] ^= 1;
    let second_model = Address::find_program_address(
        &[b"model", f.market.as_ref(), &second_init[..32]],
        &v.program,
    )
    .0;
    let second_metadata =
        Address::find_program_address(&[b"model_metadata", second_model.as_ref()], &v.program).0;
    let second_terms = Keypair::new();
    let second_capacity = Keypair::new();
    v.blank(&second_terms, terms_book_len(2));
    v.blank(&second_capacity, capacity_book_len(2));
    let second_accounts = vec![
        r(f.market),
        w(second_model),
        ws(second_terms.pubkey()),
        ws(second_capacity.pubkey()),
        s(second.pubkey()),
        ws(payer.pubkey()),
        r(SYSTEM),
        r(second_profile),
        w(second_metadata),
        s(admin),
    ];
    v.send(
        18,
        &second_init,
        second_accounts,
        &[&second_terms, &second_capacity, &second, &payer],
        true,
    );
    check_record(v, second_metadata, 104, 140, &longest);
    // admin acting as provider keeps the original eight-account registration flow.
    let admin_payout = v.token(f.mint, admin, 0);
    for slot in 0..2u32 {
        let authority = Address::find_program_address(
            &[b"bond", second_model.as_ref(), &slot.to_le_bytes()],
            &v.program,
        ).0;
        let bond = v.token(f.mint, authority, 0);
        let mut offer = registration;
        u32at(&mut offer, 0, slot);
        v.send(2, &offer, vec![
            r(f.market), r(second_model), w(second_terms.pubkey()),
            w(second_capacity.pubkey()), r(f.profile), s(admin), r(admin_payout), r(bond),
        ], &[], true);
    }
    let payout = v.token(f.mint, second.pubkey(), 1_000_000);
    let authority =
        Address::find_program_address(&[b"bond", model.as_ref(), &0u32.to_le_bytes()], &v.program)
            .0;
    let bond = v.token(f.mint, authority, 0);
    let offer_accounts = vec![
        r(f.market),
        r(model),
        w(terms.pubkey()),
        w(capacity.pubkey()),
        r(second_profile),
        s(second.pubkey()),
        r(payout),
        r(bond),
    ];
    for at in [0, 8, 40, 107] {
        let old = v.data(second_profile)[at];
        mutated(v, second_profile, at, old.wrapping_add(1), |v| {
            v.send(2, &registration, offer_accounts.clone(), &[&second], false);
            error(v, 2);
        });
    }
    let mut wrong = offer_accounts.clone();
    wrong[4] = r(profile);
    v.send(2, &registration, wrong, &[&second], false);
    error(v, 4);
    v.send(2, &registration, offer_accounts.clone(), &[&second], true);
    assert_eq!(
        &v.data(terms.pubkey())[HEADER_LEN..HEADER_LEN + 32],
        second.pubkey().as_ref()
    );
    let mut duplicate = registration;
    u32at(&mut duplicate, 0, 1);
    let duplicate_authority = Address::find_program_address(
        &[b"bond", model.as_ref(), &1u32.to_le_bytes()],
        &v.program,
    ).0;
    let duplicate_bond = v.token(f.mint, duplicate_authority, 0);
    let mut duplicate_accounts = offer_accounts.clone();
    duplicate_accounts[7] = r(duplicate_bond);
    v.send(2, &duplicate, duplicate_accounts.clone(), &[&second], false);
    error(v, 4);
    duplicate_accounts.push(s(outsider.pubkey()));
    v.send(2, &duplicate, duplicate_accounts.clone(), &[&second, &outsider], false);
    error(v, 4);
    *duplicate_accounts.last_mut().unwrap() = s(admin);
    v.send(2, &duplicate, duplicate_accounts, &[&second], true);
    assert_eq!(rd32(&v.data(terms.pubkey()), 44), 2);
    v.send(
        22,
        &payload,
        vec![r(f.market), r(model), w(model_metadata), s(second.pubkey())],
        &[&second],
        false,
    );
    error(v, 4);
    // per-offer approval remains independent of provider approval and provider term updates.
    v.send(
        16,
        &[0; 5],
        vec![r(f.market), r(model), w(terms.pubkey()), s(admin)],
        &[],
        true,
    );
    v.send(
        5,
        &registration,
        vec![r(f.market), r(model), w(terms.pubkey()), s(second.pubkey())],
        &[&second],
        true,
    );
    assert_eq!(v.data(terms.pubkey())[HEADER_LEN + 112], 0);
    v.send(
        20,
        &[0],
        vec![r(f.market), w(second_profile), s(admin)],
        &[],
        true,
    );
    u32at(&mut registration, 0, 1);
    v.send(2, &registration, offer_accounts, &[&second], false);
    error(v, 4);
    v.send(20, &[0], approval.clone(), &[], true);
    // revoked creator can still update routing metadata but cannot create a new model.
    v.send(21, &payload, provider_update, &[&provider], true);
    v.send(22, &payload, model_update, &[&provider], true);
    let mut another = init.clone();
    another[0] ^= 2;
    let another_model =
        Address::find_program_address(&[b"model", f.market.as_ref(), &another[..32]], &v.program).0;
    let another_metadata =
        Address::find_program_address(&[b"model_metadata", another_model.as_ref()], &v.program).0;
    let mut revoked = model_accounts;
    revoked[1] = w(another_model);
    revoked[2] = ws(duplicate_terms.pubkey());
    revoked[3] = ws(duplicate_capacity.pubkey());
    revoked[8] = w(another_metadata);
    v.send(
        18,
        &another,
        revoked,
        &[&duplicate_terms, &duplicate_capacity, &provider, &payer],
        false,
    );
    error(v, 4);
    let provider_packets = v.packet_max[19];
    // longest provider creation payload remains valid with distinct funding and transaction payers.
    let longest_provider = Keypair::new();
    let longest_profile = profile_address(v, f.market, longest_provider.pubkey());
    v.send(
        19,
        &longest,
        vec![
            r(f.market),
            w(longest_profile),
            s(longest_provider.pubkey()),
            ws(payer.pubkey()),
            r(SYSTEM),
        ],
        &[&longest_provider, &payer],
        true,
    );
    check_record(v, longest_profile, 72, 108, &longest);
    // same state and request before/after cold metadata updates must produce identical bookings and cu.
    let request = book_payload(&f.id, Address::default());
    let (before_booking, before_cu) = book(v, f, &request);
    let expected = v.data(before_booking);
    settle(v, f, before_booking, 0);
    let hot: Vec<_> = [f.market, f.model, f.terms, f.capacity, f.user, f.vault]
        .into_iter()
        .map(|a| (a, v.data(a)))
        .collect();
    v.send(
        21,
        &longest,
        vec![r(f.market), w(f.profile), s(f.creator.pubkey())],
        &[&f.creator],
        true,
    );
    v.send(
        22,
        &longest,
        vec![
            r(f.market),
            r(f.model),
            w(f.metadata),
            s(f.creator.pubkey()),
        ],
        &[&f.creator],
        true,
    );
    for (a, bytes) in hot {
        assert_eq!(v.data(a), bytes);
    }
    let (after_booking, after_cu) = book(v, f, &request);
    assert_eq!(v.data(after_booking), expected);
    assert_eq!(before_cu, after_cu);
    let pinned = &f.offers[rd32(&expected, 352) as usize];
    v.send(
        20,
        &[0],
        vec![r(f.market), w(pinned.profile), s(admin)],
        &[],
        true,
    );
    v.send(
        21,
        &payload,
        vec![r(f.market), w(pinned.profile), s(pinned.provider.pubkey())],
        &[&pinned.provider],
        true,
    );
    settle(v, f, after_booking, 3);
    // revocation deliberately preserves existing offer eligibility; admin separately controls opcode 16.
    let (still_eligible, _) = book(v, f, &request);
    assert_eq!(rd32(&v.data(still_eligible), 352), rd32(&expected, 352));
    settle(v, f, still_eligible, 0);
    v.send(
        20,
        &[1],
        vec![r(f.market), w(pinned.profile), s(admin)],
        &[],
        true,
    );
    reconcile(v, f, &[]);
    serde_json::json!({"executions":v.executions-start,"model_init_with_system_creates_cu":model_init_cu,"model_init_max_packet_bytes":v.packet_max[18],"provider_init_max_packet_bytes":v.packet_max[19],"previous_provider_packet_bytes":provider_packets,"book_before_metadata_cu":before_cu,"book_after_metadata_cu":after_cu,"identical_booking_bytes":true,"hot_accounts_unchanged":true,"revoked_provider_claim_settled":true})
}
