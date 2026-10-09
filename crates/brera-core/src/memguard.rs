//! memguard — scrubbing primitives for anything sensitive that crosses
//! the core boundary, plus purge reporting for the UI.
//!
//! Rule of the house: keys are born inside `Zeroizing`, and every buffer
//! that ever carried a secret goes through `scrub` before it dies.

use serde::Serialize;
use zeroize::{Zeroize, Zeroizing};

/// Overwrite a mutable buffer with zeros. Cheap, explicit, honest.
pub fn scrub(buf: &mut [u8]) {
    buf.zeroize();
}

/// A byte buffer that is wiped on drop.
#[derive(Clone)]
pub struct SecureBuffer {
    label: &'static str,
    inner: Zeroizing<Vec<u8>>,
}

impl SecureBuffer {
    pub fn new(label: &'static str, data: Vec<u8>) -> Self {
        Self {
            label,
            inner: Zeroizing::new(data),
        }
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Read-only view. Callers must never copy out of it without zeroizing.
    pub fn expose(&self) -> &[u8] {
        &self.inner
    }
}

/// What happened during a memory purge. Serialized straight to the UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PurgeReport {
    pub buffers_wiped: u64,
    pub frames_dropped: u64,
    pub vault_locked: bool,
    pub duration_micros: u128,
}

impl PurgeReport {
    pub fn empty() -> Self {
        Self {
            buffers_wiped: 0,
            frames_dropped: 0,
            vault_locked: false,
            duration_micros: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_zeroes_everything() {
        let mut buf = vec![0xAAu8; 128];
        scrub(&mut buf);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn secure_buffer_keeps_label_and_len() {
        let sb = SecureBuffer::new("frame", vec![1, 2, 3, 4]);
        assert_eq!(sb.label(), "frame");
        assert_eq!(sb.len(), 4);
        assert!(!sb.is_empty());
        assert_eq!(sb.expose(), &[1, 2, 3, 4]);
    }

    #[test]
    fn purge_report_defaults() {
        let r = PurgeReport::empty();
        assert_eq!(r.buffers_wiped, 0);
        assert!(!r.vault_locked);
    }
}
