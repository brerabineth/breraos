/** Node body builders: one per kind. All render from live core state. */

import { bridge } from "../core/ipc";
import { bus } from "../core/bus";
import type { SignReceipt, SystemSnapshot } from "../core/types";

type El = HTMLElement;

function el(tag: string, cls: string, text?: string): El {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function row(k: string, v: string, vColor = "text-slate-200"): El {
  const r = el("div", "flex items-center justify-between text-[10.5px]");
  const kk = el("span", "text-slate-500 tracking-[0.12em]", k.toUpperCase());
  const vv = el("span", `tracking-wide ${vColor}`, v);
  r.append(kk, vv);
  return r;
}

/* --------------------------------------------------------------- webview */

export function buildWebviewNode(): El {
  const root = el("div", "space-y-2.5");
  const bar = el("div", "flex items-center gap-2 chip px-2.5 py-1.5");
  const lock = el("span", "text-teal-300 text-[10px]", "◍");
  const url = el("span", "text-[10.5px] text-slate-300 tracking-wide", "exchange.example/feed");
  const via = el("span", "ml-auto text-[8.5px] tracking-[0.18em] text-cyan-300/80", "VIA PROXY");
  bar.append(lock, url, via);

  const frame = el("div", "relative rounded-xl overflow-hidden border hairline h-24");
  frame.style.background =
    "linear-gradient(150deg, rgba(34,211,238,0.06), rgba(139,92,246,0.05) 55%, rgba(0,0,0,0.2))";
  const grid = el("div", "absolute inset-0 opacity-60");
  grid.style.backgroundImage =
    "linear-gradient(rgba(148,163,184,0.07) 1px, transparent 1px), linear-gradient(90deg, rgba(148,163,184,0.07) 1px, transparent 1px)";
  grid.style.backgroundSize = "18px 18px";
  const tagEl = el(
    "div",
    "absolute inset-x-0 bottom-0 px-2.5 py-1 text-[8.5px] tracking-[0.16em] text-slate-400 flex justify-between",
  );
  const frameState = el("span", "text-cyan-300/90", "FRAME ISOLATED");
  const encState = el("span", "text-teal-300/90", "CHACHA20-P1305");
  tagEl.append(frameState, encState);
  frame.append(grid, tagEl);

  const footer = row("egress", "encrypted channel", "text-teal-300");
  root.append(bar, frame, footer);
  return root;
}

/* ---------------------------------------------------------------- wallet */

export function buildWalletNode(snapshot: SystemSnapshot): El {
  const root = el("div", "space-y-2.5");
  const status = el("div", "space-y-1.5");
  status.append(row("vault", snapshot.vault.unlocked ? "UNLOCKED" : "LOCKED", snapshot.vault.unlocked ? "text-violet-300" : "text-slate-400"));
  if (snapshot.vault.fingerprint) {
    status.append(row("fingerprint", snapshot.vault.fingerprint.slice(0, 16)));
  }
  status.append(row("derivation", snapshot.vault.derivation, "text-slate-400"));

  const actions = el("div", "flex gap-2");
  const mkAction = (label: string, tone: string): HTMLButtonElement => {
    const b = el("button", `flex-1 text-[9.5px] tracking-[0.14em] chip py-1.5 ${tone} hover:brightness-125 transition`) as HTMLButtonElement;
    b.textContent = label;
    return b;
  };
  const gen = mkAction("CREATE", "text-violet-300");
  const unlock = mkAction("UNLOCK", "text-cyan-300");
  const lock = mkAction("LOCK", "text-slate-400");
  actions.append(gen, unlock, lock);

  const secret = el("div", "hidden chip p-2.5 space-y-2");
  const phrase = el("div", "text-[10px] leading-relaxed text-amber-200/90 break-words", "");
  const warn = el("div", "text-[8.5px] tracking-[0.14em] text-amber-400/70", "WRITE IT DOWN. SHOWN ONCE. NEVER AGAIN.");
  const confirm = el("button", "w-full text-[9.5px] tracking-[0.14em] chip py-1.5 text-teal-300") as HTMLButtonElement;
  confirm.textContent = "I WROTE IT DOWN";
  secret.append(phrase, warn, confirm);

  const addr = el("div", "hidden space-y-1.5");
  const addrValue = el("div", "text-[10px] leading-relaxed text-slate-200 tracking-wide break-all", "");
  const addrRow = el("div", "space-y-1");
  addrRow.append(el("div", "text-slate-500 tracking-[0.12em] text-[10.5px]", "ADDRESS 0"));
  addrRow.append(addrValue);
  const signBtn = el("button", "w-full text-[9.5px] tracking-[0.14em] chip py-1.5 text-violet-300") as HTMLButtonElement;
  signBtn.textContent = "SIGN PAYLOAD";
  const signed = el("div", "hidden chip p-2.5 space-y-1 text-[9px]");
  addr.append(addrRow, signBtn, signed);

  const setError = (msg: string): void => {
    bus.emit("toast", { text: msg, tone: "err" });
  };

  gen.addEventListener("click", async () => {
    try {
      const receipt = await bridge.vaultGenerate(12);
      phrase.textContent = receipt.mnemonic;
      secret.classList.remove("hidden");
      actions.classList.add("hidden");
      await refresh();
    } catch (err) {
      setError(`create failed: ${JSON.stringify(err)}`);
    }
  });

  confirm.addEventListener("click", async () => {
    try {
      await bridge.vaultUnlock(phrase.textContent ?? "");
      secret.classList.add("hidden");
      actions.classList.add("hidden");
      await showAddress();
      await refresh();
      bus.emit("toast", { text: "vault unlocked — seed lives in Rust memory only", tone: "ok" });
    } catch (err) {
      setError(`unlock failed: ${JSON.stringify(err)}`);
    }
  });

  unlock.addEventListener("click", async () => {
    if (snapshot.vault.unlocked) {
      await showAddress();
      return;
    }
    // same flow as create: generate → confirm words → unlocked
    try {
      const receipt = await bridge.vaultGenerate(12);
      phrase.textContent = receipt.mnemonic;
      secret.classList.remove("hidden");
      actions.classList.add("hidden");
    } catch (err) {
      setError(`unlock prep failed: ${JSON.stringify(err)}`);
    }
  });

  lock.addEventListener("click", async () => {
    try {
      await bridge.vaultLock();
      addr.classList.add("hidden");
      signed.classList.add("hidden");
      actions.classList.remove("hidden");
      await refresh();
      bus.emit("toast", { text: "vault locked — seed scrubbed from RAM", tone: "ok" });
    } catch (err) {
      setError(`lock failed: ${JSON.stringify(err)}`);
    }
  });

  async function showAddress(): Promise<void> {
    try {
      const address = await bridge.walletAddress(0);
      addrValue.textContent = address;
      addr.classList.remove("hidden");
    } catch (err) {
      setError(`address failed: ${JSON.stringify(err)}`);
    }
  }

  signBtn.addEventListener("click", async () => {
    try {
      const payload = `breraos://sign?node=wallet&ts=${Date.now()}`;
      const receipt: SignReceipt = await bridge.walletSign(payload, 0);
      signed.replaceChildren(
        el("div", "text-[8.5px] tracking-[0.14em] text-slate-500", "SIGNED IN RUST · SEED NEVER LEFT CORE"),
        row("sig", `${receipt.signature.slice(0, 28)}…`, "text-violet-300"),
        row("digest", `${receipt.payload_digest.slice(0, 20)}…`, "text-slate-400"),
      );
      signed.classList.remove("hidden");
      bus.emit("toast", { text: "payload signed via isolated signer thread", tone: "ok" });
    } catch (err) {
      setError(`sign failed: ${JSON.stringify(err)}`);
    }
  });

  async function refresh(): Promise<void> {
    const snap = await bridge.snapshot();
    snapshot.vault = snap.vault;
    status.replaceChildren(
      row("vault", snap.vault.unlocked ? "UNLOCKED" : "LOCKED", snap.vault.unlocked ? "text-violet-300" : "text-slate-400"),
      ...(snap.vault.fingerprint ? [row("fingerprint", snap.vault.fingerprint.slice(0, 16))] : []),
      row("derivation", snap.vault.derivation, "text-slate-400"),
    );
  }

  root.append(status, actions, secret, addr, signed);
  return root;
}

/* ----------------------------------------------------------------- proxy */

export function buildProxyNode(snapshot: SystemSnapshot): El {
  const root = el("div", "space-y-2.5");
  const head = el("div", "space-y-1.5");
  const stats = el("div", "space-y-1.5");
  const actions = el("div", "flex gap-2");
  const start = el("button", "flex-1 text-[9.5px] tracking-[0.14em] chip py-1.5 text-teal-300") as HTMLButtonElement;
  start.textContent = "START";
  const stop = el("button", "flex-1 text-[9.5px] tracking-[0.14em] chip py-1.5 text-slate-400") as HTMLButtonElement;
  stop.textContent = "STOP";
  actions.append(start, stop);

  function paint(p: NonNullable<SystemSnapshot["proxy"]> | null): void {
    head.replaceChildren(
      row("listener", p ? p.addr : "—", p ? "text-teal-300" : "text-slate-500"),
      row("transport", p ? p.mode : "offline", p ? "text-cyan-300" : "text-slate-500"),
    );
    stats.replaceChildren(
      row("requests", String(p?.requests ?? 0)),
      row("tunnels", String(p?.tunnels ?? 0)),
      row("errors", String(p?.errors ?? 0), (p?.errors ?? 0) > 0 ? "text-rose-300" : "text-slate-200"),
      row("egress", fmtBytes(p?.bytes_out ?? 0), "text-slate-300"),
      row("ingress", fmtBytes(p?.bytes_in ?? 0), "text-slate-300"),
    );
  }

  start.addEventListener("click", async () => {
    try {
      const p = await bridge.proxyStart();
      paint(p);
      bus.emit("toast", { text: `proxy listening on ${p.addr}`, tone: "ok" });
    } catch (err) {
      bus.emit("toast", { text: `proxy start failed: ${JSON.stringify(err)}`, tone: "err" });
    }
  });
  stop.addEventListener("click", async () => {
    await bridge.proxyStop();
    paint(null);
    bus.emit("toast", { text: "proxy stopped", tone: "warn" });
  });

  paint(snapshot.proxy);
  root.append(head, stats, actions);

  // live counters: the node keeps itself honest while the proxy runs
  const poll = setInterval(() => {
    void (async () => {
      try {
        const p = await bridge.proxyStatus();
        paint(p);
      } catch {
        /* node outlives nothing; next tick retries */
      }
    })();
  }, 1500);
  // stop polling when the node leaves the DOM
  const mo = new MutationObserver(() => {
    if (!root.isConnected) {
      clearInterval(poll);
      mo.disconnect();
    }
  });
  mo.observe(document.body, { childList: true, subtree: true });

  return root;
}

/* ------------------------------------------------------------------ note */

export function buildNoteNode(): El {
  const root = el("div", "space-y-2");
  const area = el("textarea", "w-full h-20 bg-transparent text-[10.5px] leading-relaxed text-slate-300 resize-none") as HTMLTextAreaElement;
  area.value = "op notes\n— rotate seed quarterly\n— verify fingerprints over signal";
  area.spellcheck = false;
  const hint = el("div", "text-[8.5px] tracking-[0.16em] text-slate-500", "NODE MEMORY IS VOLATILE · PURGED WITH THE SESSION");
  root.append(area, hint);
  return root;
}

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}
