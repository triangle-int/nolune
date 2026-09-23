import { invoke } from "@tauri-apps/api/core";

export type Connection = { url: string };
type Draft = { url: string; token: string };
class ConnectionError extends Error {}

export const auth = $state({
  connection: null as Connection | null,
  loading: true,
  error: null as string | null,
  message: null as string | null,
  /** The server stopped accepting this app (revoked in Settings): pair again. */
  signedOut: false,
});

// Stable codes the native side returns for pairing and validation. Anything
// else is shown as a generic failure so native error text never reaches the UI.
const NATIVE_ERRORS: Record<string, string> = {
  invalid_code: "That code didn't work. Codes expire after 5 minutes and only work once. A code made in a browser also needs the same server address that browser uses.",
  rate_limited: "Too many attempts. Wait about 10 minutes, then get a fresh code.",
  auth_disabled: "This server has authentication turned off, so there is nothing to pair. Connect with an API token instead, or set auth_token in its config.toml.",
  pairing_unsupported: "This server is too old for pairing codes. Update it, or connect with its API token.",
  unreachable: "Could not reach the server. Check the address and that Nolune is running.",
  signed_out: "This app is no longer paired with the server. Pair it again with a fresh code.",
};

function nativeError(error: unknown, fallback: string) {
  return new ConnectionError(typeof error === "string" && error in NATIVE_ERRORS ? NATIVE_ERRORS[error] : fallback);
}

/** Pairing codes are eight digits shown as `1234-5678`. */
export function formatPairingCode(raw: string) {
  const digits = String(raw ?? "").replace(/\D/g, "").slice(0, 8);
  return digits.length > 4 ? `${digits.slice(0, 4)}-${digits.slice(4)}` : digits;
}

export function isCompletePairingCode(raw: string) {
  return String(raw ?? "").replace(/\D/g, "").length === 8;
}

export function normalizeConnection(url: string, token: string): Draft {
  let parsed: URL;
  try {
    const input = url.trim();
    parsed = new URL(input.includes("://") ? input : `http://${input}`);
  } catch {
    throw new ConnectionError("Enter a valid server URL.");
  }
  if (!["http:", "https:"].includes(parsed.protocol) || !parsed.hostname ||
      parsed.username || parsed.password || parsed.search || parsed.hash || parsed.pathname !== "/") {
    throw new ConnectionError("Use a root HTTP or HTTPS server URL without a base path, credentials, query parameters, or a fragment.");
  }
  if (/\r|\n/.test(token)) throw new ConnectionError("Enter a valid auth token.");
  return { url: parsed.origin, token: token.trim() };
}

export async function init() {
  auth.loading = true;
  auth.connection = null;
  auth.error = null;
  try {
    await invoke("clear_legacy_browser_auth");
    const url = await invoke<string | null>("initialize_saved_connection");
    auth.connection = url ? { url } : null;
  } catch {
    auth.error = "Could not restore the saved connection. Unlock the OS credential store and retry.";
  } finally {
    auth.loading = false;
  }
}

async function run(action: () => Promise<void>) {
  if (auth.loading) return false;
  auth.loading = true;
  auth.error = null;
  auth.message = null;
  try {
    await action();
    return true;
  } catch (error) {
    auth.error = error instanceof ConnectionError ? error.message : "Connection operation failed. Please retry.";
    return false;
  } finally {
    auth.loading = false;
  }
}

function normalizedSavedUrl(url: string) {
  const normalized = normalizeConnection(url, "").url;
  if (!auth.connection || auth.connection.url !== normalized) {
    throw new ConnectionError("Enter the auth token when changing the server URL.");
  }
  return normalized;
}

export async function testConnection(url: string, token = "") {
  return run(async () => {
    const config = normalizeConnection(url, token);
    try {
      if (config.token) {
        await invoke("test_connection", config);
      } else {
        normalizedSavedUrl(config.url);
        await invoke("test_saved_connection");
      }
    } catch (error) {
      if (error instanceof ConnectionError) throw error;
      if (error === "signed_out" && config.token) throw new ConnectionError("The server rejected that token.");
      if (error === "signed_out") auth.signedOut = true;
      throw nativeError(error === "signed_out" ? error : null, "Connection operation failed. Please retry.");
    }
    auth.message = "Connection test succeeded.";
  });
}

/** Redeem a one-time pairing code; the native side keeps the device token. */
export async function pairConnection(url: string, code: string) {
  return run(async () => {
    const { url: origin } = normalizeConnection(url, "");
    if (!isCompletePairingCode(code)) throw new ConnectionError("Enter the eight-digit pairing code.");
    let savedUrl: string;
    try {
      savedUrl = await invoke<string>("pair_connection", { url: origin, code: formatPairingCode(code) });
    } catch (error) {
      throw nativeError(error, "Could not pair with the server. Please retry.");
    }
    auth.connection = { url: savedUrl };
    auth.signedOut = false;
    auth.message = "Paired. This app now has its own sign-in, which you can revoke from Settings → Connections.";
  });
}

export async function saveConnection(url: string, token: string) {
  return run(async () => {
    const config = normalizeConnection(url, token);
    if (!config.token) normalizedSavedUrl(config.url);
    const savedUrl = await invoke<string>("save_connection", config);
    auth.connection = { url: savedUrl };
    auth.signedOut = false;
    auth.message = "Connection saved.";
  });
}

export async function openConnection() {
  return run(async () => {
    if (!auth.connection) return;
    try {
      await invoke("open_saved_connection");
    } catch (error) {
      await invoke("disconnect_computer_use").catch(() => {});
      if (error === "signed_out") auth.signedOut = true;
      throw nativeError(error === "signed_out" ? error : null, "Could not open the companion. Please reconnect.");
    }
  });
}

export async function disconnect() {
  return run(async () => {
    let failed = false;
    try { await invoke("disconnect_computer_use"); } catch { failed = true; }
    try { await invoke("delete_saved_connection"); } catch { failed = true; }
    auth.connection = null;
    auth.signedOut = false;
    if (failed) throw new ConnectionError("Some credentials could not be removed. Unlock the OS credential store and retry Disconnect.");
  });
}
