//! brera-core: the Rust heart of BreraOS.
//!
//! Everything sensitive lives here, behind a wall:
//! - `wallet`:  BIP-39 vault, BIP-32 derivation, isolated signer thread
//! - `proxy`:   local intercept proxy, encrypted frame channel, transports
//! - `memguard`: scrubbing primitives and purge reporting
//!
//! The web layer never touches seeds or keys. It asks; the core decides.

pub mod error;
pub mod memguard;
pub mod proxy;
pub mod wallet;

pub use error::CoreError;

/// Result alias used across the crate.
pub type CoreResult<T> = Result<T, CoreError>;

/// Semantic version of the core, surfaced to the shell.
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
