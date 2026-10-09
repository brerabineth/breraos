/**
 * Spatial graph: floating glass nodes, logical pipelines (edges) between
 * output and input ports. Drag to move, drag port-to-port to connect,
 * click to raise. The model stays in memory; DOM is the only view.
 */

import type { Vec2 } from "./viewport";
import { bus } from "../core/bus";

export type NodeKind = "webview" | "wallet" | "proxy" | "note";

export interface GraphNode {
  id: string;
  kind: NodeKind;
  title: string;
  pos: Vec2;
  el: HTMLElement;
  body: HTMLElement;
}

export interface GraphEdge {
  id: string;
  from: string;
  to: string;
  el: SVGPathElement;
  flow: SVGPathElement;
}

const NODE_W = 300;

export class Graph {
  private nodes = new Map<string, GraphNode>();
  private edges = new Map<string, GraphEdge>();
  private seq = 0;

  private surface: HTMLElement;
  private world: HTMLDivElement;
  private svg: SVGSVGElement;
  private selected: string | null = null;
  private zTop = 10;
  private linking: { from: string; ghost: SVGPathElement } | null = null;

  constructor(surface: HTMLElement, world: HTMLDivElement, svg: SVGSVGElement) {
    this.surface = surface;
    this.world = world;
    this.svg = svg;
    this.bindGhostTracking();
  }

  /* ------------------------------------------------------------- queries */

  list(): GraphNode[] {
    return [...this.nodes.values()];
  }

  bounds(): { minX: number; minY: number; maxX: number; maxY: number } {
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const n of this.nodes.values()) {
      minX = Math.min(minX, n.pos.x);
      minY = Math.min(minY, n.pos.y);
      maxX = Math.max(maxX, n.pos.x + NODE_W);
      maxY = Math.max(maxY, n.pos.y + n.el.offsetHeight);
    }
    if (this.nodes.size === 0) {
      return { minX: 0, minY: 0, maxX: 1, maxY: 1 };
    }
    return { minX, minY, maxX, maxY };
  }

  centerWorld(): Vec2 {
    const b = this.bounds();
    return { x: (b.minX + b.maxX) / 2, y: (b.minY + b.maxY) / 2 };
  }

  /* -------------------------------------------------------------- create */

  addNode(kind: NodeKind, title: string, pos: Vec2, build: (body: HTMLElement) => HTMLElement): GraphNode {
    this.seq += 1;
    const id = `n${this.seq}`;
    const el = document.createElement("div");
    el.className = "node-card glass";
    el.dataset.node = id;
    el.dataset.kind = kind;
    el.style.left = "0px";
    el.style.top = "0px";
    el.style.transform = `translate(${pos.x}px, ${pos.y}px)`;

    const header = document.createElement("div");
    header.className =
      "flex items-center gap-2 px-3.5 h-10 border-b hairline cursor-grab";
    const dot = document.createElement("span");
    dot.className = "status-dot";
    dot.style.background = kindColor(kind);
    dot.style.boxShadow = `0 0 8px ${kindColor(kind)}`;
    const label = document.createElement("span");
    label.className = "text-[11px] tracking-[0.18em] text-slate-300 uppercase";
    label.textContent = title;
    const close = document.createElement("button");
    close.className =
      "ml-auto text-slate-500 hover:text-rose-300 text-[13px] leading-none px-1";
    close.textContent = "×";
    close.dataset.ui = "close";
    header.append(dot, label, close);

    const body = document.createElement("div");
    body.className = "p-3.5 space-y-2.5";

    const outPort = document.createElement("div");
    outPort.className = "port port-out";
    outPort.dataset.ui = "out";
    const inPort = document.createElement("div");
    inPort.className = "port port-in";
    inPort.dataset.ui = "in";

    el.append(inPort, header, body, outPort);
    const built = build(body);
    if (built !== body) body.replaceChildren(built);

    this.world.appendChild(el);
    const node: GraphNode = { id, kind, title, pos, el, body };
    this.nodes.set(id, node);
    this.bindNode(node, close);
    return node;
  }

  connect(fromId: string, toId: string): void {
    if (fromId === toId) return;
    for (const e of this.edges.values()) {
      if (e.from === fromId && e.to === toId) return; // no duplicates
    }
    const from = this.nodes.get(fromId);
    const to = this.nodes.get(toId);
    if (!from || !to) return;

    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("class", "edge-path");
    const flow = document.createElementNS("http://www.w3.org/2000/svg", "path");
    flow.setAttribute("class", "edge-flow");
    this.svg.append(path, flow);
    const edge: GraphEdge = { id: `e${fromId}-${toId}`, from: fromId, to: toId, el: path, flow };
    this.edges.set(edge.id, edge);
    this.redrawEdge(edge);
  }

  clear(): void {
    for (const e of this.edges.values()) {
      e.el.remove();
      e.flow.remove();
    }
    this.edges.clear();
    for (const n of this.nodes.values()) n.el.remove();
    this.nodes.clear();
  }

  redrawAll(): void {
    for (const e of this.edges.values()) this.redrawEdge(e);
  }

  /* --------------------------------------------------------------- wiring */

  private kindCenter(n: GraphNode, side: "in" | "out"): Vec2 {
    const rect = n.el.getBoundingClientRect();
    const world = n.el.parentElement as HTMLElement;
    const wrect = world.getBoundingClientRect();
    const x = side === "in" ? rect.left - wrect.left : rect.right - wrect.left;
    return { x: x / this.worldScale(), y: (rect.top - wrect.top + rect.height / 2) / this.worldScale() };
  }

  private worldScale(): number {
    return Number(this.world.dataset.zoom ?? "1");
  }

  private redrawEdge(e: GraphEdge): void {
    const a = this.nodes.get(e.from);
    const b = this.nodes.get(e.to);
    if (!a || !b) return;
    const p1 = this.kindCenter(a, "out");
    const p2 = this.kindCenter(b, "in");
    const dx = Math.max(40, Math.abs(p2.x - p1.x) * 0.55);
    const d = `M ${p1.x} ${p1.y} C ${p1.x + dx} ${p1.y}, ${p2.x - dx} ${p2.y}, ${p2.x} ${p2.y}`;
    e.el.setAttribute("d", d);
    e.flow.setAttribute("d", d);
  }

  private bindNode(node: GraphNode, closeBtn: HTMLButtonElement): void {
    const el = node.el;

    el.addEventListener("pointerdown", (e) => {
      const target = e.target as HTMLElement;
      if (target.dataset.ui === "close" || target.dataset.ui === "out") return;
      this.select(node.id);
      const startX = e.clientX;
      const startY = e.clientY;
      const originX = node.pos.x;
      const originY = node.pos.y;
      const move = (ev: PointerEvent): void => {
        const scale = this.worldScale();
        node.pos = {
          x: originX + (ev.clientX - startX) / scale,
          y: originY + (ev.clientY - startY) / scale,
        };
        el.style.transform = `translate(${node.pos.x}px, ${node.pos.y}px)`;
        this.redrawAll();
      };
      const up = (): void => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      e.stopPropagation();
    });

    closeBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      this.removeNode(node.id);
    });

    const outPort = el.querySelector<HTMLElement>('[data-ui="out"]');
    outPort?.addEventListener("pointerdown", (e) => {
      e.stopPropagation();
      e.preventDefault();
      const ghost = document.createElementNS("http://www.w3.org/2000/svg", "path");
      ghost.setAttribute("class", "edge-path");
      ghost.style.opacity = "0.55";
      this.svg.appendChild(ghost);
      this.linking = { from: node.id, ghost };
      outPort.classList.add("linking");
    });
  }

  private bindGhostTracking(): void {
    this.surface.addEventListener("pointermove", (e) => {
      if (!this.linking) return;
      const from = this.nodes.get(this.linking.from);
      if (!from) return;
      const rect = this.surface.getBoundingClientRect();
      const p = {
        x: (e.clientX - rect.left - this.worldOffsetX()) / this.worldScale(),
        y: (e.clientY - rect.top - this.worldOffsetY()) / this.worldScale(),
      };
      const p1 = this.kindCenter(from, "out");
      const dx = Math.max(40, Math.abs(p.x - p1.x) * 0.55);
      this.linking.ghost.setAttribute(
        "d",
        `M ${p1.x} ${p1.y} C ${p1.x + dx} ${p1.y}, ${p.x - dx} ${p.y}, ${p.x} ${p.y}`,
      );
    });

    const finish = (e: PointerEvent): void => {
      if (!this.linking) return;
      const el = document.elementFromPoint(e.clientX, e.clientY);
      const targetEl = el?.closest("[data-node]") as HTMLElement | null;
      const outPort = el?.closest('[data-ui="out"]');
      outPort?.classList.remove("linking");
      const ghost = this.linking.ghost;
      const from = this.linking.from;
      this.linking = null;
      ghost.remove();
      if (targetEl) {
        const to = targetEl.dataset.node;
        if (to && to !== from) {
          this.connect(from, to);
          bus.emit("toast", { text: "pipeline linked", tone: "ok" });
        }
      }
    };
    this.surface.addEventListener("pointerup", finish);
    this.surface.addEventListener("pointerleave", () => {
      if (this.linking) {
        this.linking.ghost.remove();
        this.linking = null;
      }
    });
  }

  private worldOffsetX(): number {
    const m = new DOMMatrixReadOnly(this.world.style.transform);
    return m.m41;
  }

  private worldOffsetY(): number {
    const m = new DOMMatrixReadOnly(this.world.style.transform);
    return m.m42;
  }

  private select(id: string): void {
    if (this.selected && this.selected !== id) {
      this.nodes.get(this.selected)?.el.classList.remove("selected");
    }
    this.selected = id;
    const el = this.nodes.get(id)?.el;
    if (el) {
      el.classList.add("selected");
      // raise WITHOUT re-parenting: moving a node mid-pointer-sequence
      // cancels the browser's click event (classic DOM gotcha)
      this.zTop += 1;
      el.style.zIndex = String(this.zTop);
    }
    bus.emit("node:selected", id);
  }

  private removeNode(id: string): void {
    const node = this.nodes.get(id);
    if (!node) return;
    node.el.remove();
    this.nodes.delete(id);
    for (const [eid, e] of [...this.edges.entries()]) {
      if (e.from === id || e.to === id) {
        e.el.remove();
        e.flow.remove();
        this.edges.delete(eid);
      }
    }
  }
}

export function kindColor(kind: NodeKind): string {
  switch (kind) {
    case "webview":
      return "#22d3ee";
    case "wallet":
      return "#8b5cf6";
    case "proxy":
      return "#2dd4bf";
    case "note":
      return "#94a3b8";
  }
}

export { NODE_W };
