//! Shell-wide state. One signer thread, at most one proxy, one flag for
//! the capture shield.

use std::sync::atomic::AtomicBool;
use std::thread::JoinHandle;
use std::time::Instant;

use brera_core::error::CoreError;
use brera_core::proxy::ProxyServer;
use brera_core::wallet::SignerHandle;

pub struct AppState {
    pub signer: SignerHandle,
    /// Kept so the signer thread is joined on orderly shutdown paths.
    pub signer_join: std::sync::Mutex<Option<JoinHandle<()>>>,
    pub proxy: parking_lot::Mutex<Option<ProxyServer>>,
    pub protection: AtomicBool,
    pub started: Instant,
}

impl AppState {
    pub fn new() -> Result<Self, CoreError> {
        let (signer, join) = SignerHandle::spawn()?;
        Ok(Self {
            signer,
            signer_join: std::sync::Mutex::new(Some(join)),
            proxy: parking_lot::Mutex::new(None),
            protection: AtomicBool::new(true),
            started: Instant::now(),
        })
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}
