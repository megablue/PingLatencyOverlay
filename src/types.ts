export type Anchor =
  | "topLeft"
  | "topCenter"
  | "topRight"
  | "centerLeft"
  | "center"
  | "centerRight"
  | "bottomLeft"
  | "bottomCenter"
  | "bottomRight";

export type Orientation = 0 | 90 | 180 | 270;

export type ProbeConfig =
  | { protocol: "icmp"; host: string }
  | { protocol: "tcp"; host: string; port: number };

export interface OverlayConfig {
  id: string;
  name: string;
  enabled: boolean;
  probe: ProbeConfig;
  orientation: Orientation;
  mirrored: boolean;
  lineColor: string;
  timeoutColor: string;
  position: Anchor;
  /** Visible time window in seconds; each tick is one ping. */
  windowSeconds: number;
  /** Visual scale: pixels per tick. */
  scale: number;
  /** Ping timeout in milliseconds. */
  timeoutMs: number;
  /** Height of the Y axis on screen, in logical pixels. */
  graphHeightPx: number;
  /** Latency ceiling in milliseconds; higher pings clamp to the top. */
  maxYMs: number;
  /** Gap between the overlay and the screen edge, in logical pixels. */
  marginPx: number;
  /** Background color drawn behind the graph. */
  bgColor: string;
  /** Background opacity, 0 (transparent) to 100 (opaque). */
  bgOpacity: number;
}

export interface Config {
  overlays: OverlayConfig[];
}

export interface Sample {
  latency: number | null;
}

export const MIN_WINDOW_SECONDS = 30;
export const DEFAULT_TIMEOUT_MS = 1000;
export const DEFAULT_GRAPH_HEIGHT_PX = 60;
export const DEFAULT_MAX_Y_MS = 1000;
export const MAX_SCALE = 10;
export const DEFAULT_SCALE = 2;
export const DEFAULT_MARGIN_PX = 20;
export const DEFAULT_BG_COLOR = "#0f172a";
export const DEFAULT_BG_OPACITY = 0;
