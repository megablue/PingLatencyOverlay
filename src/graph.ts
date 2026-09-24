import type { OverlayConfig, Sample } from "./types";

/** Convert `#rrggbb` to an `rgba(...)` string with the given alpha. */
function hexToRgba(hex: string, alpha: number): string {
  const match = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) return `rgba(0, 0, 0, ${alpha})`;
  const n = parseInt(match[1], 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/**
 * Draw one frame of an overlay graph.
 *
 * Rendering rules (see docs/SPEC.md):
 * - One tick per ping. The graph fills the window's long axis, so the effective
 *   pixels-per-tick is `viewport / windowSeconds` (the window itself is sized
 *   from `windowSeconds * scale` on the Rust side).
 * - The first point starts at the first responding latency, not y0.
 * - Timeouts are not interpolated: a vertical line spans y0..yMax in the
 *   timeout color. The line resumes at the next responding sample's X using the
 *   last responding Y.
 * - The Y axis ceiling is `maxYMs` (latency above it clamps to the top).
 * - `orientation` rotates the whole graph anticlockwise (90 => plots bottom to
 *   top); `mirrored` flips the amplitude axis.
 *
 * NOTE: this uses the canvas's *actual* CSS size, not `windowSeconds * scale`.
 * The webview's devicePixelRatio can differ from the scale Tauri used to size
 * the window, so relying on the requested size clips the graph.
 */
export function drawGraph(
  canvas: HTMLCanvasElement,
  cfg: OverlayConfig,
  samples: Sample[],
): void {
  const dpr = window.devicePixelRatio || 1;
  const cw = canvas.clientWidth;
  const ch = canvas.clientHeight;
  if (cw === 0 || ch === 0) return;

  const targetW = Math.round(cw * dpr);
  const targetH = Math.round(ch * dpr);
  if (canvas.width !== targetW || canvas.height !== targetH) {
    canvas.width = targetW;
    canvas.height = targetH;
  }

  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.scale(dpr, dpr);

  // Background fill (drawn before the transform, so it never rotates).
  if (cfg.bgOpacity > 0) {
    ctx.fillStyle = hexToRgba(cfg.bgColor, cfg.bgOpacity / 100);
    ctx.fillRect(0, 0, cw, ch);
  }

  const rotated = cfg.orientation === 90 || cfg.orientation === 270;
  const longPx = rotated ? ch : cw; // time axis (viewport)
  const shortPx = rotated ? cw : ch; // amplitude axis (viewport)
  const step = longPx / Math.max(1, cfg.windowSeconds);

  // Map logical graph space (x: time, y: 0 = yMax, shortPx = y0) onto screen.
  ctx.save();
  ctx.translate(cw / 2, ch / 2);
  // Anticlockwise rotation (canvas rotate() is clockwise for positive angles).
  ctx.rotate((-cfg.orientation * Math.PI) / 180);
  if (cfg.mirrored) ctx.scale(1, -1);
  ctx.translate(-longPx / 2, -shortPx / 2);

  const yMax = Math.max(1, cfg.maxYMs);
  // Inset from the edges so values at y0 and yMax stay visible instead of being
  // clipped by the half-width of the stroke.
  const pad = 2;
  const top = pad;
  const bottom = shortPx - pad;
  const mapY = (value: number) => {
    const t = Math.min(value, yMax) / yMax;
    return bottom - t * (bottom - top);
  };
  const tickX = (index: number) => index * step + step / 2;

  ctx.lineWidth = 1.5;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";

  // Timeout markers: a full-height vertical line in the timeout color.
  ctx.strokeStyle = cfg.timeoutColor;
  for (let i = 0; i < samples.length; i++) {
    if (samples[i].latency == null) {
      const x = tickX(i);
      ctx.beginPath();
      ctx.moveTo(x, top);
      ctx.lineTo(x, bottom);
      ctx.stroke();
    }
  }

  // Latency line, broken across timeouts.
  ctx.strokeStyle = cfg.lineColor;
  ctx.beginPath();
  let inSegment = false;
  let lastY: number | null = null;

  for (let i = 0; i < samples.length; i++) {
    const sample = samples[i];
    const x = tickX(i);

    if (sample.latency == null) {
      inSegment = false;
      continue;
    }

    const y = mapY(sample.latency);
    if (!inSegment) {
      if (lastY == null) {
        // Very first responding sample: start here, not at y0.
        ctx.moveTo(x, y);
      } else {
        // Resume from the last responding Y, then jump to the new value.
        ctx.moveTo(x, lastY);
        ctx.lineTo(x, y);
      }
      inSegment = true;
    } else {
      ctx.lineTo(x, y);
    }
    lastY = y;
  }
  ctx.stroke();

  ctx.restore();
}
