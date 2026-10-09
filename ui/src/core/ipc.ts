/**
 * IPC bridge. Two implementations behind one interface:
 *  - TauriBridge: real core, real keys, real proxy (inside the desktop shell)
 *  - LocalBridge: simulated core for browser design-preview only. Clearly
 *    flagged in the UI; never produces real key material.
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { asIpcError, type IpcError } from "./types";
import type {
  ProxySnapshot,
  PurgeReport,
  SignReceipt,
  SystemSnapshot,
  VaultReceipt,
  VaultStatus,
} from "./types";

export interface Bridge {
  readonly mode: "core" | "simulated";
  snapshot(): Promise<SystemSnapshot>;
  vaultGenerate(words: number): Promise<VaultReceipt>;
  vaultUnlock(phrase: string): Promise<VaultStatus>;
  vaultLock(): Promise<PurgeReport>;
  vaultStatus(): Promise<VaultStatus>;
  walletAddress(index: number): Promise<string>;
  walletSign(payload: string, index: number): Promise<SignReceipt>;
  proxyStart(port?: number): Promise<ProxySnapshot>;
  proxyStop(): Promise<ProxySnapshot | null>;
  proxyStatus(): Promise<ProxySnapshot | null>;
  protectionSet(enabled: boolean): Promise<boolean>;
  memoryPurge(): Promise<PurgeReport>;
}

function inTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ !== undefined
  );
}

class TauriBridge implements Bridge {
  readonly mode = "core" as const;

  private async call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await tauriInvoke<T>(cmd, args ?? {});
    } catch (err) {
      throw asIpcError(err);
    }
  }

  snapshot(): Promise<SystemSnapshot> {
    return this.call<SystemSnapshot>("system_snapshot");
  }
  vaultGenerate(words: number): Promise<VaultReceipt> {
    return this.call<VaultReceipt>("vault_generate", { words });
  }
  vaultUnlock(phrase: string): Promise<VaultStatus> {
    return this.call<VaultStatus>("vault_unlock", { phrase });
  }
  vaultLock(): Promise<PurgeReport> {
    return this.call<PurgeReport>("vault_lock");
  }
  vaultStatus(): Promise<VaultStatus> {
    return this.call<VaultStatus>("vault_status");
  }
  walletAddress(index: number): Promise<string> {
    return this.call<string>("wallet_address", { index });
  }
  walletSign(payload: string, index: number): Promise<SignReceipt> {
    return this.call<SignReceipt>("wallet_sign", { payload, index });
  }
  proxyStart(port?: number): Promise<ProxySnapshot> {
    return this.call<ProxySnapshot>("proxy_start", { port: port ?? null });
  }
  async proxyStop(): Promise<ProxySnapshot | null> {
    return this.call<ProxySnapshot | null>("proxy_stop");
  }
  async proxyStatus(): Promise<ProxySnapshot | null> {
    return this.call<ProxySnapshot | null>("proxy_status");
  }
  protectionSet(enabled: boolean): Promise<boolean> {
    return this.call<boolean>("protection_set", { enabled });
  }
  memoryPurge(): Promise<PurgeReport> {
    return this.call<PurgeReport>("memory_purge");
  }
}

/* ------------------------------------------------------------------ local */

const DEMO_WORDS = [
  "harbor", "signal", "cobalt", "ember", "lattice", "onyx",
  "meadow", "quartz", "vector", "willow", "zephyr", "anchor",
] as const;

class LocalBridge implements Bridge {
  readonly mode = "simulated" as const;
  private vault: VaultStatus = {
    unlocked: false,
    fingerprint: null,
    derivation: "m/44'/60'/0'/0/*",
    word_count: null,
  };
  private pendingReceipt: VaultReceipt | null = null;
  private proxy: ProxySnapshot | null = null;
  private proxyTimer: ReturnType<typeof setInterval> | null = null;
  private shield = true;
  private tick = 0;

  async snapshot(): Promise<SystemSnapshot> {
    return {
      core_version: "0.1.0",
      shell_version: "0.1.0",
      uptime_secs: Math.floor(performance.now() / 1000),
      capture_shield: this.shield,
      vault: { ...this.vault },
      proxy: this.proxy ? { ...this.proxy } : null,
    };
  }

  async vaultGenerate(words: number): Promise<VaultReceipt> {
    const count = words >= 24 ? 12 : 6;
    const picked: string[] = [];
    while (picked.length < count) {
      const w = DEMO_WORDS[Math.floor(Math.random() * DEMO_WORDS.length)];
      if (w && !picked.includes(w)) picked.push(w);
    }
    while (picked.length < count) {
      picked.push(`${DEMO_WORDS[picked.length % DEMO_WORDS.length]}${picked.length}`);
    }
    const fingerprint = this.fakeHex(16);
    this.pendingReceipt = {
      mnemonic: picked.join(" "),
      fingerprint,
      word_count: words,
      derivation: "m/44'/60'/0'/0/*",
    };
    return { ...this.pendingReceipt };
  }

  async vaultUnlock(phrase: string): Promise<VaultStatus> {
    const target = this.pendingReceipt?.mnemonic ?? "";
    if (phrase.trim() !== target) {
      const err: IpcError = { kind: "mnemonic", message: "checksum failed: phrase rejected" };
      throw err;
    }
    this.vault = {
      unlocked: true,
      fingerprint: this.pendingReceipt?.fingerprint ?? this.fakeHex(16),
      derivation: "m/44'/60'/0'/0/*",
      word_count: this.pendingReceipt?.word_count ?? 12,
    };
    return { ...this.vault };
  }

  async vaultLock(): Promise<PurgeReport> {
    this.vault = {
      unlocked: false,
      fingerprint: null,
      derivation: "m/44'/60'/0'/0/*",
      word_count: null,
    };
    this.pendingReceipt = null;
    return {
      buffers_wiped: 1,
      frames_dropped: 0,
      vault_locked: true,
      duration_micros: 400 + Math.floor(Math.random() * 300),
    };
  }

  async vaultStatus(): Promise<VaultStatus> {
    return { ...this.vault };
  }

  async walletAddress(index: number): Promise<string> {
    if (!this.vault.unlocked) {
      throw { kind: "vault_locked", message: "vault is locked" } as IpcError;
    }
    // deterministic pseudo-address from fingerprint + index (demo only)
    const seedText = `${this.vault.fingerprint}:${index}`;
    let h = 0x811c9dc5;
    for (let i = 0; i < seedText.length; i++) {
      h ^= seedText.charCodeAt(i);
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    let addr = "0x";
    for (let i = 0; i < 5; i++) {
      h = Math.imul(h ^ (h >>> 13), 0x5bd1e995) >>> 0;
      addr += h.toString(16).padStart(8, "0").slice(0, 8);
    }
    return addr.slice(0, 42);
  }

  async walletSign(payload: string, index: number): Promise<SignReceipt> {
    if (!this.vault.unlocked) {
      throw { kind: "vault_locked", message: "vault is locked" } as IpcError;
    }
    const digest = await this.sha256(payload);
    return {
      signature: (await this.sha256(`${digest}:${index}`)) + (await this.sha256(`${index}:${digest}`)),
      address: await this.walletAddress(index),
      payload_digest: digest,
      index,
    };
  }

  async proxyStart(port?: number): Promise<ProxySnapshot> {
    if (this.proxy) return { ...this.proxy };
    this.tick = 0;
    this.proxy = {
      running: true,
      addr: `127.0.0.1:${(port ?? 0) || 7341}`,
      mode: "loopback-direct",
      requests: 0,
      tunnels: 0,
      errors: 0,
      bytes_out: 0,
      bytes_in: 0,
    };
    this.proxyTimer = setInterval(() => {
      if (!this.proxy) return;
      this.tick++;
      this.proxy.requests = 12 + this.tick * 3;
      this.proxy.tunnels = 2 + Math.floor(this.tick / 4);
      this.proxy.bytes_out = 48_000 + this.tick * 7_340;
      this.proxy.bytes_in = 212_000 + this.tick * 31_200;
    }, 1200);
    return { ...this.proxy };
  }

  async proxyStop(): Promise<ProxySnapshot | null> {
    if (this.proxyTimer) clearInterval(this.proxyTimer);
    const p = this.proxy;
    this.proxy = null;
    this.proxyTimer = null;
    return p ? { ...p, running: false } : null;
  }

  async proxyStatus(): Promise<ProxySnapshot | null> {
    return this.proxy ? { ...this.proxy } : null;
  }

  async protectionSet(enabled: boolean): Promise<boolean> {
    this.shield = enabled;
    return this.shield;
  }

  async memoryPurge(): Promise<PurgeReport> {
    return {
      buffers_wiped: 5,
      frames_dropped: this.tick,
      vault_locked: false,
      duration_micros: 1_240 + Math.floor(Math.random() * 900),
    };
  }

  private fakeHex(len: number): string {
    let out = "";
    for (let i = 0; i < len; i++) {
      out += "0123456789abcdef"[Math.floor(Math.random() * 16)];
    }
    return out;
  }

  private async sha256(text: string): Promise<string> {
    const data = new TextEncoder().encode(text);
    const digest = await crypto.subtle.digest("SHA-256", data);
    return [...new Uint8Array(digest)]
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
  }
}

export const bridge: Bridge = inTauri() ? new TauriBridge() : new LocalBridge();
