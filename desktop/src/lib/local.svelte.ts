// In-app server install (#128): download the server binary, run `nolune onboard`,
// start `nolune gateway` as an app-managed child, and report progress. Everything
// with side effects happens in native commands; this store only reflects them.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type LocalStatus = {
  supported: boolean;
  target: string | null;
  home: string;
  binary_installed: boolean;
  config_exists: boolean;
  gateway_running: boolean;
  port_in_use: boolean;
};

export type Step = "idle" | "downloading" | "preparing" | "starting" | "ready" | "error";

type InstallOutcome = { url: string; version: string };
type Progress = { downloaded: number; total: number | null };

export const MAX_LOG_LINES = 500;

const STEPS: readonly Step[] = ["idle", "downloading", "preparing", "starting", "ready", "error"];
const BUSY: readonly Step[] = ["downloading", "preparing", "starting"];

export const local = $state({
  status: null as LocalStatus | null,
  step: "idle" as Step,
  progress: 0,
  logs: [] as string[],
  showLogs: false,
  error: null as string | null,
  nightly: false,
  url: null as string | null,
  version: null as string | null,
});

let busy = false;
let unlisteners: UnlistenFn[] = [];

/** True when the install button should be usable: not unsupported, not already busy. */
export function canInstall(): boolean {
  if (local.status && !local.status.supported) return false;
  return !busy && !BUSY.includes(local.step);
}

export async function refreshLocalStatus(): Promise<LocalStatus | null> {
  try {
    const status = await invoke<LocalStatus>("local_server_status");
    local.status = status;
    return status;
  } catch {
    local.status = null;
    local.error = "Could not check whether Nolune is installed on this computer.";
    return null;
  }
}

export async function subscribeLocalEvents(): Promise<void> {
  if (unlisteners.length) return;
  unlisteners = await Promise.all([
    listen<string>("local-install-step", (event) => {
      if ((STEPS as readonly string[]).includes(event.payload)) local.step = event.payload as Step;
    }),
    listen<Progress>("local-install-progress", (event) => {
      const { downloaded, total } = event.payload;
      local.progress = total ? Math.min(1, downloaded / total) : 0;
    }),
    listen<string>("local-gateway-log", (event) => appendLog(event.payload)),
  ]);
}

export function appendLog(line: string): void {
  local.logs.push(line);
  if (local.logs.length > MAX_LOG_LINES) {
    local.logs.splice(0, local.logs.length - MAX_LOG_LINES);
  }
}

/** Native install errors are written for the user (offline, rate limit, port in use). */
function installMessage(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error.trim();
  const message = (error as { message?: unknown } | null)?.message;
  if (typeof message === "string" && message.trim()) return message.trim();
  return "Install failed. Show logs for details.";
}

export async function installLocal(): Promise<boolean> {
  if (busy) return false;
  busy = true;
  local.step = "downloading";
  local.progress = 0;
  local.error = null;
  local.url = null;
  local.version = null;
  try {
    const outcome = await invoke<InstallOutcome>("install_local_server", {
      channel: local.nightly ? "nightly" : "stable",
    });
    local.url = outcome.url;
    local.version = outcome.version;
    local.step = "ready";
    return true;
  } catch (error) {
    local.step = "error";
    local.error = installMessage(error);
    local.showLogs = true;
    return false;
  } finally {
    busy = false;
  }
}

/** On relaunch: start the app-managed gateway for an existing local install. */
export async function startLocalGateway(): Promise<boolean> {
  if (busy) return false;
  busy = true;
  local.step = "starting";
  local.error = null;
  try {
    local.url = await invoke<string>("start_local_gateway");
    local.step = "ready";
    return true;
  } catch {
    local.step = "error";
    local.error = "Could not start the Nolune server on this computer. Show logs for details.";
    local.showLogs = true;
    return false;
  } finally {
    busy = false;
  }
}

export function toggleLogs(): void {
  local.showLogs = !local.showLogs;
}
