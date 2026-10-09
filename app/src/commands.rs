//! The IPC surface. Everything the web layer can ask for, and nothing it
//! can't. Errors cross the bridge as structured strings; secrets never do.

use serde::Serialize;
use tauri::State;

use brera_core::memguard::PurgeReport;
use brera_core::proxy::{ProxyConfig, ProxyServer, ProxySnapshot};
use brera_core::wallet::mnemonic::WordCount;
use brera_core::wallet::{SignReceipt, VaultReceipt, VaultStatus};
use brera_core::CoreResult;

use crate::state::AppState;

/// IPC error type. Tauri requires Serialize; the web layer gets a stable
/// `{ kind, message }` shape it can branch on without parsing prose.
#[derive(Debug, Clone, Serialize)]
pub struct CmdError {
    pub kind: String,
    pub message: String,
}

impl CmdError {
    pub fn of(err: brera_core::CoreError) -> Self {
        use brera_core::CoreError::*;
        let kind = match &err {
            Mnemonic(_) => "mnemonic",
            VaultLocked => "vault_locked",
            VaultUnlocked => "vault_unlocked",
            Derivation { .. } => "derivation",
            Channel(_) => "channel",
            Transport(_) => "transport",
            Proxy(_) => "proxy",
            SignerClosed => "signer_closed",
            PayloadTooLarge { .. } => "payload_too_large",
            Frame(_) => "frame",
            Io(_) => "io",
            Other(_) => "internal",
        };
        Self {
            kind: kind.to_string(),
            message: err.to_string(),
        }
    }

    fn s<T>(r: CoreResult<T>) -> Result<T, Self> {
        r.map_err(Self::of)
    }
}

type CmdResult<T> = Result<T, CmdError>;

// ---------------------------------------------------------------- system

/// Boot snapshot for the UI: one round trip, everything it needs.
#[derive(Debug, Clone, Serialize)]
pub struct SystemSnapshot {
    pub core_version: String,
    pub shell_version: String,
    pub uptime_secs: u64,
    pub capture_shield: bool,
    pub vault: VaultStatus,
    pub proxy: Option<ProxySnapshot>,
}

#[tauri::command]
pub fn system_snapshot(state: State<'_, AppState>) -> CmdResult<SystemSnapshot> {
    let proxy = state.proxy.lock().as_ref().map(ProxyServer::snapshot);
    Ok(SystemSnapshot {
        core_version: brera_core::CORE_VERSION.to_string(),
        shell_version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: state.uptime_secs(),
        capture_shield: state.protection.load(std::sync::atomic::Ordering::Relaxed),
        vault: CmdError::s(state.signer.status())?,
        proxy,
    })
}

// ----------------------------------------------------------------- vault

/// Generate a mnemonic. The phrase is delivered once and never again.
#[tauri::command]
pub fn vault_generate(words: u16, state: State<'_, AppState>) -> CmdResult<VaultReceipt> {
    let words = CmdError::s(WordCount::from_u16(words))?;
    CmdError::s(state.signer.generate(words))
}

/// Unlock with a phrase (12-24 words). Checksum-verified in core.
#[tauri::command]
pub fn vault_unlock(phrase: String, state: State<'_, AppState>) -> CmdResult<VaultStatus> {
    CmdError::s(state.signer.unlock(zeroize::Zeroizing::new(phrase)))
}

/// Lock + scrub the seed from RAM. Returns a purge receipt.
#[tauri::command]
pub fn vault_lock(state: State<'_, AppState>) -> CmdResult<PurgeReport> {
    CmdError::s(state.signer.lock())
}

#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> CmdResult<VaultStatus> {
    CmdError::s(state.signer.status())
}

// ---------------------------------------------------------------- wallet

#[tauri::command]
pub fn wallet_address(index: u32, state: State<'_, AppState>) -> CmdResult<String> {
    CmdError::s(state.signer.address(index))
}

#[tauri::command]
pub fn wallet_sign(
    payload: String,
    index: u32,
    state: State<'_, AppState>,
) -> CmdResult<SignReceipt> {
    CmdError::s(state.signer.sign(payload.into_bytes(), index))
}

// ----------------------------------------------------------------- proxy

/// Start the local intercept proxy on a loopback port.
#[tauri::command]
pub async fn proxy_start(
    port: Option<u16>,
    state: State<'_, AppState>,
) -> CmdResult<ProxySnapshot> {
    let existing = state.proxy.lock();
    if let Some(running) = existing.as_ref() {
        return Ok(running.snapshot());
    }
    drop(existing);

    let cfg = ProxyConfig {
        port,
        ..ProxyConfig::default()
    };
    let server = CmdError::s(brera_core::proxy::spawn_proxy(cfg).await)?;
    let snapshot = server.snapshot();
    *state.proxy.lock() = Some(server);
    Ok(snapshot)
}

#[tauri::command]
pub async fn proxy_stop(state: State<'_, AppState>) -> CmdResult<Option<ProxySnapshot>> {
    let taken = state.proxy.lock().take();
    match taken {
        Some(server) => {
            let snap = server.snapshot();
            server.stop().await;
            Ok(Some(snap))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub fn proxy_status(state: State<'_, AppState>) -> Option<ProxySnapshot> {
    state.proxy.lock().as_ref().map(ProxyServer::snapshot)
}

// ------------------------------------------------------------ protection

/// Arm or disarm the OS-level capture shield (screenshot/screen-share block).
#[tauri::command]
pub fn protection_set(
    window: tauri::WebviewWindow,
    enabled: bool,
    state: State<'_, AppState>,
) -> CmdResult<bool> {
    crate::protection::apply(&window, enabled).map_err(|e| CmdError {
        kind: "protection".into(),
        message: e,
    })?;
    state
        .protection
        .store(enabled, std::sync::atomic::Ordering::Relaxed);
    Ok(enabled)
}

#[tauri::command]
pub fn protection_status(state: State<'_, AppState>) -> bool {
    state.protection.load(std::sync::atomic::Ordering::Relaxed)
}

// -------------------------------------------------------------- memguard

/// Reactive memory purge: resets proxy telemetry buffers and reports what
/// was wiped. Does NOT lock the vault — use vault_lock for that.
#[tauri::command]
pub fn memory_purge(state: State<'_, AppState>) -> PurgeReport {
    let started = std::time::Instant::now();
    let mut report = PurgeReport::empty();
    if let Some(server) = state.proxy.lock().as_ref() {
        let wiped = server.purge_scratch();
        report.buffers_wiped += wiped;
    }
    report.duration_micros = started.elapsed().as_micros();
    report
}
