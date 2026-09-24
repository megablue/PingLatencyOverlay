import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

import { getConfig } from "../api";
import { drawGraph } from "../graph";
import type { Config, OverlayConfig, Sample } from "../types";

/**
 * Renders a single overlay window: a live line graph fed by `latency://<id>`
 * events, and restyled live from `config://updated` events.
 */
export function OverlayView({ overlayId }: { overlayId: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const cfgRef = useRef<OverlayConfig | null>(null);
  const samplesRef = useRef<Sample[]>([]);
  const frameRef = useRef<number | null>(null);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    const schedule = () => {
      if (frameRef.current != null) return;
      frameRef.current = requestAnimationFrame(() => {
        frameRef.current = null;
        const canvas = canvasRef.current;
        const cfg = cfgRef.current;
        if (canvas && cfg) drawGraph(canvas, cfg, samplesRef.current);
      });
    };

    const applyConfig = (cfg: OverlayConfig) => {
      cfgRef.current = cfg;
      const buffer = samplesRef.current;
      while (buffer.length > cfg.windowSeconds) buffer.shift();
      schedule();
    };

    const addUnlisten = (fn: () => void) => {
      if (disposed) fn();
      else unlisteners.push(fn);
    };

    getConfig()
      .then((config) => {
        const cfg = config.overlays.find((o) => o.id === overlayId);
        if (!cfg || disposed) return;
        cfgRef.current = cfg;
        samplesRef.current = [];
        schedule();
      })
      .catch(() => {
        /* config not ready yet; window will be reconciled again */
      });

    listen<Sample>(`latency://${overlayId}`, (event) => {
      const buffer = samplesRef.current;
      buffer.push(event.payload);
      const max = cfgRef.current?.windowSeconds ?? buffer.length;
      while (buffer.length > max) buffer.shift();
      schedule();
    })
      .then(addUnlisten)
      .catch(() => {});

    listen<Config>("config://updated", (event) => {
      const cfg = event.payload.overlays.find((o) => o.id === overlayId);
      if (cfg) applyConfig(cfg);
    })
      .then(addUnlisten)
      .catch(() => {});

    const onResize = () => schedule();
    window.addEventListener("resize", onResize);

    return () => {
      disposed = true;
      window.removeEventListener("resize", onResize);
      if (frameRef.current != null) cancelAnimationFrame(frameRef.current);
      for (const fn of unlisteners) fn();
    };
  }, [overlayId]);

  return <canvas ref={canvasRef} className="overlay-canvas" />;
}
