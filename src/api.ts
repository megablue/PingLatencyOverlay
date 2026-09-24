import { invoke } from "@tauri-apps/api/core";
import type { Config } from "./types";

export function getConfig(): Promise<Config> {
  return invoke<Config>("get_config");
}

export function saveConfig(config: Config): Promise<void> {
  return invoke<void>("save_config", { config });
}

export function getRunning(): Promise<boolean> {
  return invoke<boolean>("get_running");
}

export function setRunning(running: boolean): Promise<void> {
  return invoke<void>("set_running", { running });
}
