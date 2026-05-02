#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::{TokenProgram, *};

solana_address::declare_id!("11111111111111111111111111111112");

// ERROR: system_program is not a token program type.
#[derive(Accounts)]
pub struct BadProgramOverride {
    pub authority: Signer,
    pub mint: Account<Mint>,

    #[account(token(mint = mint, authority = authority, token_program = system_program))]
    pub vault: Account<Token>,

    pub system_program: Program<SystemProgram>,
}

fn main() {}
