/**
 * Infinite viewport: pan on empty-canvas drag, zoom on wheel, world<->screen
 * transforms. The world lives in one transformed <div>; nodes position
 * absolutely inside it.
 */

export interface Vec2 {
  x: number;
  y: number;
}

export class Viewport {
  private scale = 1;
  private tx = 0;
  private ty = 0;

  readonly world: HTMLDivElement;
  private surface: HTMLElement;
  private minScale = 0.35;
  private maxScale = 2.2;

  constructor(surface: HTMLElement, world: HTMLDivElement) {
    this.surface = surface;
    this.world = world;
    this.bind();
    this.apply();
  }

  get zoom(): number {
    return this.scale;
  }

  screenToWorld(p: Vec2): Vec2 {
    const rect = this.surface.getBoundingClientRect();
    return {
      x: (p.x - rect.left - this.tx) / this.scale,
      y: (p.y - rect.top - this.ty) / this.scale,
    };
  }

  zoomBy(factor: number, center?: Vec2): void {
    const next = Math.min(this.maxScale, Math.max(this.minScale, this.scale * factor));
    const rect = this.surface.getBoundingClientRect();
    const cx = center ? center.x - rect.left : rect.width / 2;
    const cy = center ? center.y - rect.top : rect.height / 2;
    const wx = (cx - this.tx) / this.scale;
    const wy = (cy - this.ty) / this.scale;
    this.scale = next;
    this.tx = cx - wx * this.scale;
    this.ty = cy - wy * this.scale;
    this.apply();
  }

  panBy(dx: number, dy: number): void {
    this.tx += dx;
    this.ty += dy;
    this.apply();
  }

  /** Center on a world point at current zoom. */
  focusOn(world: Vec2): void {
    const rect = this.surface.getBoundingClientRect();
    this.tx = rect.width / 2 - world.x * this.scale;
    this.ty = rect.height / 2 - world.y * this.scale;
    this.apply();
  }

  fitAll(bounds: { minX: number; minY: number; maxX: number; maxY: number }): void {
    const rect = this.surface.getBoundingClientRect();
    const pad = 70;
    const w = Math.max(1, bounds.maxX - bounds.minX);
    const h = Math.max(1, bounds.maxY - bounds.minY);
    const scale = Math.min(
      this.maxScale,
      Math.max(this.minScale, Math.min((rect.width - pad * 2) / w, (rect.height - pad * 2) / h)),
    );
    this.scale = scale;
    this.tx = rect.width / 2 - (bounds.minX + w / 2) * scale;
    this.ty = rect.height / 2 - (bounds.minY + h / 2) * scale;
    this.apply();
  }

  private apply(): void {
    this.world.style.transform = `translate(${this.tx}px, ${this.ty}px) scale(${this.scale})`;
    this.world.dataset.zoom = String(this.scale);
  }

  private bind(): void {
    this.surface.addEventListener("wheel", (e) => {
      e.preventDefault();
      const factor = e.deltaY < 0 ? 1.12 : 1 / 1.12;
      this.zoomBy(factor, { x: e.clientX, y: e.clientY });
    }, { passive: false });

    let panning = false;
    let last: Vec2 = { x: 0, y: 0 };
    this.surface.addEventListener("pointerdown", (e) => {
      const target = e.target as HTMLElement;
      if (target.closest("[data-node]") || target.closest("[data-ui]")) return;
      panning = true;
      last = { x: e.clientX, y: e.clientY };
      this.surface.setPointerCapture(e.pointerId);
      this.surface.classList.add("cursor-grabbing");
    });
    this.surface.addEventListener("pointermove", (e) => {
      if (!panning) return;
      this.panBy(e.clientX - last.x, e.clientY - last.y);
      last = { x: e.clientX, y: e.clientY };
    });
    const stop = (e: PointerEvent): void => {
      if (!panning) return;
      panning = false;
      try {
        this.surface.releasePointerCapture(e.pointerId);
      } catch {
        /* pointer already released */
      }
      this.surface.classList.remove("cursor-grabbing");
    };
    this.surface.addEventListener("pointerup", stop);
    this.surface.addEventListener("pointercancel", stop);
  }
}
