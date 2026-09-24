import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { ConfigWindow } from "./components/ConfigWindow";
import { OverlayView } from "./components/OverlayView";
import "./styles.css";

const OVERLAY_PREFIX = "overlay-";
const label = getCurrentWindow().label;

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);

if (label.startsWith(OVERLAY_PREFIX)) {
  document.body.classList.add("overlay-body");
  const overlayId = label.slice(OVERLAY_PREFIX.length);
  root.render(
    <React.StrictMode>
      <OverlayView overlayId={overlayId} />
    </React.StrictMode>,
  );
} else {
  root.render(
    <React.StrictMode>
      <ConfigWindow />
    </React.StrictMode>,
  );
}
