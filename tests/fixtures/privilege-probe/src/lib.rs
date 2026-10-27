#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
pub const ID: Address = Address::new_from_array([10; 32]);

#[derive(Accounts)]
pub struct Probe {}

#[program]
pub mod privilege_probe {
    use super::*;
    #[discrim = 9]
    pub fn launch(ctx: &mut Context<Probe>, _payload: [u8; 40]) -> Result<()> {
        let accounts = ctx.remaining_accounts()?;
        require!(accounts.len() == 17, ProgramError::Custom(99));
        let flags: Vec<_> = accounts
            .iter()
            .map(|a| u8::from(a.is_writable()) | (u8::from(a.is_signer()) << 1))
            .collect();
        anchor_lang::sol_log_data(&[b"parity/cpi-flags", &flags]);
        Ok(())
    }
}
