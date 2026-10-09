//! Isolated signer thread.
//!
//! The vault lives on a dedicated OS thread. The UI (and the shell) talk to
//! it through jobs over a channel. There is no API surface through which a
//! caller could extract the seed or a raw key — only signatures and
//! addresses come back out.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use zeroize::Zeroizing;

use super::mnemonic::WordCount;
use super::vault::{SignReceipt, Vault, VaultReceipt, VaultStatus};
use crate::error::CoreError;
use crate::memguard::PurgeReport;
use crate::CoreResult;

/// One unit of work for the signer thread.
pub enum SignerJob {
    /// Generate a mnemonic; vault stays locked afterwards.
    Generate {
        words: WordCount,
        reply: mpsc::Sender<CoreResult<VaultReceipt>>,
    },
    /// Unlock with a phrase (create-or-restore).
    Unlock {
        phrase: Zeroizing<String>,
        reply: mpsc::Sender<CoreResult<VaultStatus>>,
    },
    /// Lock + scrub.
    Lock { reply: mpsc::Sender<PurgeReport> },
    /// Status snapshot.
    Status { reply: mpsc::Sender<VaultStatus> },
    /// Address at index (requires unlocked).
    Address {
        index: u32,
        reply: mpsc::Sender<CoreResult<String>>,
    },
    /// Sign payload at index (requires unlocked).
    Sign {
        payload: Vec<u8>,
        index: u32,
        reply: mpsc::Sender<CoreResult<SignReceipt>>,
    },
    /// Ask the thread to exit.
    Shutdown { reply: mpsc::Sender<PurgeReport> },
}

/// Cheap clonable handle to the signer thread.
#[derive(Clone)]
pub struct SignerHandle {
    tx: mpsc::SyncSender<SignerJob>,
}

impl SignerHandle {
    /// Spawn the signer thread. It owns the only vault instance.
    pub fn spawn() -> CoreResult<(Self, JoinHandle<()>)> {
        let (tx, rx) = mpsc::sync_channel::<SignerJob>(64);
        let join = thread::Builder::new()
            .name("brera-signer".into())
            .spawn(move || {
                let mut vault = Vault::locked();
                for job in rx {
                    match job {
                        SignerJob::Generate { words, reply } => {
                            let _ = reply.send(vault.generate(words));
                        }
                        SignerJob::Unlock { phrase, reply } => {
                            let _ = reply.send(vault.unlock(phrase.as_str()));
                        }
                        SignerJob::Lock { reply } => {
                            let _ = reply.send(vault.lock());
                        }
                        SignerJob::Status { reply } => {
                            let _ = reply.send(vault.status());
                        }
                        SignerJob::Address { index, reply } => {
                            let _ = reply.send(vault.address(index));
                        }
                        SignerJob::Sign {
                            payload,
                            index,
                            reply,
                        } => {
                            let _ = reply.send(vault.sign(&payload, index));
                        }
                        SignerJob::Shutdown { reply } => {
                            let _ = reply.send(vault.lock());
                            break;
                        }
                    }
                }
                // if the loop exits by disconnect, the vault still drops locked
            })
            .map_err(|e| CoreError::Other(format!("signer spawn failed: {e}")))?;
        Ok((Self { tx }, join))
    }

    fn request<R>(&self, make: impl FnOnce(mpsc::Sender<R>) -> SignerJob) -> CoreResult<R> {
        let (rtx, rrx) = mpsc::channel();
        self.tx
            .send(make(rtx))
            .map_err(|_| CoreError::SignerClosed)?;
        rrx.recv().map_err(|_| CoreError::SignerClosed)
    }

    pub fn generate(&self, words: WordCount) -> CoreResult<VaultReceipt> {
        self.request(|reply| SignerJob::Generate { words, reply })
            .and_then(|inner| inner)
    }

    pub fn unlock(&self, phrase: Zeroizing<String>) -> CoreResult<VaultStatus> {
        self.request(|reply| SignerJob::Unlock { phrase, reply })
            .and_then(|inner| inner)
    }

    pub fn lock(&self) -> CoreResult<PurgeReport> {
        self.request(|reply| SignerJob::Lock { reply })
    }

    pub fn status(&self) -> CoreResult<VaultStatus> {
        self.request(|reply| SignerJob::Status { reply })
    }

    pub fn address(&self, index: u32) -> CoreResult<String> {
        self.request(|reply| SignerJob::Address { index, reply })
            .and_then(|inner| inner)
    }

    pub fn sign(&self, payload: Vec<u8>, index: u32) -> CoreResult<SignReceipt> {
        self.request(|reply| SignerJob::Sign {
            payload,
            index,
            reply,
        })
        .and_then(|inner| inner)
    }

    pub fn shutdown(&self) -> CoreResult<PurgeReport> {
        self.request(|reply| SignerJob::Shutdown { reply })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_lifecycle_through_the_thread() {
        let (handle, join) = SignerHandle::spawn().expect("spawn");

        // status starts locked
        let st = handle.status().expect("status");
        assert!(!st.unlocked);

        // generate -> stays locked -> unlock -> use
        let receipt = handle.generate(WordCount::Twelve).expect("generate");
        let st = handle.status().expect("status2");
        assert!(!st.unlocked);

        let st = handle
            .unlock(Zeroizing::new(receipt.mnemonic.clone()))
            .expect("unlock");
        assert!(st.unlocked);

        let addr = handle.address(0).expect("addr");
        assert!(addr.starts_with("0x"));
        let signed = handle.sign(b"payload".to_vec(), 0).expect("sign");
        assert_eq!(signed.address, addr);
        assert_eq!(signed.signature.len(), 128); // r||s hex

        // locked-after-lock refuses
        handle.lock().expect("lock");
        assert!(matches!(handle.address(0), Err(CoreError::VaultLocked)));

        let purge = handle.shutdown().expect("shutdown");
        assert!(purge.vault_locked);
        join.join().expect("thread exits cleanly");
    }

    #[test]
    fn bad_phrase_is_rejected_through_channel() {
        let (handle, _join) = SignerHandle::spawn().expect("spawn");
        let err = handle
            .unlock(Zeroizing::new("junk junk junk".into()))
            .expect_err("must reject");
        assert!(matches!(err, CoreError::Mnemonic(_)));
    }
}
