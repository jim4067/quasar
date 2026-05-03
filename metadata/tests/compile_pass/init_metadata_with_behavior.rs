//! Compile-pass: init metadata account with behavior module.
#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_metadata::{accounts::metadata, *};

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Accounts)]
pub struct InitMetadataAccount {
    #[account(mut)]
    pub payer: Signer,
    pub metadata_program: Program<MetadataProgram>,
    pub system_program: Program<SystemProgram>,
    pub rent: Sysvar<Rent>,
    pub mint: UncheckedAccount,
    pub mint_authority: Signer,
    pub update_authority: Signer,

    #[account(
        init,
        payer = payer,
        metadata(
            program = metadata_program,
            mint = mint,
            mint_authority = mint_authority,
            update_authority = update_authority,
            system_program = system_program,
            rent = rent,
            name = "My NFT",
            symbol = "NFT",
            uri = "https://example.com/meta.json",
            seller_fee_basis_points = 500,
            is_mutable = true,
        )
    )]
    pub metadata: Account<MetadataAccount>,
}

fn main() {}
