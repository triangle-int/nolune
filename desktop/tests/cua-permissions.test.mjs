// Computer-use permission onboarding (#20): the settings window shows
// Accessibility and Screen Recording as macOS granted them to the Cua
// Driver's own bundle, read from the driver's report, with explicit
// unsupported, headless, missing-driver, unreachable and version-mismatch
// states, and copy that only ever promises one-shot capture during an action.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  PERMISSION_NAMES,
  grantOutcomeText,
  intro,
  pageState,
  permissionRows,
  stateText,
  statusLines,
} from "../src/lib/cua-permissions.js";

const source = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

const PIN = "0.28.2";
const INSTALL = "nolune cua install";
const BUNDLE = "com.trycua.driver";
const DRIVER = "/Users/me/.nolune/cua-driver/releases/0.28.2/CuaDriver.app/Contents/MacOS/cua-driver";

const macos = () => ({ kind: "supported", os: "macos", triple: "aarch64-apple-darwin" });
const pinned = () => ({ kind: "pinned", version: PIN, driver: DRIVER });
const reported = (over = {}) => ({
  kind: "reported",
  path: DRIVER,
  version: PIN,
  compatible: true,
  incompatibility: null,
  health: "ok",
  bundle: BUNDLE,
  failed_checks: [],
  ...over,
});

/** A report as the command returns it, with overrides. */
const report = (over = {}) => ({
  platform: macos(),
  pinned_version: PIN,
  install_command: INSTALL,
  driver_bundle: BUNDLE,
  install: pinned(),
  driver: reported(),
  permissions: { accessibility: "granted", screen_recording: "granted" },
  summary: `CuaDriver ${PIN} holds Accessibility and Screen Recording; computer use is ready.`,
  ...over,
});

const DENIED = report({
  driver: reported({
    health: "degraded",
    failed_checks: [
      { name: "tcc_accessibility", message: "Accessibility is not granted.", hint: "Run cua-driver permissions grant." },
      { name: "ax_capability", message: "AX is not trusted.", hint: "Grant Accessibility." },
    ],
  }),
  permissions: { accessibility: "denied", screen_recording: "granted" },
  summary: `CuaDriver ${PIN} is missing Accessibility; grant it below.`,
});

const NEVER_ASKED = report({
  permissions: { accessibility: "prompt_required", screen_recording: "prompt_required" },
  summary: `CuaDriver ${PIN} is missing Accessibility and Screen Recording; grant them below.`,
});

const MISMATCH = report({
  driver: reported({
    version: "0.27.0",
    compatible: false,
    incompatibility: `Cua Driver 0.27.0 is not the pinned ${PIN}; Nolune does not update the driver on its own, so install Cua Driver ${PIN} and restart it (\`${INSTALL}\` installs the pinned release)`,
  }),
  install: { kind: "stale", version: "0.27.0", driver: DRIVER },
  summary: `Cua Driver 0.27.0 is not the pinned ${PIN}; Nolune does not update the driver on its own, so install Cua Driver ${PIN} and restart it (\`${INSTALL}\` installs the pinned release)`,
});

const ABSENT = report({
  install: { kind: "none" },
  driver: { kind: "absent" },
  permissions: null,
  summary: `No Cua Driver is installed on this computer; run \`${INSTALL}\`, then check again.`,
});

const UNREACHABLE = report({
  driver: { kind: "unreachable", path: DRIVER, error: 'the MCP handshake timed out; the driver said: "CuaDriver daemon is not running"' },
  permissions: null,
  summary: `The driver at ${DRIVER} could not report: the MCP handshake timed out; the driver said: "CuaDriver daemon is not running"`,
});

const LINUX = report({
  platform: {
    kind: "unsupported",
    os: "linux",
    triple: "x86_64-unknown-linux-gnu",
    reason: `Computer use is not available on Linux yet: Nolune drives computers on macOS only for now. The pinned driver still installs with \`${INSTALL}\` so a later release can turn it on; there is nothing to grant here.`,
  },
  install: { kind: "none" },
  driver: { kind: "skipped" },
  permissions: null,
  summary: `Computer use is not available on Linux yet: Nolune drives computers on macOS only for now. The pinned driver still installs with \`${INSTALL}\` so a later release can turn it on; there is nothing to grant here.`,
});

const WINDOWS = report({
  platform: { ...LINUX.platform, os: "windows", triple: "x86_64-pc-windows-msvc", reason: LINUX.platform.reason.replace("Linux", "Windows") },
  install: { kind: "none" },
  driver: { kind: "skipped" },
  permissions: null,
  summary: LINUX.summary.replace("Linux", "Windows"),
});

const HEADLESS = report({
  platform: {
    kind: "headless",
    os: "macos",
    triple: "aarch64-apple-darwin",
    reason: "No graphical session (launchctl managername reports Background, an SSH or background session, not Aqua): the driver is never started here and there is nothing to grant. Headless installs need nothing from this page.",
  },
  driver: { kind: "skipped" },
  permissions: null,
  summary: "No graphical session (launchctl managername reports Background, an SSH or background session, not Aqua): the driver is never started here and there is nothing to grant. Headless installs need nothing from this page.",
});

const ALL = { READY: report(), DENIED, NEVER_ASKED, MISMATCH, ABSENT, UNREACHABLE, LINUX, WINDOWS, HEADLESS };

test("a driver holding both grants shows two granted rows with nothing to grant", () => {
  const ready = report();
  assert.equal(pageState(ready), "ready");
  const rows = permissionRows(ready);
  assert.deepEqual(rows.map((row) => [row.key, row.name, row.stateText, row.canGrant]), [
    ["accessibility", "Accessibility", "Granted", false],
    ["screen_recording", "Screen recording", "Granted", false],
  ]);
  assert.deepEqual(PERMISSION_NAMES, { accessibility: "Accessibility", screen_recording: "Screen recording" });
  const lines = statusLines(ready);
  assert.ok(lines.some((line) => line.tone === "ok" && line.text.includes(PIN) && line.text.includes("matches the pin")), JSON.stringify(lines));
  assert.ok(lines.every((line) => line.tone !== "error"), "nothing is wrong");
  // The lead says whose grants these are.
  assert.ok(intro(ready).includes(BUNDLE), intro(ready));
  assert.ok(intro(ready).includes("not to this app"), intro(ready));
});

test("a denied permission reads Denied, can be granted, and carries the driver's hint", () => {
  assert.equal(pageState(DENIED), "ready");
  const [accessibility, screen] = permissionRows(DENIED);
  assert.equal(accessibility.state, "denied");
  assert.equal(accessibility.stateText, "Denied");
  assert.equal(accessibility.canGrant, true);
  assert.equal(accessibility.hint, "Run cua-driver permissions grant.");
  assert.equal(screen.stateText, "Granted");
  assert.equal(screen.canGrant, false);
  assert.equal(screen.hint, null);
  const health = statusLines(DENIED).find((line) => line.text.includes("degraded"));
  assert.ok(health, JSON.stringify(statusLines(DENIED)));
  assert.equal(health.tone, "error");
});

test("a permission the driver never asked for reads Not asked yet and can be granted", () => {
  for (const row of permissionRows(NEVER_ASKED)) {
    assert.equal(row.state, "prompt_required");
    assert.equal(row.stateText, "Not asked yet");
    assert.equal(row.canGrant, true);
  }
  assert.equal(stateText("granted"), "Granted");
  assert.equal(stateText("denied"), "Denied");
  assert.equal(stateText("prompt_required"), "Not asked yet");
  assert.equal(stateText("unavailable"), "Unavailable");
});

test("a version mismatch fails clearly with both versions and the install command", () => {
  assert.equal(pageState(MISMATCH), "incompatible");
  const lines = statusLines(MISMATCH);
  const mismatch = lines.find((line) => line.tone === "error" && line.text.includes("0.27.0"));
  assert.ok(mismatch, JSON.stringify(lines));
  assert.ok(mismatch.text.includes(PIN), mismatch.text);
  assert.ok(mismatch.text.includes(INSTALL), mismatch.text);
  const install = lines.find((line) => line.text.includes("installed"));
  assert.ok(install && install.tone === "error" && install.text.includes(INSTALL), JSON.stringify(lines));
  // The rows still say what the driver reported, but nothing is granted through a wrong driver.
  const rows = permissionRows(MISMATCH);
  assert.equal(rows.length, 2);
  assert.ok(rows.every((row) => row.canGrant === false), "no grant through an incompatible driver");
});

test("no driver means the install command and no rows", () => {
  assert.equal(pageState(ABSENT), "absent");
  assert.deepEqual(permissionRows(ABSENT), []);
  const lines = statusLines(ABSENT);
  assert.ok(lines.some((line) => line.tone === "error" && line.text.includes(INSTALL)), JSON.stringify(lines));
  assert.ok(ABSENT.summary.includes(INSTALL));
});

test("a driver that cannot report shows what it said", () => {
  assert.equal(pageState(UNREACHABLE), "unreachable");
  assert.deepEqual(permissionRows(UNREACHABLE), []);
  const line = statusLines(UNREACHABLE).find((line) => line.tone === "error");
  assert.ok(line && line.text.includes("CuaDriver daemon is not running"), JSON.stringify(statusLines(UNREACHABLE)));
});

test("Linux and Windows are named as unsupported with what still works", () => {
  for (const [name, unsupported] of [["Linux", LINUX], ["Windows", WINDOWS]]) {
    assert.equal(pageState(unsupported), "unsupported", name);
    assert.deepEqual(permissionRows(unsupported), [], name);
    const lines = statusLines(unsupported);
    assert.equal(lines.length, 1, `${name}: one line, the reason`);
    assert.equal(lines[0].tone, "muted");
    assert.ok(lines[0].text.includes(name), lines[0].text);
    assert.ok(lines[0].text.includes("macOS only"), lines[0].text);
    assert.ok(lines[0].text.includes(INSTALL), lines[0].text);
    assert.ok(lines[0].text.includes("nothing to grant"), lines[0].text);
  }
});

test("a headless host says the driver is never started and nothing is needed", () => {
  assert.equal(pageState(HEADLESS), "headless");
  assert.deepEqual(permissionRows(HEADLESS), []);
  const [line] = statusLines(HEADLESS);
  assert.equal(line.tone, "muted");
  assert.ok(line.text.includes("never started"), line.text);
  assert.ok(line.text.includes("Headless installs need nothing"), line.text);
});

test("a grant explains the prompt, the pane, and what to do when neither helps", () => {
  const both = grantOutcomeText({ permission: "accessibility", driver_grant: true, opened_settings: true }, DENIED);
  assert.ok(both.includes("CuaDriver"), both);
  assert.ok(both.includes("Accessibility"), both);
  assert.ok(both.includes("System Settings"), both);
  const paneOnly = grantOutcomeText({ permission: "screen_recording", driver_grant: false, opened_settings: true }, DENIED);
  assert.ok(paneOnly.includes("Screen recording"), paneOnly);
  assert.ok(!paneOnly.includes("prompt"), "no prompt was requested");
  assert.ok(paneOnly.includes("System Settings"), paneOnly);
});

test("every sentence promises one-shot capture during an action and never more", () => {
  const forbidden = ["continuous", "always on", "always-on", "stream", "watch", "records your screen", "record your screen", "in the background"];
  const texts = [];
  for (const sample of Object.values(ALL)) {
    texts.push(intro(sample), ...statusLines(sample).map((line) => line.text));
    for (const row of permissionRows(sample)) texts.push(row.desc, row.stateText);
    for (const permission of ["accessibility", "screen_recording"]) {
      texts.push(grantOutcomeText({ permission, driver_grant: true, opened_settings: true }, sample));
      texts.push(grantOutcomeText({ permission, driver_grant: false, opened_settings: true }, sample));
    }
  }
  for (const text of texts) {
    const lowered = text.toLowerCase();
    for (const token of forbidden) assert.ok(!lowered.includes(token), `${JSON.stringify(text)} says ${JSON.stringify(token)}`);
  }
  const [, screen] = permissionRows(report());
  assert.ok(screen.desc.includes("one-shot"), screen.desc);
  assert.ok(screen.desc.toLowerCase().includes("nothing is recorded"), screen.desc);
  assert.ok(intro(report()).includes("one-shot"), intro(report()));
});

test("the settings window renders the report through the pure views and the two commands", () => {
  const page = source("../src/routes/settings/+page.svelte");
  for (const required of ['"cua_permissions"', '"cua_grant_permission"', "$lib/cua-permissions", "permissionRows", "statusLines", "pageState", "grantOutcomeText", 'role="alert"', "Retry", "Refresh status"]) {
    assert.ok(page.includes(required), `settings page has ${required}`);
  }
  for (const stale of ["Take screenshots of your screen", "Nolune needs these permissions to control your computer"]) {
    assert.ok(!page.includes(stale), `settings page no longer says ${JSON.stringify(stale)}`);
  }
});
