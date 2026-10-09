//! BIP-39 mnemonic handling. Generation is OS-entropy only; parsing is
//! checksum-verified. Words never leave this module un-wrapped.

use bip39::{Language, Mnemonic};
use zeroize::Zeroizing;

use crate::error::CoreError;
use crate::CoreResult;

/// Supported mnemonic lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordCount {
    Twelve = 12,
    Fifteen = 15,
    Eighteen = 18,
    TwentyOne = 21,
    TwentyFour = 24,
}

// serde impl without an extra crate dependency
use serde::{Deserialize, Deserializer, Serialize, Serializer};

impl WordCount {
    pub fn from_u16(n: u16) -> CoreResult<Self> {
        match n {
            12 => Ok(Self::Twelve),
            15 => Ok(Self::Fifteen),
            18 => Ok(Self::Eighteen),
            21 => Ok(Self::TwentyOne),
            24 => Ok(Self::TwentyFour),
            other => Err(CoreError::Mnemonic(format!(
                "unsupported word count {other} (allowed: 12, 15, 18, 21, 24)"
            ))),
        }
    }

    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

impl Serialize for WordCount {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u16(self.as_u16())
    }
}

impl<'de> Deserialize<'de> for WordCount {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let n = u16::deserialize(d)?;
        Self::from_u16(n).map_err(serde::de::Error::custom)
    }
}

/// Generate a fresh mnemonic from the OS CSPRNG. The phrase is returned
/// exactly once, wrapped; nothing retains it.
pub fn generate(words: WordCount) -> CoreResult<Zeroizing<String>> {
    let m = Mnemonic::generate_in(Language::English, words.as_u16() as usize)
        .map_err(|e| CoreError::Mnemonic(format!("generator failure: {e}")))?;
    Ok(Zeroizing::new(m.to_string()))
}

/// Parse and checksum-verify a phrase the user typed back.
pub fn parse(phrase: &str) -> CoreResult<VerifiedMnemonic> {
    let trimmed = phrase.trim();
    if trimmed.is_empty() {
        return Err(CoreError::Mnemonic("phrase is empty".into()));
    }
    let normalized = Zeroizing::new(trimmed.split_whitespace().collect::<Vec<_>>().join(" "));
    let m = Mnemonic::parse_in(Language::English, normalized.as_str())
        .map_err(|_| CoreError::Mnemonic("checksum failed: phrase rejected".into()))?;
    Ok(VerifiedMnemonic { inner: m })
}

/// A mnemonic that passed checksum. It zeroizes on drop (bip39 crate).
pub struct VerifiedMnemonic {
    inner: Mnemonic,
}

impl VerifiedMnemonic {
    /// 64-byte BIP-39 seed (PBKDF2-HMAC-SHA512, 2048 rounds, no passphrase).
    pub fn to_seed(&self) -> Zeroizing<[u8; 64]> {
        Zeroizing::new(self.inner.to_seed_normalized(""))
    }

    pub fn word_count(&self) -> u16 {
        self.inner.word_count() as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_all_lengths() {
        for w in [
            WordCount::Twelve,
            WordCount::Fifteen,
            WordCount::Eighteen,
            WordCount::TwentyOne,
            WordCount::TwentyFour,
        ] {
            let phrase = generate(w).expect("generate");
            assert_eq!(phrase.split_whitespace().count(), w.as_u16() as usize);
        }
    }

    #[test]
    fn generate_is_entropy_backed() {
        // two generations must never collide in practice
        let a = generate(WordCount::Twelve).expect("a");
        let b = generate(WordCount::Twelve).expect("b");
        assert_ne!(a.as_str(), b.as_str());
    }

    #[test]
    fn parse_roundtrip_is_deterministic_seed() {
        let phrase = generate(WordCount::Twelve).expect("generate");
        let v1 = parse(phrase.as_str()).expect("parse 1");
        let v2 = parse(phrase.as_str()).expect("parse 2");
        assert_eq!(v1.to_seed().as_slice(), v2.to_seed().as_slice());
        assert_eq!(v1.word_count(), 12);
    }

    #[test]
    fn parse_rejects_bad_checksum() {
        // valid BIP-39 words, impossible checksum combination
        let bad = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        assert!(parse(bad).is_err()); // 11 words AND bad count
    }

    #[test]
    fn parse_rejects_non_word_junk() {
        assert!(parse("not even close to a real mnemonic phrase here friend").is_err());
        assert!(parse("").is_err());
        assert!(parse("   ").is_err());
    }

    #[test]
    fn word_count_validation() {
        assert!(WordCount::from_u16(12).is_ok());
        assert!(WordCount::from_u16(13).is_err());
        assert!(WordCount::from_u16(24).is_ok());
        assert!(WordCount::from_u16(0).is_err());
    }
}
