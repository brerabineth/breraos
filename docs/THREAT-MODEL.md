# BreraOS — threat model (v0.1.0)

Scope: the desktop shell, the core crate, and the local proxy.
Out of scope: the host OS being compromised, side-channel attacks on the
CPU, and anything below the Windows display stack.

| threat | control | status |
|---|---|---|
| seed phrase visible on screen after setup | phrase rendered once at creation, UI forces a write-down step, then the phrase is never serialized to the DOM again | done |
| screenshot / screen-share harvesting of the window | `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` applied at window creation; disarmable from the titlebar (anchor flips red) | done (Windows) |
| seed extraction from process memory | seed held in `Zeroizing<[u8;64]>` on a dedicated signer thread; lock/drop scrubs; no IPC surface returns key material | done |
| wallet-state scraping from the webview DOM | DOM holds fingerprints, addresses and signature receipts only — nothing derivable back to a key | done |
| clipboard swap of destination address (clippers) | signing happens in core on payload bytes; receipts bind signature to payload digest and address | partial — payload composition UX is manual in this release |
| passive eavesdrop on proxied traffic | all proxied payloads sealed as XChaCha20-Poly1305 frames (per-frame nonce, seq AAD); tamper fails authentication | done for frame channel; TLS still terminates at origin |
| malicious node injection via canvas | canvas nodes are UI-local; no node can invoke privileged commands beyond the fixed IPC allow-list | done |
| memory bloat of stale viewport/telemetry data | reactive purge command wipes proxy telemetry and reports buffers wiped; node memory is volatile by construction | done |
| proxy abuse as an open relay | listener binds loopback only; non-loopback bind refused by construction and tested | done |
| vault-at-rest theft (disk image) | seed is memory-only; nothing hits disk in v0.1.0 | accepted risk until Argon2id + AEAD lands |

Known limitations, on purpose:

- macOS/Linux get the UI but the capture shield is a Windows primitive
  today; the call is a documented no-op there.
- The p2p transport is a stub that fails closed. No frame leaves the
  machine through an unimplemented path — ever.
- The browser design preview runs a simulated bridge. It is labeled as
  such in the sidebar. It cannot create real keys.
