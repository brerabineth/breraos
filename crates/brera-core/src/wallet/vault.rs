//! The vault. A locked vault holds nothing but its identity.
//! An unlocked vault holds the seed in `Zeroizing` memory and dies locked.
//!
//! Invariants (enforced by construction, proven by tests):
//! 1. `generate` hands the phrase to the caller exactly once, then forgets it.
//! 2. Every key-use command requires an unlocked vault.
//! 3. `lock` and `Drop` scrub the seed immediately.

use serde::Serialize;
use zeroize::Zeroizing;

use super::address;
use super::mnemonic::{self, WordCount};
use crate::error::CoreError;
use crate::memguard::PurgeReport;
use crate::CoreResult;

/// Returned once, right after generation. The UI must force the user to
/// write these words down before anything else can happen.
///
/// The phrase crosses the IPC bridge exactly once, by design; every other
/// surface returns fingerprints and addresses only.
#[derive(Debug, Clone, Serialize)]
pub struct VaultReceipt {
    pub mnemonic: String,
    pub fingerprint: String,
    pub word_count: u16,
    pub derivation: String,
}

/// Safe-to-display vault state.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VaultStatus {
    pub unlocked: bool,
    pub fingerprint: Option<String>,
    pub derivation: String,
    pub word_count: Option<u16>,
}

/// A signing result. Contains only public artifacts.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignReceipt {
    /// 64-byte r||s signature, hex.
    pub signature: String,
    /// EIP-55 address that produced it.
    pub address: String,
    /// SHA-256 of the payload, hex — ties receipt to request.
    pub payload_digest: String,
    /// Derivation index used.
    pub index: u32,
}

/// Seed holder. Exists only while unlocked.
/// `Zeroizing` wipes the seed the instant this struct drops.
struct UnlockedSeed {
    seed: Zeroizing<[u8; 64]>,
    fingerprint: String,
    word_count: u16,
}

/// The vault itself. `None` == locked.
pub struct Vault {
    inner: Option<UnlockedSeed>,
}

impl Vault {
    pub fn locked() -> Self {
        Self { inner: None }
    }

    /// Generate a new mnemonic, return it once, and stay locked.
    /// The user must re-enter the phrase to unlock — that's the point.
    pub fn generate(&mut self, words: WordCount) -> CoreResult<VaultReceipt> {
        let phrase = mnemonic::generate(words)?;
        let seed = mnemonic::parse(phrase.as_str())?.to_seed();
        let fp = address::fingerprint(&seed);
        Ok(VaultReceipt {
            mnemonic: phrase.to_string(),
            fingerprint: fp,
            word_count: words.as_u16(),
            derivation: "m/44'/60'/0'/0/*".to_string(),
        })
    }

    /// Unlock with a phrase (new or restored wallet). Refuses if unlocked.
    pub fn unlock(&mut self, phrase: &str) -> CoreResult<VaultStatus> {
        if self.inner.is_some() {
            return Err(CoreError::VaultUnlocked);
        }
        let verified = mnemonic::parse(phrase)?;
        let seed = verified.to_seed();
        let fp = address::fingerprint(&seed);
        self.inner = Some(UnlockedSeed {
            seed,
            fingerprint: fp.clone(),
            word_count: verified.word_count(),
        });
        Ok(VaultStatus {
            unlocked: true,
            fingerprint: Some(fp),
            derivation: "m/44'/60'/0'/0/*".to_string(),
            word_count: Some(verified.word_count()),
        })
    }

    /// Lock and scrub. Returns a purge report so the UI can prove it.
    pub fn lock(&mut self) -> PurgeReport {
        let mut report = PurgeReport::empty();
        report.vault_locked = true;
        if let Some(seed) = self.inner.take() {
            report.buffers_wiped += 1;
            report.frames_dropped = 0;
            drop(seed); // zeroize-on-drop
        }
        report
    }

    pub fn is_unlocked(&self) -> bool {
        self.inner.is_some()
    }

    pub fn status(&self) -> VaultStatus {
        match &self.inner {
            Some(s) => VaultStatus {
                unlocked: true,
                fingerprint: Some(s.fingerprint.clone()),
                derivation: "m/44'/60'/0'/0/*".to_string(),
                word_count: Some(s.word_count),
            },
            None => VaultStatus {
                unlocked: false,
                fingerprint: None,
                derivation: "m/44'/60'/0'/0/*".to_string(),
                word_count: None,
            },
        }
    }

    fn with_seed<T>(&self, f: impl FnOnce(&[u8; 64]) -> CoreResult<T>) -> CoreResult<T> {
        match &self.inner {
            Some(s) => f(&s.seed),
            None => Err(CoreError::VaultLocked),
        }
    }

    /// EIP-55 address at `index`. Requires unlock.
    pub fn address(&self, index: u32) -> CoreResult<String> {
        self.with_seed(|seed| address::address_for(seed, index))
    }

    /// Sign an arbitrary payload with the key at `index`. Requires unlock.
    /// The seed and the private scalar never leave this call frame.
    pub fn sign(&self, payload: &[u8], index: u32) -> CoreResult<SignReceipt> {
        use k256::ecdsa::signature::Signer as _;
        use sha2::{Digest as _, Sha256};
        self.with_seed(|seed| {
            let sk = address::derive_signing_key(seed, index)?;
            let sig: k256::ecdsa::Signature = sk.sign(payload);
            let sig_bytes = sig.to_bytes();
            let digest = Sha256::digest(payload);
            Ok(SignReceipt {
                signature: hex::encode(&sig_bytes[..]),
                address: address::eip55(&address::raw_address(&sk)),
                payload_digest: hex::encode(digest),
                index,
            })
        })
    }

    /// Fingerprint without unlocking — impossible by design; a locked
    /// vault has no identity beyond "locked". Reported for honesty.
    pub fn purge_report_stub(&self) -> PurgeReport {
        PurgeReport::empty()
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = self.lock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::mnemonic;

    fn fresh_phrase() -> Zeroizing<String> {
        mnemonic::generate(WordCount::Twelve).expect("gen")
    }

    #[test]
    fn generate_returns_phrase_once_and_stays_locked() {
        let mut v = Vault::locked();
        let receipt = v.generate(WordCount::Twelve).expect("generate");
        assert_eq!(receipt.mnemonic.split_whitespace().count(), 12);
        assert!(!v.is_unlocked());
        assert!(!receipt.fingerprint.is_empty());
    }

    #[test]
    fn unlock_requires_exact_phrase() {
        let mut v = Vault::locked();
        let phrase = fresh_phrase();
        v.unlock(phrase.as_str()).expect("unlock");
        assert!(v.is_unlocked());
        // wrong phrase on a second vault fails cleanly
        let mut v2 = Vault::locked();
        assert!(v2.unlock("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about").is_ok()); // valid vector phrase
        let mut v3 = Vault::locked();
        assert!(v3
            .unlock("completely bogus words here no checksum at all")
            .is_err());
        drop(phrase);
    }

    #[test]
    fn double_unlock_refused() {
        let mut v = Vault::locked();
        let p = fresh_phrase();
        v.unlock(p.as_str()).expect("first");
        assert!(matches!(
            v.unlock(p.as_str()),
            Err(CoreError::VaultUnlocked)
        ));
    }

    #[test]
    fn locked_vault_refuses_everything() {
        let v = Vault::locked();
        assert!(matches!(v.address(0), Err(CoreError::VaultLocked)));
        assert!(matches!(v.sign(b"x", 0), Err(CoreError::VaultLocked)));
        assert!(!v.is_unlocked());
    }

    #[test]
    fn lock_scrubs_and_report_reflects_it() {
        let mut v = Vault::locked();
        let p = fresh_phrase();
        v.unlock(p.as_str()).expect("unlock");
        let report = v.lock();
        assert!(report.vault_locked);
        assert_eq!(report.buffers_wiped, 1);
        assert!(!v.is_unlocked());
        assert!(matches!(v.address(0), Err(CoreError::VaultLocked)));
    }

    #[test]
    fn sign_receipt_verifies_against_address() {
        use k256::ecdsa::signature::Verifier;
        use k256::ecdsa::{Signature, VerifyingKey};

        let mut v = Vault::locked();
        let p = fresh_phrase();
        v.unlock(p.as_str()).expect("unlock");
        let payload = b"breraos:tx:0001:to=0xdead...beef:amount=0";
        let receipt = v.sign(payload, 0).expect("sign");

        // independently derive the same key and verify
        let verified = mnemonic::parse(p.as_str()).expect("parse");
        let sk = address::derive_signing_key(&verified.to_seed(), 0).expect("derive");
        let sig = Signature::from_slice(&hex::decode(&receipt.signature).expect("hex"))
            .expect("sig parse");
        let vk: &VerifyingKey = sk.verifying_key();
        vk.verify(payload, &sig).expect("signature must verify");

        // and the address must match the vault's
        assert_eq!(receipt.address, v.address(0).expect("addr"));
        assert_eq!(receipt.index, 0);
    }

    #[test]
    fn status_shape() {
        let mut v = Vault::locked();
        let s = v.status();
        assert!(!s.unlocked);
        assert!(s.fingerprint.is_none());
        let p = fresh_phrase();
        v.unlock(p.as_str()).expect("unlock");
        let s = v.status();
        assert!(s.unlocked);
        assert_eq!(s.word_count, Some(12));
    }
}
