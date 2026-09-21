// @ts-check
/**
 * The desktop overlay's companion state (#86): the Tauri events the bridge
 * already emits (`computer-use-action` after each action it performed on
 * this computer, `computer-use-idle` when the relay connection ends) mapped
 * onto the client's companion-state reducer, so the overlay's moon makes the
 * same face for the same state as the chat. The reducer and the expression
 * table are imported from the client source, never copied.
 */
import { initialCompanionState, reduceCompanion } from "../../../client/src/lib/companion/state.js";

/** The one "conversation" the overlay sees: the actions performed here. */
export const OVERLAY_CHAT = "desktop";

/** Human labels for the bridge's action names; anything else keeps its name. */
const ACTION_LABELS = Object.freeze({
  screenshot: "Screenshot",
  left_click: "Click",
  right_click: "Right click",
  middle_click: "Middle click",
  double_click: "Double click",
  mouse_move: "Move",
  scroll: "Scroll",
  type: "Typing",
  key: "Key",
  bash: "Command",
  switch_desktop: "Switch space",
});

/**
 * The line an action reads as: its label, with the detail after a colon.
 * @param {string} name
 * @param {string} detail
 */
export function actionText(name, detail) {
  const label = ACTION_LABELS[/** @type {keyof typeof ACTION_LABELS} */ (name)] ?? name;
  return detail ? `${label}: ${detail}` : label;
}

/**
 * The overlay starts connected and idle: its events arrive over Tauri's own
 * bus, so there is no socket to wait for, and nothing has happened yet.
 */
export function overlayInitialState() {
  return reduceCompanion(initialCompanionState(), { type: "connection", connected: true });
}

/**
 * A Tauri event as a reducer event. An action is work on this computer.
 * Idle means the connection ended: the run is over, and nothing says it
 * succeeded, so it ends the way a conversation snapshot does.
 * @param {string} name
 * @param {string | undefined} payload
 * @returns {import("../../../client/src/lib/companion/state.js").CompanionEvent | null}
 */
export function overlayEvent(name, payload) {
  switch (name) {
    case "computer-use-action": {
      let summary = payload ?? "";
      try {
        const data = JSON.parse(payload ?? "");
        summary = actionText(String(data.action ?? ""), String(data.detail ?? ""));
      } catch {
        // Not JSON: the payload is the whole line.
      }
      return { type: "action", chatId: OVERLAY_CHAT, tool: "computer_use", summary, machine: null };
    }
    case "computer-use-idle":
      return { type: "snapshot", chatId: OVERLAY_CHAT, running: false };
    default:
      return null;
  }
}
