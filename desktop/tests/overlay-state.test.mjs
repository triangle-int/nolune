import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { OVERLAY_CHAT, actionText, overlayEvent, overlayInitialState } from "../src/lib/overlay-state.js";
import { companionStatusText, reduceCompanion } from "../../client/src/lib/companion/state.js";
import { companionExpression } from "../../client/src/lib/companion/expressions.js";

const source = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
const reduce = (state, events) => events.reduce((s, e) => reduceCompanion(s, e), state);

test("the desktop overlay runs the same companion-state model as the client", () => {
  // The overlay only ever sees actions performed on this computer, so it
  // starts connected and idle; the events map onto the shared reducer.
  const idle = overlayInitialState();
  assert.equal(idle.kind, "idle");
  assert.equal(companionStatusText(idle), "Nolune is idle.");
  const click = overlayEvent("computer-use-action", JSON.stringify({ action: "left_click", detail: "120, 40" }));
  assert.deepEqual(click, { type: "action", chatId: OVERLAY_CHAT, tool: "computer_use", summary: "Click: 120, 40", machine: null });
  const working = reduce(idle, [click]);
  assert.equal(working.kind, "working");
  assert.equal(companionStatusText(working), "Nolune is working on this computer: Click: 120, 40.");
  assert.deepEqual(overlayEvent("computer-use-action", JSON.stringify({ action: "screenshot", detail: "" })).summary, "Screenshot", "no detail, no colon");
  assert.deepEqual(overlayEvent("computer-use-action", "not json").summary, "not json", "an unparsable payload is shown as it came");
  assert.equal(actionText("bash", "ls -la"), "Command: ls -la");
  assert.equal(actionText("switch_desktop", ""), "Switch space");
  assert.equal(actionText("wiggle", "x"), "wiggle: x", "an unknown action keeps its name");
  // Idle from the bridge means the connection ended, not that the task succeeded.
  const idleAgain = reduce(working, [overlayEvent("computer-use-idle", undefined)]);
  assert.equal(idleAgain.kind, "idle");
  assert.equal(idleAgain.completed, false, "the overlay never claims a completion the runtime did not report");
  assert.equal(reduce(idle, [overlayEvent("computer-use-idle", undefined)]), idle, "idle while idle changes nothing");
  assert.equal(overlayEvent("something-else", "{}"), null);
});

test("the overlay draws the shared expressions and holds still under reduced motion", () => {
  const working = companionExpression("working");
  assert.equal(working.expression, "working");
  assert.notEqual(working.motion, "none");
  assert.equal(companionExpression("working", { reducedMotion: true }).motion, "none");
  assert.equal(companionExpression("working", { reducedMotion: true }).expression, "working", "the face stays, only the motion stops");
});

test("the overlay page listens to the existing Tauri events and reads the shared model", () => {
  const page = source("../src/routes/overlay/+page.svelte");
  for (const required of ['"computer-use-action"', '"computer-use-idle"', "overlay-state", "companionExpression", "companionStatusText", 'aria-live="polite"', "prefers-reduced-motion"]) {
    assert.ok(page.includes(required), `overlay page has ${required}`);
  }
  assert.ok(!page.includes("actionLabels"), "the action labels moved into the pure module");
  const moon = source("../src/lib/components/Moon.svelte");
  assert.ok(moon.includes("eyesMarkup") && moon.includes("expression"), "Moon.svelte draws the expression the shared table describes");
});
