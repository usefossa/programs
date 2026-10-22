use crate::helpers::require;
use pinocchio::{cpi::Signer, instruction::seeds, AccountView, Address, ProgramResult};
use pinocchio_token::{
    instructions::transfer_checked::TransferChecked, state::Account as TokenAccount, TokenProgram,
};

pub(crate) fn token_owned_by(
    token: &AccountView,
    mint: &Address,
    owner: &Address,
) -> ProgramResult {
    let d = TokenAccount::from_account_view(token)?;
    require(
        d.is_initialized() && d.mint() == mint && d.owner() == owner,
        2,
    )
}
pub(crate) fn vault_owned_by(
    vault: &AccountView,
    mint: &Address,
    authority: &Address,
) -> ProgramResult {
    token_owned_by(vault, mint, authority)?;
    let d = TokenAccount::from_account_view(vault)?;
    require(
        !d.has_delegate() && !d.has_close_authority() && !d.is_native(),
        2,
    )
}
pub(crate) fn user_vault(
    vault: &AccountView,
    mint: &AccountView,
    md: &[u8],
    u: &AccountView,
    ud: &[u8],
    program: &Address,
) -> ProgramResult {
    require(
        &ud[80..112] == vault.address().as_ref() && &md[40..72] == mint.address().as_ref(),
        2,
    )?;
    let authority =
        Address::derive_address(&[b"vault", u.address().as_ref()], Some(ud[112]), program);
    vault_owned_by(vault, mint.address(), &authority)
}
pub(crate) fn bond_vault(
    accounts: [&AccountView; 2],
    md: &[u8],
    model: &Address,
    slot: u32,
    stored: &[u8],
    bump: u8,
    program: &Address,
) -> ProgramResult {
    let [vault, mint] = accounts;
    require(
        stored == vault.address().as_ref() && &md[40..72] == mint.address().as_ref(),
        2,
    )?;
    let authority = Address::derive_address(
        &[b"bond", model.as_ref(), &slot.to_le_bytes()],
        Some(bump),
        program,
    );
    vault_owned_by(vault, mint.address(), &authority)
}
pub(crate) fn transfer(
    from: &AccountView,
    mint: &AccountView,
    to: &AccountView,
    authority: &AccountView,
    amount: u64,
) -> ProgramResult {
    TransferChecked::<&AccountView, TokenProgram>::new(from, mint, to, authority, amount, 6)
        .invoke()
}
pub(crate) fn user_transfer(
    accounts: [&AccountView; 4],
    lane: &Address,
    bump: u8,
    program: &Address,
    amount: u64,
) -> ProgramResult {
    let [from, mint, to, authority] = accounts;
    require(
        authority.address()
            == &Address::derive_address(&[b"vault", lane.as_ref()], Some(bump), program),
        2,
    )?;
    let bump_ref = [bump];
    let signer_seeds = seeds!(b"vault", lane.as_ref(), &bump_ref);
    TransferChecked::<&AccountView, TokenProgram>::new(from, mint, to, authority, amount, 6)
        .invoke_signed(&[Signer::from(&signer_seeds)])
}
pub(crate) fn bond_transfer(
    accounts: [&AccountView; 4],
    model: &Address,
    slot: u32,
    bump: u8,
    program: &Address,
    amount: u64,
) -> ProgramResult {
    let [from, mint, to, authority] = accounts;
    let slot_ref = slot.to_le_bytes();
    require(
        authority.address()
            == &Address::derive_address(&[b"bond", model.as_ref(), &slot_ref], Some(bump), program),
        2,
    )?;
    let bump_ref = [bump];
    let signer_seeds = seeds!(b"bond", model.as_ref(), &slot_ref, &bump_ref);
    TransferChecked::<&AccountView, TokenProgram>::new(from, mint, to, authority, amount, 6)
        .invoke_signed(&[Signer::from(&signer_seeds)])
}
