#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::{AssociatedTokenProgram, Mint, Token, TokenProgram};

solana_address::declare_id!("11111111111111111111111111111112");

// ERROR: system_program override points to a token program field.
#[derive(Accounts)]
pub struct BadSystemOverride {
    pub authority: Signer,
    pub mint: Account<Mint>,

    #[account(
        init,
        associated_token(
            mint = mint,
            authority = authority,
            system_program = token_program,
        )
    )]
    pub ata: InterfaceAccount<Token>,

    pub payer: Signer,
    pub token_program: Program<TokenProgram>,
    pub system_program: Program<SystemProgram>,
    pub ata_program: Program<AssociatedTokenProgram>,
}

fn main() {}
