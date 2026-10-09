//! BIP-32 derivation (Ethereum path) and EIP-55 checksummed addresses.
//! Path: m/44'/60'/0'/0/index — standard, well-known, easy to import.

use bip32::{DerivationPath, ExtendedPrivateKey};
use k256::ecdsa::SigningKey;
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use crate::error::CoreError;
use crate::CoreResult;

/// Derive the signing key for `index` from a 64-byte seed.
pub fn derive_signing_key(seed: &[u8; 64], index: u32) -> CoreResult<SigningKey> {
    let path: DerivationPath =
        format!("m/44'/60'/0'/0/{index}")
            .parse()
            .map_err(|e| CoreError::Derivation {
                path: format!("m/44'/60'/0'/0/{index}"),
                note: format!("path parse failed: {e}"),
            })?;
    let xprv = ExtendedPrivateKey::<SigningKey>::derive_from_path(seed, &path).map_err(|e| {
        CoreError::Derivation {
            path: format!("m/44'/60'/0'/0/{index}"),
            note: e.to_string(),
        }
    })?;
    Ok(xprv.private_key().clone())
}

/// Uncompressed public key (65 bytes, 0x04-prefixed) for a signing key.
pub fn public_key_bytes(sk: &SigningKey) -> [u8; 65] {
    let mut out = [0u8; 65];
    out.copy_from_slice(sk.verifying_key().to_encoded_point(false).as_bytes());
    out
}

/// Raw 20-byte ethereum address for a signing key.
pub fn raw_address(sk: &SigningKey) -> [u8; 20] {
    let pk = public_key_bytes(sk);
    let mut scratch = Zeroizing::new([0u8; 32]);
    scratch.copy_from_slice(&Keccak256::digest(&pk[1..]));
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&scratch[12..]);
    addr
}

/// EIP-55 mixed-case checksum encoding of a raw address.
pub fn eip55(addr: &[u8; 20]) -> String {
    let lower = hex::encode(addr);
    let hash = Keccak256::digest(lower.as_bytes());
    let mut out = String::with_capacity(42);
    out.push_str("0x");
    for (i, ch) in lower.chars().enumerate() {
        let nibble = if i % 2 == 0 {
            hash[i / 2] >> 4
        } else {
            hash[i / 2] & 0x0F
        };
        if ch.is_ascii_alphabetic() && nibble >= 8 {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Convenience: seed index -> display address.
pub fn address_for(seed: &[u8; 64], index: u32) -> CoreResult<String> {
    let sk = derive_signing_key(seed, index)?;
    Ok(eip55(&raw_address(&sk)))
}

/// Fingerprint: first 8 bytes of SHA-256 over the seed, hex.
/// Lets the UI tell wallets apart without exposing anything derivable.
pub fn fingerprint(seed: &[u8; 64]) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(seed);
    hex::encode(&digest[..8])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::mnemonic;

    fn seed_from(phrase: &str) -> Zeroizing<[u8; 64]> {
        mnemonic::parse(phrase).expect("parse").to_seed()
    }

    // The canonical test mnemonic from BIP-39 / trezor vectors.
    const VEC_PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn eip55_known_vectors() {
        let cases: [(&str, &str); 4] = [
            (
                "5aaeb6053f3e94c9b9a09f33669435e7ef1beaed",
                "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            ),
            (
                "fb6916095ca1df60bb79ce92ce3ea74c37c5d359",
                "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            ),
            (
                "dbf03b407c01e7cd3cbea99509d93f8dddc8c6fb",
                "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            ),
            (
                "d1220a0cf47c7b9be7a2e6ba89f429762e7b9adb",
                "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
            ),
        ];
        for (raw, want) in cases {
            let mut addr = [0u8; 20];
            hex::decode_to_slice(raw, &mut addr).expect("hex");
            assert_eq!(eip55(&addr), want, "vector {raw}");
        }
    }

    #[test]
    fn derivation_is_deterministic_and_indexed() {
        let seed = seed_from(VEC_PHRASE);
        let a0 = address_for(&seed, 0).expect("addr 0");
        let a0_again = address_for(&seed, 0).expect("addr 0 again");
        let a1 = address_for(&seed, 1).expect("addr 1");
        assert_eq!(a0, a0_again, "same seed+index must repeat");
        assert_ne!(a0, a1, "different indices must diverge");
        assert!(a0.starts_with("0x"));
        assert_eq!(a0.len(), 42);
    }

    #[test]
    fn derivation_matches_known_vector() {
        // seed of the canonical 12-word vector, path m/44'/60'/0'/0/0:
        // a public on-chain fact of the trezor test vector set
        let seed = seed_from(VEC_PHRASE);
        let a0 = address_for(&seed, 0).expect("addr");
        assert_eq!(
            a0.to_lowercase(),
            "0x9858effd232b4033e47d90003d41ec34ecaeda94"
        );
    }

    #[test]
    fn fingerprint_is_stable_short_hex() {
        let seed = seed_from(VEC_PHRASE);
        let f1 = fingerprint(&seed);
        let f2 = fingerprint(&seed);
        assert_eq!(f1, f2);
        assert_eq!(f1.len(), 16);
        assert!(f1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn raw_address_is_zeroized_scratch_not_leaking() {
        // structural test: scratch lives in Zeroizing; public function only
        // returns the 20-byte address
        let seed = seed_from(VEC_PHRASE);
        let sk = derive_signing_key(&seed, 3).expect("derive");
        let addr = raw_address(&sk);
        assert_eq!(addr.len(), 20);
    }
}
