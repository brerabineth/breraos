/** BreraOS entry: chrome, sidebar anchor, spatial canvas, live core wiring. */

import "./styles.css";
import { bus } from "./core/bus";
import { bridge } from "./core/ipc";
import type { SystemSnapshot } from "./core/types";
import { Viewport } from "./canvas/viewport";
import { Graph, kindColor, type NodeKind } from "./canvas/graph";
import { Anchor } from "./anchor/orb";
import { buildTitlebar, bindShield } from "./shell/titlebar";
import { buildWebviewNode, buildWalletNode, buildProxyNode, buildNoteNode } from "./nodes/factories";

/* ------------------------------------------------------------------ boot */

async function boot(): Promise<void> {
  const app = document.getElementById("app");
  if (!app) throw new Error("#app missing");

  const overlay = buildBootOverlay();
  document.body.appendChild(overlay);

  const titlebar = await buildTitlebar();
  app.appendChild(titlebar);

  const main = document.createElement("div");
  main.className = "flex-1 flex min-h-0";
  app.appendChild(main);

  const sidebar = buildSidebar();
  const canvasWrap = buildCanvas();
  main.append(sidebar.root, canvasWrap.root);
  sidebarRef = sidebar;

  bindShield((armed) => {
    if (latest && !armed) sidebar.paintStatus({ ...latest, capture_shield: false });
    else void pollOnce();
  });

  wireToasts();
  setTimeout(() => pollOnce(), 60);
  setInterval(() => void pollOnce(), 2000);

  // seed the canvas: a working layout, edges wired, everything real
  const g = canvasWrap.graph;
  const snap = await bridge.snapshot();

  const web = g.addNode("webview", "web · feed", { x: 90, y: 120 }, buildWebviewNode);
  const proxy = g.addNode("proxy", "proxy · shield", { x: 480, y: 340 }, () => buildProxyNode(snap));
  const wallet = g.addNode("wallet", "cold wallet", { x: 120, y: 430 }, () => buildWalletNode(snap));
  const notes = g.addNode("note", "op notes", { x: 520, y: 90 }, buildNoteNode);
  void wallet;

  g.connect(web.id, proxy.id);
  g.connect(proxy.id, notes.id);
  g.redrawAll();

  // add-node toolbar
  canvasWrap.bindAdd((kind) => {
    const c = canvasWrap.viewport.screenToWorld({
      x: window.innerWidth / 2,
      y: window.innerHeight / 2,
    });
    const jitter = (Math.random() - 0.5) * 140;
    const n = g.addNode(kind, kindTitle(kind), { x: c.x + jitter, y: c.y + jitter }, () => {
      if (kind === "webview") return buildWebviewNode();
      if (kind === "proxy") return buildProxyNode(snap);
      if (kind === "wallet") return buildWalletNode(snap);
      return buildNoteNode();
    });
    void n;
  });
  canvasWrap.bindViewportHelpers(() => bridge.snapshot());

  window.addEventListener("resize", () => g.redrawAll());
  setTimeout(() => g.redrawAll(), 80);
  setTimeout(() => {
    overlay.classList.add("opacity-0");
    overlay.style.transition = "opacity 0.5s ease";
    setTimeout(() => overlay.remove(), 550);
  }, 1150);
}

/* -------------------------------------------------------------- sidebar */

interface Sidebar {
  root: HTMLElement;
  paintStatus: (snap: SystemSnapshot) => void;
}

let sidebarRef: Sidebar | null = null;

function buildSidebar(): Sidebar {
  const root = document.createElement("aside");
  root.className =
    "w-[248px] shrink-0 flex flex-col glass-strong border-0 border-r hairline z-30";

  // anchor block
  const anchorWrap = document.createElement("div");
  anchorWrap.className = "flex flex-col items-center pt-5 pb-4 px-4";
  const canvas = document.createElement("canvas");
  canvas.className = "orb-canvas orb-breathe";
  const label = document.createElement("div");
  label.className = "text-[13px] tracking-[0.3em] font-semibold mt-1";
  label.style.color = "#94a3b8";
  label.textContent = "LOCKED";
  const sub = document.createElement("div");
  sub.className = "text-[8.5px] tracking-[0.2em] text-slate-500 mt-1 text-center";
  sub.textContent = "QUANTUM-GLOW SECURITY ANCHOR";
  anchorWrap.append(canvas, label, sub);

  const anchor = new Anchor(canvas, label, sub);

  const divider = document.createElement("div");
  divider.className = "h-px hairline border-t mx-4";

  // status rows
  const statusWrap = document.createElement("div");
  statusWrap.className = "px-4 py-4 space-y-3";

  const mkRow = (key: string): { row: HTMLElement; value: HTMLElement; dot: HTMLElement } => {
    const row = document.createElement("div");
    row.className = "flex items-center justify-between text-[10px]";
    const kk = document.createElement("span");
    kk.className = "text-slate-500 tracking-[0.16em]";
    kk.textContent = key;
    const right = document.createElement("span");
    right.className = "flex items-center gap-2";
    const dot = document.createElement("span");
    dot.className = "status-dot";
    dot.style.background = "#475569";
    const vv = document.createElement("span");
    vv.className = "text-slate-300 tracking-wide";
    vv.textContent = "—";
    right.append(dot, vv);
    row.append(kk, right);
    statusWrap.appendChild(row);
    return { row, value: vv, dot };
  };

  const vaultRow = mkRow("VAULT");
  const proxyRow = mkRow("PROXY");
  const shieldRow = mkRow("CAPTURE SHIELD");
  const memRow = mkRow("MEMORY");

  const purge = document.createElement("button");
  purge.className =
    "w-full mt-1 text-[9.5px] tracking-[0.16em] chip py-2 text-slate-300 hover:text-cyan-200 hover:brightness-125 transition";
  purge.textContent = "PURGE VOLATILE MEMORY";
  purge.dataset.ui = "purge";
  purge.addEventListener("click", async () => {
    const report = await bridge.memoryPurge();
    bus.emit("toast", {
      text: `purged ${report.buffers_wiped} buffer groups · ${report.duration_micros}µs`,
      tone: "ok",
    });
    memRow.value.textContent = `${report.buffers_wiped} wiped`;
    memRow.dot.style.background = "#2dd4bf";
    memRow.dot.style.boxShadow = "0 0 8px rgba(45,212,191,0.8)";
    setTimeout(() => {
      memRow.value.textContent = "clean";
    }, 1600);
  });

  const spacer = document.createElement("div");
  spacer.className = "flex-1";

  const footer = document.createElement("div");
  footer.className = "px-4 py-3 border-t hairline space-y-1.5";
  const mode = document.createElement("div");
  mode.className = "text-[8.5px] tracking-[0.18em]";
  mode.textContent =
    bridge.mode === "core" ? "CORE: RUST · CONNECTED" : "UI PREVIEW · SIMULATED BRIDGE";
  mode.style.color = bridge.mode === "core" ? "#2dd4bf" : "#fbbf24";
  const ver = document.createElement("div");
  ver.className = "text-[8.5px] tracking-[0.18em] text-slate-500";
  ver.textContent = "BRERAOS v0.1.0 · RUST CORE";
  footer.append(mode, ver);

  root.append(anchorWrap, divider, statusWrap, purge, spacer, footer);

  function paint(snap: SystemSnapshot): void {
    // vault
    if (snap.vault.unlocked) {
      vaultRow.value.textContent = `0x${snap.vault.fingerprint?.slice(0, 8) ?? "00000000"}`;
      vaultRow.dot.style.background = "#8b5cf6";
      vaultRow.dot.style.boxShadow = "0 0 8px rgba(139,92,246,0.8)";
    } else {
      vaultRow.value.textContent = "locked";
      vaultRow.dot.style.background = "#475569";
      vaultRow.dot.style.boxShadow = "none";
    }
    // proxy
    if (snap.proxy?.running) {
      proxyRow.value.textContent = snap.proxy.addr;
      proxyRow.dot.style.background = "#2dd4bf";
      proxyRow.dot.style.boxShadow = "0 0 8px rgba(45,212,191,0.8)";
    } else {
      proxyRow.value.textContent = "offline";
      proxyRow.dot.style.background = "#475569";
      proxyRow.dot.style.boxShadow = "none";
    }
    // shield
    if (snap.capture_shield) {
      shieldRow.value.textContent = "armed";
      shieldRow.dot.style.background = "#22d3ee";
      shieldRow.dot.style.boxShadow = "0 0 8px rgba(34,211,238,0.8)";
    } else {
      shieldRow.value.textContent = "OFF";
      shieldRow.dot.style.background = "#f87171";
      shieldRow.dot.style.boxShadow = "0 0 8px rgba(248,113,113,0.9)";
    }
    memRow.value.textContent = "clean";

    // anchor state follows the composite picture
    if (!snap.capture_shield) anchor.setState("alert");
    else if (!snap.vault.unlocked) anchor.setState("locked");
    else if (!snap.proxy?.running) anchor.setState("warn");
    else anchor.setState("secure");
  }

  return { root, paintStatus: paint };
}

/* ---------------------------------------------------------------- canvas */

interface CanvasWrap {
  root: HTMLElement;
  viewport: Viewport;
  graph: Graph;
  bindAdd: (cb: (kind: NodeKind) => void) => void;
  bindViewportHelpers: (snapshot: () => Promise<SystemSnapshot>) => void;
}

function buildCanvas(): CanvasWrap {
  const root = document.createElement("section");
  root.className = "relative flex-1 min-w-0 overflow-hidden cursor-grab";

  const bg = document.createElement("div");
  bg.className = "brera-bg";
  const grid = document.createElement("div");
  grid.className = "brera-grid";

  const world = document.createElement("div");
  world.className = "absolute inset-0";
  world.style.transformOrigin = "0 0";

  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("style", "position:absolute;left:0;top:0;width:4200px;height:3200px;overflow:visible;pointer-events:none");

  world.appendChild(svg);
  root.append(bg, grid, world);

  const viewport = new Viewport(root, world);
  const graph = new Graph(root, world, svg);

  // toolbar
  const toolbar = document.createElement("div");
  toolbar.className =
    "absolute left-4 bottom-4 flex items-center gap-2 glass rounded-xl px-2.5 py-2 z-20";
  toolbar.dataset.ui = "toolbar";
  const zoomLabel = document.createElement("span");
  zoomLabel.className = "text-[9.5px] tracking-[0.14em] text-slate-400 w-10 text-center";
  zoomLabel.textContent = "100%";

  const mkTool = (text: string, title: string): HTMLButtonElement => {
    const b = document.createElement("button");
    b.textContent = text;
    b.title = title;
    b.className =
      "text-[9.5px] tracking-[0.1em] chip px-2.5 py-1.5 text-slate-300 hover:text-cyan-200 transition";
    return b;
  };
  const zoomOut = mkTool("−", "zoom out");
  const zoomIn = mkTool("+", "zoom in");
  const fit = mkTool("FIT", "fit all nodes");
  toolbar.append(zoomOut, zoomLabel, zoomIn, fit);
  root.appendChild(toolbar);

  // add-node dock
  const dock = document.createElement("div");
  dock.className =
    "absolute right-4 bottom-4 flex items-center gap-2 glass rounded-xl px-2.5 py-2 z-20";
  dock.dataset.ui = "dock";
  const addLabel = document.createElement("span");
  addLabel.className = "text-[8.5px] tracking-[0.2em] text-slate-500 mr-1";
  addLabel.textContent = "ADD NODE";
  dock.appendChild(addLabel);
  const kinds: NodeKind[] = ["webview", "wallet", "proxy", "note"];
  let addCb: ((kind: NodeKind) => void) | null = null;
  for (const k of kinds) {
    const b = document.createElement("button");
    b.className =
      "w-7 h-7 rounded-lg grid place-items-center chip hover:brightness-150 transition";
    b.title = `new ${k} node`;
    const dot = document.createElement("span");
    dot.className = "status-dot";
    dot.style.background = kindColor(k);
    dot.style.boxShadow = `0 0 8px ${kindColor(k)}`;
    b.appendChild(dot);
    b.addEventListener("click", () => addCb?.(k));
    dock.appendChild(b);
  }
  root.appendChild(dock);

  zoomOut.addEventListener("click", () => {
    viewport.zoomBy(1 / 1.15);
    zoomLabel.textContent = `${Math.round(viewport.zoom * 100)}%`;
  });
  zoomIn.addEventListener("click", () => {
    viewport.zoomBy(1.15);
    zoomLabel.textContent = `${Math.round(viewport.zoom * 100)}%`;
  });
  root.addEventListener("wheel", () => {
    zoomLabel.textContent = `${Math.round(viewport.zoom * 100)}%`;
  });

  return {
    root,
    viewport,
    graph,
    bindAdd: (cb) => {
      addCb = cb;
    },
    bindViewportHelpers: (snapshot) => {
      fit.addEventListener("click", () => {
        viewport.fitAll(graph.bounds());
        zoomLabel.textContent = `${Math.round(viewport.zoom * 100)}%`;
      });
      void snapshot;
    },
  };
}

/* ----------------------------------------------------------------- misc */

function kindTitle(kind: NodeKind): string {
  switch (kind) {
    case "webview":
      return "web · viewer";
    case "wallet":
      return "cold wallet";
    case "proxy":
      return "proxy · shield";
    case "note":
      return "op notes";
  }
}

function buildBootOverlay(): HTMLElement {
  const overlay = document.createElement("div");
  overlay.className = "fixed inset-0 z-50 grid place-items-center";
  overlay.style.background = "#05060b";
  const box = document.createElement("div");
  box.className = "w-[420px] space-y-2";
  const lines = [
    "BRERAOS KERNEL v0.1.0",
    "mounting rust core .......... ok",
    "signer thread ............... isolated",
    "capture shield .............. armed",
    "spatial canvas .............. ready",
  ];
  lines.forEach((text, i) => {
    const line = document.createElement("div");
    line.className = "boot-line text-[11px] tracking-[0.14em] text-slate-400";
    line.style.animationDelay = `${0.12 + i * 0.16}s`;
    if (i === 0) {
      line.className = "boot-line text-[13px] tracking-[0.3em] text-cyan-300 font-semibold mb-3";
    }
    line.textContent = text;
    box.appendChild(line);
  });
  overlay.appendChild(box);
  return overlay;
}

function wireToasts(): void {
  const wrap = document.createElement("div");
  wrap.className = "fixed right-5 bottom-16 z-50 space-y-2 w-[300px] pointer-events-none";
  document.body.appendChild(wrap);
  bus.on("toast", ({ text, tone }) => {
    const t = document.createElement("div");
    const color = tone === "ok" ? "text-teal-300" : tone === "warn" ? "text-amber-300" : "text-rose-300";
    t.className = `glass rounded-xl px-3.5 py-2.5 text-[10px] tracking-wide ${color}`;
    t.style.opacity = "0";
    t.style.transition = "opacity 0.25s ease";
    t.textContent = text;
    wrap.appendChild(t);
    requestAnimationFrame(() => {
      t.style.opacity = "1";
    });
    setTimeout(() => {
      t.style.opacity = "0";
      setTimeout(() => t.remove(), 300);
    }, 2600);
  });
}

let latest: SystemSnapshot | null = null;

async function pollOnce(): Promise<void> {
  try {
    const snap = await bridge.snapshot();
    latest = snap;
    sidebarRef?.paintStatus(snap);
  } catch {
    // core not ready yet; next tick retries
  }
}

boot().catch((err) => {
  console.error("breraos boot failed", err);
});
