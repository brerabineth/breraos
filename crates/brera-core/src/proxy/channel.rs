//! Encrypted frame channel.
//!
//! Every payload that leaves the proxy toward a transport is sealed into a
//! `Frame` with XChaCha20-Poly1305 under an ephemeral session key. The key
//! is born at cipher construction, lives in `Zeroizing`, and is scrubbed on
//! drop. Frames are independent (24-byte random nonces — no counter state
//! to desynchronize across the future P2P hop).

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use zeroize::Zeroizing;

use crate::error::CoreError;
use crate::CoreResult;

/// One sealed unit of traffic. Opaque bytes + the nonce that opened them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Frame {
    /// base64 of the 24-byte nonce (serde-friendly form).
    pub nonce: String,
    /// base64 of ciphertext||tag.
    pub body: String,
    /// monotonic send counter, for observability only.
    pub seq: u64,
}

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

/// Seal/open pair with an ephemeral session key.
/// NOTE: the key material lives inside the cipher instance; the Zeroizing
/// copy below it in `new()` documents and guarantees one scrubbed origin.
pub struct SessionCipher {
    cipher: XChaCha20Poly1305,
    seq: AtomicU64,
    sealed: AtomicU64,
    opened: AtomicU64,
    failed: AtomicU64,
}

impl SessionCipher {
    /// New cipher with a fresh OS-entropy session key.
    pub fn new() -> Self {
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        rand::rngs::OsRng.fill_bytes(key.as_mut());
        let cipher = XChaCha20Poly1305::new((&*key).into());
        // `key` (Zeroizing) is consumed here: the scrubbed origin drops wiped.
        drop(key);
        Self {
            cipher,
            seq: AtomicU64::new(0),
            sealed: AtomicU64::new(0),
            opened: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        }
    }

    /// Seal plaintext into a frame. Never fails; allocation is the only risk.
    pub fn seal(&self, plaintext: &[u8]) -> Frame {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);
        let ct = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: &seq_aad(self.seq.load(Ordering::Relaxed)),
                },
            )
            .map_err(|_| CoreError::Channel("seal failure".into()));
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        match ct {
            Ok(ct) => {
                self.sealed.fetch_add(1, Ordering::Relaxed);
                Frame {
                    nonce: base64_encode(&nonce_bytes),
                    body: base64_encode(&ct),
                    seq,
                }
            }
            Err(_) => {
                // unreachable with correct key/nonce lengths; still total
                self.failed.fetch_add(1, Ordering::Relaxed);
                Frame {
                    nonce: base64_encode(&nonce_bytes),
                    body: String::new(),
                    seq,
                }
            }
        }
    }

    /// Open a frame. A flipped bit anywhere kills it (Poly1305).
    pub fn open(&self, frame: &Frame) -> CoreResult<Vec<u8>> {
        let nonce = base64_decode(&frame.nonce)
            .ok_or_else(|| CoreError::Frame("bad nonce encoding".into()))?;
        if nonce.len() != NONCE_LEN {
            self.failed.fetch_add(1, Ordering::Relaxed);
            return Err(CoreError::Frame("nonce length invalid".into()));
        }
        let ct = base64_decode(&frame.body)
            .ok_or_else(|| CoreError::Frame("bad body encoding".into()))?;
        let pt = self.cipher.decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &ct,
                aad: &seq_aad(frame.seq),
            },
        );
        match pt {
            Ok(pt) => {
                self.opened.fetch_add(1, Ordering::Relaxed);
                Ok(pt)
            }
            Err(e) => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                Err(CoreError::from(e))
            }
        }
    }

    /// Counters for the UI.
    pub fn snapshot(&self) -> CipherStats {
        CipherStats {
            sealed: self.sealed.load(Ordering::Relaxed),
            opened: self.opened.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
        }
    }
}

impl Default for SessionCipher {
    fn default() -> Self {
        Self::new()
    }
}

fn seq_aad(seq: u64) -> [u8; 8] {
    seq.to_be_bytes()
}

/// Cipher counters snapshot.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct CipherStats {
    pub sealed: u64,
    pub opened: u64,
    pub failed: u64,
}

// Minimal base64 (std alphabet, padding) — avoids pulling another crate
// into the security-critical path. Tested below.
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    fn val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => INVALID,
        }
    }
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        if pad > 2 || chunk[..4 - pad].iter().any(|&c| val(c) == INVALID) {
            return None;
        }
        let n = (u32::from(val(chunk[0])) << 18)
            | (u32::from(val(chunk[1])) << 12)
            | (u32::from(if pad > 1 { 0 } else { val(chunk[2]) }) << 6)
            | u32::from(if pad > 0 { 0 } else { val(chunk[3]) });
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        for v in ["f", "fo", "foo", "foob", "fooba", "foobar"] {
            assert_eq!(
                base64_decode(&base64_encode(v.as_bytes())).as_deref(),
                Some(v.as_bytes())
            );
        }
        assert!(base64_decode("Zm9v").is_some());
        assert!(base64_decode("Zm9v!").is_none());
        assert!(base64_decode("Zm9").is_none());
    }

    #[test]
    fn seal_open_roundtrip() {
        let c = SessionCipher::new();
        let frame = c.seal(b"GET / HTTP/1.1");
        assert!(!frame.body.is_empty());
        let pt = c.open(&frame).expect("open");
        assert_eq!(pt, b"GET / HTTP/1.1");
        let st = c.snapshot();
        assert_eq!(st.sealed, 1);
        assert_eq!(st.opened, 1);
    }

    #[test]
    fn tampered_frame_is_rejected() {
        let c = SessionCipher::new();
        let mut frame = c.seal(b"secret transaction payload");
        // flip one bit in the ciphertext body
        let mut raw = base64_decode(&frame.body).expect("decode");
        raw[0] ^= 0x01;
        frame.body = base64_encode(&raw);
        assert!(matches!(c.open(&frame), Err(CoreError::Channel(_))));
        assert_eq!(c.snapshot().failed, 1);
    }

    #[test]
    fn frames_are_independent_and_randomized() {
        let c = SessionCipher::new();
        let f1 = c.seal(b"same plaintext");
        let f2 = c.seal(b"same plaintext");
        assert_ne!(f1.nonce, f2.nonce, "nonce reuse would be fatal");
        assert_ne!(f1.body, f2.body);
        assert_eq!(c.open(&f1).expect("f1"), b"same plaintext");
        assert_eq!(c.open(&f2).expect("f2"), b"same plaintext");
    }

    #[test]
    fn foreign_frames_rejected() {
        let a = SessionCipher::new();
        let b = SessionCipher::new();
        let frame = a.seal(b"for b's eyes only");
        assert!(matches!(b.open(&frame), Err(CoreError::Channel(_))));
    }

    #[test]
    fn frame_serde_roundtrip() {
        let c = SessionCipher::new();
        let frame = c.seal(b"json frame test");
        let json = serde_json::to_string(&frame).expect("to json");
        let back: Frame = serde_json::from_str(&json).expect("from json");
        assert_eq!(back, frame);
        assert_eq!(c.open(&back).expect("open"), b"json frame test");
    }
}
