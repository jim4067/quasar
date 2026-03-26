use solana_address::Address;

pub const ID: Address = solana_address::address!("22222222222222222222222222222222222222222222");

pub mod instructions;
pub mod state;
pub mod events;
pub mod pda;

pub use instructions::*;
pub use state::*;
pub use events::*;
pub use pda::*;
