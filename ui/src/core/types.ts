/** Shared view of the core's serializable types (mirror of brera-core). */

export interface VaultStatus {
  unlocked: boolean;
  fingerprint: string | null;
  derivation: string;
  word_count: number | null;
}

export interface VaultReceipt {
  mnemonic: string;
  fingerprint: string;
  word_count: number;
  derivation: string;
}

export interface SignReceipt {
  signature: string;
  address: string;
  payload_digest: string;
  index: number;
}

export interface ProxySnapshot {
  running: boolean;
  addr: string;
  mode: string;
  requests: number;
  tunnels: number;
  errors: number;
  bytes_out: number;
  bytes_in: number;
}

export interface PurgeReport {
  buffers_wiped: number;
  frames_dropped: number;
  vault_locked: boolean;
  duration_micros: number;
}

export interface SystemSnapshot {
  core_version: string;
  shell_version: string;
  uptime_secs: number;
  capture_shield: boolean;
  vault: VaultStatus;
  proxy: ProxySnapshot | null;
}

export type AnchorState = "secure" | "warn" | "alert" | "locked";

export interface IpcError {
  kind: string;
  message: string;
}

export function asIpcError(err: unknown): IpcError {
  if (typeof err === "object" && err !== null) {
    const e = err as Record<string, unknown>;
    if (typeof e.kind === "string" && typeof e.message === "string") {
      return { kind: e.kind, message: e.message };
    }
    if (e.message !== undefined) {
      return { kind: "internal", message: String(e.message) };
    }
  }
  return { kind: "internal", message: String(err) };
}
