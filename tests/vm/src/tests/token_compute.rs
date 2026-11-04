use super::*;
use anchor_lang::solana_program::system_instruction;
use anchor_spl::{
    token_2022_extensions::spl_token_metadata_interface as metadata,
    token_interface::spl_token_2022 as token,
};
use solana_instruction::Instruction;
use spl_associated_token_account_interface as associated_token;

fn measured(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instructions: &[Instruction],
    signers: &[&Keypair],
    label: &str,
    ceiling: u64,
) -> u64 {
    let consumed = send(svm, instructions, payer, signers)
        .unwrap()
        .compute_units_consumed;
    eprintln!(
        "TOKEN_CU {label} instructions={} cu={consumed}",
        instructions.len()
    );
    assert!(consumed <= ceiling, "{label}: {consumed} exceeds {ceiling}");
    consumed
}

fn metadata_mint(
    svm: &mut LiteSVM,
    payer: &Keypair,
    tax_bps: u16,
    maximum_metadata: bool,
    separate: bool,
) -> Address {
    use token::extension::ExtensionType;
    let mint = Keypair::new();
    let address = mint.pubkey();
    let mut extensions = vec![ExtensionType::MetadataPointer];
    if tax_bps != 0 {
        extensions.push(ExtensionType::TransferFeeConfig);
    }
    let space =
        ExtensionType::try_calculate_account_len::<token::state::Mint>(&extensions).unwrap();
    let (name, symbol, uri) = if maximum_metadata {
        (
            "N".repeat(32),
            "S".repeat(10),
            format!("https://example.invalid/{}", "u".repeat(232)),
        )
    } else {
        (
            "Synthetic Token".to_owned(),
            "SYN".to_owned(),
            "https://example.invalid/token.json".to_owned(),
        )
    };
    assert!(uri.len() <= 256);
    let rent =
        svm.minimum_balance_for_rent_exemption(space + 84 + name.len() + symbol.len() + uri.len());
    let mut instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &address,
            rent,
            space as u64,
            &anchor_spl::token_2022::ID,
        ),
        token::extension::metadata_pointer::instruction::initialize(
            &anchor_spl::token_2022::ID,
            &address,
            None,
            Some(address),
        )
        .unwrap(),
    ];
    let mut ceilings = vec![150, 1_500];
    if tax_bps != 0 {
        instructions.push(
            token::extension::transfer_fee::instruction::initialize_transfer_fee_config(
                &anchor_spl::token_2022::ID,
                &address,
                None,
                Some(&payer.pubkey()),
                tax_bps,
                u64::MAX,
            )
            .unwrap(),
        );
        ceilings.push(2_000);
    }
    instructions.push(
        token::instruction::initialize_mint2(
            &anchor_spl::token_2022::ID,
            &address,
            &payer.pubkey(),
            None,
            6,
        )
        .unwrap(),
    );
    ceilings.push(3_000);
    instructions.push(metadata::instruction::initialize(
        &anchor_spl::token_2022::ID,
        &address,
        &payer.pubkey(),
        &address,
        &payer.pubkey(),
        name.clone(),
        symbol.clone(),
        uri.clone(),
    ));
    ceilings.push(5_000);
    if separate {
        for (index, (instruction, ceiling)) in instructions.iter().zip(&ceilings).enumerate() {
            let signers = if index == 0 { vec![&mint] } else { vec![] };
            measured(
                svm,
                payer,
                std::slice::from_ref(instruction),
                &signers,
                &format!("mint tax={tax_bps} max={maximum_metadata} step={index}"),
                *ceiling,
            );
        }
    } else {
        measured(
            svm,
            payer,
            &instructions,
            &[&mint],
            &format!("mint batch tax={tax_bps} max={maximum_metadata}"),
            ceilings.iter().sum(),
        );
    }
    use token::extension::{BaseStateWithExtensions, StateWithExtensions};
    let stored = svm.get_account(&address).unwrap();
    let state = StateWithExtensions::<token::state::Mint>::unpack(&stored.data).unwrap();
    let actual: metadata::state::TokenMetadata = state.get_variable_len_extension().unwrap();
    assert_eq!(
        (actual.name, actual.symbol, actual.uri),
        (name, symbol, uri)
    );
    address
}

#[test]
fn token_compute_profiles_cover_creation_and_maximum_metadata() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    for tax_bps in [0, 100, 300] {
        for maximum_metadata in [false, true] {
            for separate in [false, true] {
                metadata_mint(&mut svm, &payer, tax_bps, maximum_metadata, separate);
            }
        }
    }
}

#[test]
fn token_compute_profiles_cover_fresh_and_existing_associated_accounts() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    let legacy_mint = create_mint(&mut svm, &payer, payer.pubkey(), 6);
    let extended_mint = metadata_mint(&mut svm, &payer, 300, true, false);
    for (mint, program) in [
        (legacy_mint, anchor_spl::token::ID),
        (extended_mint, anchor_spl::token_2022::ID),
    ] {
        let low_bump_owner = (0u64..65_536)
            .find_map(|index| {
                let mut seed = [0u8; 32];
                seed[..8].copy_from_slice(&index.to_le_bytes());
                let owner = Address::new_from_array(seed);
                let (_, bump) = Address::find_program_address(
                    &[owner.as_ref(), program.as_ref(), mint.as_ref()],
                    &associated_token::program::ID,
                );
                (bump <= 244).then_some(owner)
            })
            .expect("low-bump associated account fixture");
        for owner in (0..32)
            .map(|_| Keypair::new().pubkey())
            .chain([low_bump_owner])
        {
            let (address, bump) = Address::find_program_address(
                &[owner.as_ref(), program.as_ref(), mint.as_ref()],
                &associated_token::program::ID,
            );
            let create = associated_token::instruction::create_associated_token_account_idempotent(
                &payer.pubkey(),
                &owner,
                &mint,
                &program,
            );
            for existing in [false, true] {
                measured(
                    &mut svm,
                    &payer,
                    std::slice::from_ref(&create),
                    &[],
                    &format!("ata existing={existing} bump={bump} program={program}"),
                    25_000 + 1_500 * u64::from(255 - bump),
                );
                assert_eq!(svm.get_account(&address).unwrap().owner, program);
            }
        }
    }
}

#[test]
fn token_compute_profiles_cover_transfers_native_cleanup_and_authority() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    let mint = create_mint(&mut svm, &payer, payer.pubkey(), 6);
    let source = create_token_account(&mut svm, &payer, mint, payer.pubkey());
    let recipient = Keypair::new().pubkey();
    let destination = create_token_account(&mut svm, &payer, mint, recipient);
    mint_to(&mut svm, &payer, mint, source, 1_000_000);
    let transfer = anchor_spl::token::spl_token::instruction::transfer(
        &anchor_spl::token::ID,
        &source,
        &destination,
        &payer.pubkey(),
        &[],
        100,
    )
    .unwrap();
    measured(&mut svm, &payer, &[transfer], &[], "spl transfer", 6_000);
    let checked = token::instruction::transfer_checked(
        &anchor_spl::token::ID,
        &source,
        &mint,
        &destination,
        &payer.pubkey(),
        &[],
        100,
        6,
    )
    .unwrap();
    measured(
        &mut svm,
        &payer,
        &[checked],
        &[],
        "spl checked transfer",
        7_000,
    );
    litesvm_token::create_native_mint(&mut svm);
    let wrapped = create_token_account(
        &mut svm,
        &payer,
        anchor_spl::token::spl_token::native_mint::ID,
        payer.pubkey(),
    );
    let funding = system_instruction::transfer(&payer.pubkey(), &wrapped, 10_000_000);
    measured(&mut svm, &payer, &[funding], &[], "system transfer", 150);
    let sync = token::instruction::sync_native(&anchor_spl::token::ID, &wrapped).unwrap();
    measured(&mut svm, &payer, &[sync], &[], "sync native", 4_000);
    let close = token::instruction::close_account(
        &anchor_spl::token::ID,
        &wrapped,
        &payer.pubkey(),
        &payer.pubkey(),
        &[],
    )
    .unwrap();
    measured(&mut svm, &payer, &[close], &[], "close native", 4_000);
    assert!(svm.get_account(&wrapped).is_none());
    let extended = metadata_mint(&mut svm, &payer, 0, false, false);
    let revoke = token::instruction::set_authority(
        &anchor_spl::token_2022::ID,
        &extended,
        None,
        token::instruction::AuthorityType::MintTokens,
        &payer.pubkey(),
        &[],
    )
    .unwrap();
    measured(
        &mut svm,
        &payer,
        &[revoke],
        &[],
        "revoke mint authority",
        2_000,
    );
    assert_eq!(token_account(&svm, source).amount, 999_800);
    assert_eq!(token_account(&svm, destination).amount, 200);
}
