/** Minimal typed event bus. No deps, no surprises. */

import type { SystemSnapshot } from "./types";

export interface BusEvents {
  "snapshot:changed": SystemSnapshot;
  "anchor:set": void;
  "node:selected": string | null;
  "toast": { text: string; tone: "ok" | "warn" | "err" };
}

type Handler<T> = (payload: T) => void;

export class Bus {
  private handlers = new Map<keyof BusEvents, Set<Handler<never>>>();

  on<K extends keyof BusEvents>(key: K, fn: Handler<BusEvents[K]>): () => void {
    let set = this.handlers.get(key);
    if (!set) {
      set = new Set();
      this.handlers.set(key, set);
    }
    set.add(fn as Handler<never>);
    return () => {
      set?.delete(fn as Handler<never>);
    };
  }

  emit<K extends keyof BusEvents>(key: K, payload: BusEvents[K]): void {
    const set = this.handlers.get(key);
    if (!set) return;
    for (const fn of set) {
      try {
        (fn as unknown as Handler<BusEvents[K]>)(payload);
      } catch (err) {
        console.error(`[bus] handler for ${String(key)} failed`, err);
      }
    }
  }
}

export const bus = new Bus();
