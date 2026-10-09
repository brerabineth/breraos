//! Cold wallet stack. Seeds are generated, stored and used strictly inside
//! this module (and the signer thread). Nothing below ever returns key
//! material to a caller.

pub mod address;
pub mod mnemonic;
pub mod signer;
pub mod vault;

pub use mnemonic::WordCount;
pub use signer::{SignerHandle, SignerJob};
pub use vault::{SignReceipt, VaultReceipt, VaultStatus};

use serde::Serialize;

/// Public, safe-to-display wallet facts.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WalletFacts {
    pub fingerprint: String,
    pub address: String,
    pub derivation: String,
}
