import { useEffect, useState, type ReactNode } from "react";

import { getConfig, getRunning, saveConfig, setRunning } from "../api";
import type { Anchor, Config, Orientation, OverlayConfig } from "../types";
import {
  DEFAULT_BG_COLOR,
  DEFAULT_BG_OPACITY,
  DEFAULT_GRAPH_HEIGHT_PX,
  DEFAULT_MARGIN_PX,
  DEFAULT_MAX_Y_MS,
  DEFAULT_SCALE,
  DEFAULT_TIMEOUT_MS,
  MAX_SCALE,
  MIN_WINDOW_SECONDS,
} from "../types";

const ANCHORS: Anchor[] = [
  "topLeft",
  "topCenter",
  "topRight",
  "centerLeft",
  "center",
  "centerRight",
  "bottomLeft",
  "bottomCenter",
  "bottomRight",
];

const ORIENTATIONS: Orientation[] = [0, 90, 180, 270];

function newOverlay(): OverlayConfig {
  return {
    id: crypto.randomUUID(),
    name: "New overlay",
    enabled: true,
    probe: { protocol: "icmp", host: "1.1.1.1" },
    orientation: 0,
    mirrored: false,
    lineColor: "#4ade80",
    timeoutColor: "#ef4444",
    position: "topRight",
    windowSeconds: 60,
    scale: DEFAULT_SCALE,
    timeoutMs: DEFAULT_TIMEOUT_MS,
    graphHeightPx: DEFAULT_GRAPH_HEIGHT_PX,
    maxYMs: DEFAULT_MAX_Y_MS,
    marginPx: DEFAULT_MARGIN_PX,
    bgColor: DEFAULT_BG_COLOR,
    bgOpacity: DEFAULT_BG_OPACITY,
  };
}

export function ConfigWindow() {
  const [config, setConfig] = useState<Config | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [running, setRunningState] = useState(true);
  const [status, setStatus] = useState("");
  const [dirty, setDirty] = useState(false);
  const [confirmId, setConfirmId] = useState<string | null>(null);

  useEffect(() => {
    getConfig().then((cfg) => {
      setConfig(cfg);
      setSelectedId(cfg.overlays[0]?.id ?? null);
    });
    getRunning().then(setRunningState);
  }, []);

  if (!config) {
    return <div className="empty">Loading…</div>;
  }

  const selected = config.overlays.find((o) => o.id === selectedId) ?? null;

  /** Right-panel edits: staged until Save. */
  const update = (overlay: OverlayConfig) => {
    setConfig({
      ...config,
      overlays: config.overlays.map((o) => (o.id === overlay.id ? overlay : o)),
    });
    setDirty(true);
    setStatus("");
  };

  /** Sidebar actions (add / enable / delete): applied and persisted immediately. */
  const persist = async (next: Config) => {
    setConfig(next);
    setDirty(false);
    try {
      await saveConfig(next);
      setStatus("");
    } catch (err) {
      setStatus(`Save failed: ${String(err)}`);
    }
  };

  const addOverlay = () => {
    const overlay = newOverlay();
    setSelectedId(overlay.id);
    void persist({ ...config, overlays: [...config.overlays, overlay] });
  };

  const toggleEnabled = (id: string) => {
    void persist({
      ...config,
      overlays: config.overlays.map((o) =>
        o.id === id ? { ...o, enabled: !o.enabled } : o,
      ),
    });
  };

  const deleteOverlay = (id: string) => {
    const overlays = config.overlays.filter((o) => o.id !== id);
    if (selectedId === id) setSelectedId(overlays[0]?.id ?? null);
    void persist({ ...config, overlays });
  };

  const onSave = async () => {
    try {
      await saveConfig(config);
      setDirty(false);
      setStatus("Saved.");
    } catch (err) {
      setStatus(`Save failed: ${String(err)}`);
    }
  };

  const onToggleRunning = async () => {
    const next = !running;
    await setRunning(next);
    setRunningState(next);
  };

  return (
    <div className="config">
      <main>
        <aside className="sidebar">
          <div className="overlay-list">
            {config.overlays.length === 0 && (
              <p className="status">No overlays yet. Add one to get started.</p>
            )}
            {config.overlays.map((o) => (
              <div key={o.id} className={`item${o.id === selectedId ? " active" : ""}`}>
                {confirmId === o.id ? (
                  <>
                    <span className="confirm-label">Delete "{o.name || "overlay"}"?</span>
                    <button
                      className="remove confirm"
                      title="Confirm delete"
                      onClick={() => {
                        setConfirmId(null);
                        deleteOverlay(o.id);
                      }}
                    >
                      ✓
                    </button>
                    <button className="cancel" title="Cancel" onClick={() => setConfirmId(null)}>
                      ✕
                    </button>
                  </>
                ) : (
                  <>
                    <button className="name" onClick={() => setSelectedId(o.id)}>
                      {o.name || "(unnamed)"}
                    </button>
                    <button
                      className="remove"
                      title="Delete overlay"
                      onClick={() => setConfirmId(o.id)}
                    >
                      ✕
                    </button>
                    <button
                      className="toggle"
                      title={o.enabled ? "Disable overlay" : "Enable overlay"}
                      onClick={() => toggleEnabled(o.id)}
                    >
                      {o.enabled ? "⏸" : "▶"}
                    </button>
                  </>
                )}
              </div>
            ))}
          </div>

          <div className="sidebar-actions">
            <button onClick={addOverlay}>Add overlay</button>
            <button onClick={onToggleRunning}>{running ? "Pause all" : "Resume all"}</button>
            <button className="primary" onClick={onSave} disabled={!dirty}>
              Save
            </button>
            {status && <span className="status">{status}</span>}
          </div>
        </aside>

        <div className="editor">
          {selected ? (
            <OverlayEditor overlay={selected} onChange={update} />
          ) : (
            <p className="empty">Select an overlay, or add a new one.</p>
          )}
        </div>
      </main>
    </div>
  );
}

function OverlayEditor({
  overlay,
  onChange,
}: {
  overlay: OverlayConfig;
  onChange: (overlay: OverlayConfig) => void;
}) {
  const patch = (changes: Partial<OverlayConfig>) => onChange({ ...overlay, ...changes });

  const isTcp = overlay.probe.protocol === "tcp";
  const host = overlay.probe.host;
  const port = overlay.probe.protocol === "tcp" ? overlay.probe.port : 80;

  const setProbe = (protocol: "icmp" | "tcp") => {
    patch({
      probe:
        protocol === "tcp"
          ? { protocol: "tcp", host, port: port || 80 }
          : { protocol: "icmp", host },
    });
  };

  return (
    <>
      <Section title="General">
        <div className="row">
          <Field label="Name">
            <input value={overlay.name} onChange={(e) => patch({ name: e.target.value })} />
          </Field>
          <Field label="Protocol">
            <select
              value={overlay.probe.protocol}
              onChange={(e) => setProbe(e.target.value as "icmp" | "tcp")}
            >
              <option value="icmp">ICMP echo</option>
              <option value="tcp">TCP connect</option>
            </select>
          </Field>
        </div>
        <div className="row">
          <Field label="Target host / IP">
            <input
              value={host}
              onChange={(e) => setProbe2(patch, overlay, { host: e.target.value })}
            />
          </Field>
          <Field label="Port (TCP only)">
            <input
              type="number"
              min={1}
              max={65535}
              disabled={!isTcp}
              value={port}
              onChange={(e) => setProbe2(patch, overlay, { port: Number(e.target.value) })}
            />
          </Field>
        </div>
        <div className="row">
          <Field label="Timeout (ms)">
            <input
              type="number"
              min={1}
              value={overlay.timeoutMs}
              onChange={(e) => patch({ timeoutMs: Number(e.target.value) })}
            />
          </Field>
        </div>
      </Section>

      <Section title="Graph">
        <div className="row">
          <Field label="Position">
            <select
              value={overlay.position}
              onChange={(e) => patch({ position: e.target.value as Anchor })}
            >
              {ANCHORS.map((a) => (
                <option key={a} value={a}>
                  {a}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Margin (px)">
            <input
              type="number"
              min={0}
              value={overlay.marginPx}
              onChange={(e) => patch({ marginPx: Number(e.target.value) })}
            />
          </Field>
        </div>
        <div className="row">
          <Field label={`Window (seconds, min ${MIN_WINDOW_SECONDS})`}>
            <input
              type="number"
              min={MIN_WINDOW_SECONDS}
              value={overlay.windowSeconds}
              onChange={(e) => patch({ windowSeconds: Number(e.target.value) })}
            />
          </Field>
          <Field label={`Visual scale: ${overlay.scale}×`}>
            <input
              type="range"
              min={1}
              max={MAX_SCALE}
              step={1}
              value={overlay.scale}
              onChange={(e) => patch({ scale: Number(e.target.value) })}
            />
          </Field>
        </div>
        <div className="row">
          <Field label="Orientation">
            <select
              value={overlay.orientation}
              onChange={(e) => patch({ orientation: Number(e.target.value) as Orientation })}
            >
              {ORIENTATIONS.map((o) => (
                <option key={o} value={o}>
                  {o}°
                </option>
              ))}
            </select>
          </Field>
          <Field label="Mirrored">
            <select
              value={overlay.mirrored ? "yes" : "no"}
              onChange={(e) => patch({ mirrored: e.target.value === "yes" })}
            >
              <option value="no">No</option>
              <option value="yes">Yes</option>
            </select>
          </Field>
        </div>
        <div className="row">
          <Field label="Graph height (px)">
            <input
              type="number"
              min={10}
              value={overlay.graphHeightPx}
              onChange={(e) => patch({ graphHeightPx: Number(e.target.value) })}
            />
          </Field>
          <Field label="Latency ceiling (ms)">
            <input
              type="number"
              min={1}
              value={overlay.maxYMs}
              onChange={(e) => patch({ maxYMs: Number(e.target.value) })}
            />
          </Field>
        </div>
      </Section>

      <Section title="Colors">
        <div className="row">
          <Field label="Line color">
            <input
              type="color"
              value={overlay.lineColor}
              onChange={(e) => patch({ lineColor: e.target.value })}
            />
          </Field>
          <Field label="Timeout color">
            <input
              type="color"
              value={overlay.timeoutColor}
              onChange={(e) => patch({ timeoutColor: e.target.value })}
            />
          </Field>
        </div>
        <div className="row">
          <Field label="Background color">
            <input
              type="color"
              value={overlay.bgColor}
              onChange={(e) => patch({ bgColor: e.target.value })}
            />
          </Field>
          <Field label={`Background opacity: ${overlay.bgOpacity}%`}>
            <input
              type="range"
              min={0}
              max={100}
              step={1}
              value={overlay.bgOpacity}
              onChange={(e) => patch({ bgOpacity: Number(e.target.value) })}
            />
          </Field>
        </div>
      </Section>
    </>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="section">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="field">
      <label>{label}</label>
      {children}
    </div>
  );
}

/** Update one side of the probe union without losing the other side. */
function setProbe2(
  patch: (changes: Partial<OverlayConfig>) => void,
  overlay: OverlayConfig,
  changes: { host?: string; port?: number },
) {
  const probe = overlay.probe;
  if (probe.protocol === "tcp") {
    patch({
      probe: {
        protocol: "tcp",
        host: changes.host ?? probe.host,
        port: changes.port ?? probe.port,
      },
    });
  } else {
    patch({ probe: { protocol: "icmp", host: changes.host ?? probe.host } });
  }
}
