/** Custom chrome: drag region + window controls + capture-shield toggle. */

import { bus } from "../core/bus";
import { bridge } from "../core/ipc";

interface WindowApi {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  startDragging(): Promise<void>;
}

async function windowApi(): Promise<WindowApi | null> {
  if ((window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ === undefined) {
    return null;
  }
  const mod = await import("@tauri-apps/api/window");
  const w = mod.getCurrentWindow();
  return {
    minimize: () => w.minimize(),
    toggleMaximize: () => w.toggleMaximize(),
    close: () => w.close(),
    startDragging: () => w.startDragging(),
  };
}

export async function buildTitlebar(): Promise<HTMLElement> {
  const bar = document.createElement("div");
  bar.className =
    "h-10 shrink-0 flex items-center pl-4 pr-2 gap-3 glass-strong border-0 border-b hairline z-40";
  bar.dataset.tauriDragRegion = "";
  bar.style.borderRadius = "0";

  const mark = document.createElement("div");
  mark.className = "flex items-center gap-2.5 pointer-events-none";
  const glyph = document.createElement("div");
  glyph.className = "w-5 h-5 rounded-md glow-edge grid place-items-center";
  glyph.style.background = "rgba(34,211,238,0.08)";
  glyph.innerHTML =
    '<svg width="11" height="11" viewBox="0 0 32 32" fill="none"><path d="M16 3l11 6.4v13.2L16 29 5 22.6V9.4z" stroke="#22d3ee" stroke-width="2.6"/><circle cx="16" cy="16" r="3.6" fill="#8b5cf6"/></svg>';
  const name = document.createElement("span");
  name.className = "text-[12px] tracking-[0.34em] text-slate-100 font-semibold";
  name.textContent = "BRERAOS";
  const tag = document.createElement("span");
  tag.className = "text-[9px] tracking-[0.2em] text-cyan-300/60 border hairline rounded px-1.5 py-0.5";
  tag.textContent = "v0.1.0";
  mark.append(glyph, name, tag);

  const spacer = document.createElement("div");
  spacer.className = "flex-1 h-full";

  const shield = document.createElement("button");
  shield.id = "shield-btn";
  shield.dataset.ui = "shield";
  shield.className =
    "flex items-center gap-1.5 text-[10px] tracking-[0.14em] chip px-2.5 py-1 text-teal-300 hover:text-teal-200 transition-colors";
  shield.title = "capture shield — block OS-level screenshots of this window";
  shield.innerHTML =
    '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4"><path d="M12 2l8 3.5v5.9c0 5-3.4 8.8-8 10.6-4.6-1.8-8-5.6-8-10.6V5.5z"/></svg><span id="shield-label">SHIELD</span>';

  const buttons = document.createElement("div");
  buttons.className = "flex items-center h-full";
  const mkBtn = (label: string, kind: string): HTMLButtonElement => {
    const b = document.createElement("button");
    b.textContent = label;
    b.dataset.ui = `win-${kind}`;
    b.className =
      "w-11 h-full grid place-items-center text-slate-400 hover:text-slate-100 hover:bg-white/5 text-[13px]";
    return b;
  };
  const min = mkBtn("–", "min");
  const max = mkBtn("□", "max");
  const closeB = mkBtn("×", "close");
  closeB.className =
    "w-11 h-full grid place-items-center text-slate-400 hover:text-white hover:bg-rose-500/80 text-[15px]";
  buttons.append(min, max, closeB);

  bar.append(mark, spacer, shield, buttons);

  const api = await windowApi();
  if (api) {
    min.addEventListener("click", () => void api.minimize());
    max.addEventListener("click", () => void api.toggleMaximize());
    closeB.addEventListener("click", () => void api.close());
    bar.addEventListener("pointerdown", (e) => {
      const t = e.target as HTMLElement;
      if (t.closest("button")) return;
      void api.startDragging();
    });
  } else {
    // browser preview: buttons inert, still look right
    min.addEventListener("click", () => bus.emit("toast", { text: "window controls live inside the desktop shell", tone: "warn" }));
    max.addEventListener("click", () => bus.emit("toast", { text: "window controls live inside the desktop shell", tone: "warn" }));
    closeB.addEventListener("click", () => bus.emit("toast", { text: "window controls live inside the desktop shell", tone: "warn" }));
  }

  return bar;
}

export function bindShield(onToggle: (enabled: boolean) => void): void {
  const btn = document.getElementById("shield-btn");
  const label = document.getElementById("shield-label");
  if (!btn || !label) return;
  let armed = true;
  const paint = (): void => {
    btn.classList.toggle("text-teal-300", armed);
    btn.classList.toggle("text-amber-300", !armed);
    label.textContent = armed ? "SHIELD" : "SHIELD OFF";
  };
  btn.addEventListener("click", async () => {
    armed = !armed;
    try {
      armed = await bridge.protectionSet(armed);
      onToggle(armed);
      bus.emit("toast", {
        text: armed ? "capture shield armed" : "capture shield disarmed",
        tone: armed ? "ok" : "warn",
      });
    } catch (err) {
      bus.emit("toast", { text: `shield error: ${String(err)}`, tone: "err" });
    }
    paint();
  });
  paint();
}
