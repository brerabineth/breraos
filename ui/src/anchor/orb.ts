/**
 * Quantum-Glow Security Anchor: a state-driven orb rendered on 2D canvas.
 * secure → teal/cyan, warn → amber, alert → red pulse, locked → slate.
 */

import type { AnchorState } from "../core/types";

const STATE_COLORS: Record<AnchorState, { core: string; halo: string; ring: string }> = {
  secure: { core: "#5eead4", halo: "rgba(45,212,191,0.55)", ring: "rgba(34,211,238,0.9)" },
  warn: { core: "#fcd34d", halo: "rgba(251,191,36,0.5)", ring: "rgba(251,191,36,0.85)" },
  alert: { core: "#fca5a5", halo: "rgba(248,113,113,0.6)", ring: "rgba(248,113,113,0.9)" },
  locked: { core: "#94a3b8", halo: "rgba(100,116,139,0.4)", ring: "rgba(100,116,139,0.7)" },
};

const STATE_LABEL: Record<AnchorState, string> = {
  secure: "SECURE",
  warn: "PARTIAL",
  alert: "EXPOSED",
  locked: "LOCKED",
};

interface Particle {
  a: number;
  r: number;
  s: number;
  z: number;
}

export class Anchor {
  private ctx: CanvasRenderingContext2D;
  private labelEl: HTMLElement;
  private state: AnchorState = "locked";
  private t = 0;
  private raf = 0;
  private particles: Particle[] = [];

  constructor(canvas: HTMLCanvasElement, labelEl: HTMLElement, _subEl: HTMLElement) {
    this.labelEl = labelEl;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("canvas 2d unavailable");
    this.ctx = ctx;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = 168 * dpr;
    canvas.height = 168 * dpr;
    ctx.scale(dpr, dpr);
    for (let i = 0; i < 14; i++) {
      this.particles.push({
        a: Math.random() * Math.PI * 2,
        r: 46 + Math.random() * 26,
        s: 0.15 + Math.random() * 0.5,
        z: 0.8 + Math.random() * 1.6,
      });
    }
    this.loop();
  }

  setState(state: AnchorState): void {
    if (this.state === state) return;
    this.state = state;
    this.labelEl.textContent = STATE_LABEL[state];
    const c = STATE_COLORS[state];
    this.labelEl.style.color = c.core;
    this.labelEl.style.textShadow = `0 0 18px ${c.halo}`;
  }

  private loop = (): void => {
    this.t += 0.016;
    this.draw();
    this.raf = requestAnimationFrame(this.loop);
  };

  private draw(): void {
    const ctx = this.ctx;
    const cx = 84;
    const cy = 84;
    const c = STATE_COLORS[this.state];
    ctx.clearRect(0, 0, 168, 168);

    // halo
    const breathe = 1 + Math.sin(this.t * 1.4) * 0.05;
    const halo = ctx.createRadialGradient(cx, cy, 6, cx, cy, 74 * breathe);
    halo.addColorStop(0, c.halo);
    halo.addColorStop(0.55, c.halo.replace("0.5", "0.16").replace("0.55", "0.18").replace("0.6", "0.2").replace("0.4", "0.12"));
    halo.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = halo;
    ctx.beginPath();
    ctx.arc(cx, cy, 80 * breathe, 0, Math.PI * 2);
    ctx.fill();

    // rotating hex ring
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate(this.t * 0.4);
    ctx.strokeStyle = c.ring;
    ctx.lineWidth = 1.4;
    ctx.globalAlpha = 0.85;
    hexPath(ctx, 56);
    ctx.stroke();
    ctx.rotate(-this.t * 1.05);
    ctx.globalAlpha = 0.35;
    hexPath(ctx, 44);
    ctx.stroke();
    ctx.restore();

    // core
    const core = ctx.createRadialGradient(cx - 6, cy - 8, 2, cx, cy, 26);
    core.addColorStop(0, "#ffffff");
    core.addColorStop(0.35, c.core);
    core.addColorStop(1, "rgba(0,0,0,0.15)");
    ctx.fillStyle = core;
    ctx.beginPath();
    ctx.arc(cx, cy, 26 * breathe, 0, Math.PI * 2);
    ctx.fill();

    // particles
    ctx.globalAlpha = 0.8;
    for (const p of this.particles) {
      p.a += 0.002 * p.s * (this.state === "alert" ? 3 : 1);
      const px = cx + Math.cos(p.a) * p.r;
      const py = cy + Math.sin(p.a) * p.r;
      ctx.fillStyle = c.core;
      ctx.globalAlpha = 0.25 + 0.35 * Math.abs(Math.sin(this.t * p.s * 2));
      ctx.beginPath();
      ctx.arc(px, py, p.z, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  destroy(): void {
    cancelAnimationFrame(this.raf);
  }
}

function hexPath(ctx: CanvasRenderingContext2D, r: number): void {
  ctx.beginPath();
  for (let i = 0; i < 6; i++) {
    const a = (Math.PI / 3) * i - Math.PI / 2;
    const x = Math.cos(a) * r;
    const y = Math.sin(a) * r;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.closePath();
}

export function anchorStatusLabel(s: AnchorState): string {
  return STATE_LABEL[s];
}
