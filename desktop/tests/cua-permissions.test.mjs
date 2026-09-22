// Computer-use permission onboarding (#20): the settings window shows
// Accessibility and Screen Recording as macOS granted them to the Cua
// Driver's own bundle, read from the driver's report, with explicit
// unsupported, headless, missing-driver, unreachable and version-mismatch
// states, and copy that only ever promises one-shot capture during an action.
// A computer without a driver is installed from the page itself, so
// no state ever asks a desktop user to run a command they do not have.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  PERMISSION_NAMES,
  grantOutcomeText,
  installAction,
  installOutcomeText,
  installProgressText,
  intro,
  pageState,
  permissionRows,
  stateText,
  statusLines,
} from "../src/lib/cua-permissions.js";

const source = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

const PIN = "0.28.2";
const INSTALL = "nolune cua install";
const ACTION = "Install driver";
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
  bundle_mismatch: null,
  failed_checks: [],
  ...over,
});

/** A report as the command returns it, with overrides. */
const report = (over = {}) => ({
  platform: macos(),
  pinned_version: PIN,
  install_action: ACTION,
  install_command: INSTALL,
  can_install: true,
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
    incompatibility: `Cua Driver 0.27.0 is not the pinned ${PIN}; Nolune does not update the driver on its own, so install Cua Driver ${PIN} and restart it (${ACTION} below installs the pinned release)`,
  }),
  install: { kind: "stale", version: "0.27.0", driver: DRIVER },
  summary: `Cua Driver 0.27.0 is not the pinned ${PIN}; Nolune does not update the driver on its own, so install Cua Driver ${PIN} and restart it (${ACTION} below installs the pinned release)`,
});

const ABSENT = report({
  install: { kind: "none" },
  driver: { kind: "absent" },
  permissions: null,
  summary: `No Cua Driver is installed on this computer; ${ACTION} puts the pinned one here.`,
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
    reason:
      "Computer use is not available on Linux yet: Nolune drives computers on macOS only for now, so there is nothing to install or grant on this computer. A later release can turn it on without moving the pinned driver.",
  },
  install: { kind: "none" },
  driver: { kind: "skipped" },
  can_install: false,
  permissions: null,
  summary:
    "Computer use is not available on Linux yet: Nolune drives computers on macOS only for now, so there is nothing to install or grant on this computer. A later release can turn it on without moving the pinned driver.",
});

const WINDOWS = report({
  platform: { ...LINUX.platform, os: "windows", triple: "x86_64-pc-windows-msvc", reason: LINUX.platform.reason.replace("Linux", "Windows") },
  install: { kind: "none" },
  driver: { kind: "skipped" },
  can_install: false,
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
  can_install: false,
  permissions: null,
  summary: "No graphical session (launchctl managername reports Background, an SSH or background session, not Aqua): the driver is never started here and there is nothing to grant. Headless installs need nothing from this page.",
});

const FORK = "com.example.fork";
const PATH_DRIVER = "/opt/homebrew/bin/cua-driver";

/** A driver on the pinned version from PATH, built as another bundle: its grants are that bundle's. */
const OTHER_BUNDLE = report({
  install: { kind: "none" },
  driver: reported({
    path: PATH_DRIVER,
    bundle: FORK,
    bundle_mismatch: `The driver at ${PATH_DRIVER} holds its grants as ${FORK}, not as CuaDriver (${BUNDLE}), the bundle Nolune's pinned driver runs as; nothing is granted through it (${ACTION} below installs the pinned release)`,
  }),
  permissions: { accessibility: "granted", screen_recording: "denied" },
  summary: `The driver at ${PATH_DRIVER} holds its grants as ${FORK}, not as CuaDriver (${BUNDLE}), the bundle Nolune's pinned driver runs as; nothing is granted through it (${ACTION} below installs the pinned release)`,
});

const ALL = { READY: report(), DENIED, NEVER_ASKED, MISMATCH, OTHER_BUNDLE, ABSENT, UNREACHABLE, LINUX, WINDOWS, HEADLESS };

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

test("a version mismatch fails clearly with both versions and offers the pinned install", () => {
  assert.equal(pageState(MISMATCH), "incompatible");
  const lines = statusLines(MISMATCH);
  // What the driver itself reports: both versions and the way out.
  const mismatch = lines.find((line) => line.tone === "error" && line.text.includes("does not update"));
  assert.ok(mismatch, JSON.stringify(lines));
  assert.ok(mismatch.text.includes("0.27.0") && mismatch.text.includes(PIN), mismatch.text);
  assert.ok(mismatch.text.includes(ACTION), mismatch.text);
  // What the workspace holds, against the pin.
  const install = lines.find((line) => line.text.includes("is installed for Nolune"));
  assert.ok(install && install.tone === "error" && install.text.includes("0.27.0") && install.text.includes("install the pinned one below"), JSON.stringify(lines));
  // The button reinstalls over the stale one rather than leaving it in place.
  const action = installAction(MISMATCH);
  assert.ok(action && action.force, JSON.stringify(action));
  assert.ok(action.label.includes(PIN), action.label);
  // The rows still say what the driver reported, but nothing is granted through a wrong driver.
  const rows = permissionRows(MISMATCH);
  assert.equal(rows.length, 2);
  assert.ok(rows.every((row) => row.canGrant === false), "no grant through an incompatible driver");
});

test("the install line names what the workspace holds against the pin, and the fix is a button", () => {
  // A manifest whose binary is gone.
  const missing = report({ install: { kind: "missing", version: PIN, driver: DRIVER } });
  const [missingLine] = statusLines(missing);
  assert.equal(missingLine.tone, "error");
  assert.ok(missingLine.text.includes(PIN) && missingLine.text.includes("gone"), missingLine.text);
  assert.ok(missingLine.text.includes("install it again below"), missingLine.text);
  assert.equal(pageState(missing), "ready", "the driver that reported is still the pinned one");

  // A manifest that cannot be read.
  const detail = "cannot read /Users/me/.nolune/cua-driver/install.json: Permission denied (os error 13)";
  const unreadable = report({ install: { kind: "unreadable", detail } });
  const [unreadableLine] = statusLines(unreadable);
  assert.equal(unreadableLine.tone, "error");
  assert.ok(unreadableLine.text.includes(detail), unreadableLine.text);
  assert.ok(unreadableLine.text.includes("install it again below"), unreadableLine.text);

  // Nothing installed for Nolune, but a pinned driver on PATH reported: said, not flagged.
  const fromPath = report({ install: { kind: "none" }, driver: reported({ path: PATH_DRIVER }) });
  const [pathLine, versionLine] = statusLines(fromPath);
  assert.equal(pathLine.tone, "muted");
  assert.ok(pathLine.text.includes(PATH_DRIVER), pathLine.text);
  assert.ok(pathLine.text.includes("installed for Nolune"), pathLine.text);
  assert.equal(versionLine.tone, "ok");
  assert.equal(pageState(fromPath), "ready");
  assert.equal(permissionRows(fromPath).length, 2);
  assert.ok(statusLines(fromPath).every((line) => line.tone !== "error"), "nothing is wrong with a pinned driver from PATH");
});

test("a driver under another bundle shows that bundle's grants as such, and nothing is granted through it", () => {
  assert.equal(pageState(OTHER_BUNDLE), "incompatible");
  // Whose grants these are: the reported bundle, never the constant.
  const lead = intro(OTHER_BUNDLE);
  assert.ok(lead.includes(FORK), lead);
  assert.ok(!lead.includes(BUNDLE), lead);
  assert.ok(lead.includes("not to this app"), lead);
  const lines = statusLines(OTHER_BUNDLE);
  const mismatch = lines.find((line) => line.tone === "error" && line.text.includes(FORK));
  assert.ok(mismatch, JSON.stringify(lines));
  assert.ok(mismatch.text.includes(BUNDLE), mismatch.text);
  assert.ok(mismatch.text.includes(ACTION), mismatch.text);
  assert.ok(lines.some((line) => line.tone === "ok" && line.text.includes("matches the pin")), "the version itself is fine");
  // The rows say what the driver reported, but nothing is granted through the wrong bundle.
  const rows = permissionRows(OTHER_BUNDLE);
  assert.deepEqual(rows.map((row) => [row.stateText, row.canGrant]), [["Granted", false], ["Denied", false]]);
  // A page that must still describe a grant names the reported bundle.
  const note = grantOutcomeText({ permission: "screen_recording", driver_grant: false, opened_settings: true }, OTHER_BUNDLE);
  assert.ok(note.includes(FORK) && !note.includes(BUNDLE), note);

  // The JS check stands on its own: a report without the Rust wording still flags the bundle.
  const bare = report({ driver: reported({ bundle: FORK }) });
  assert.equal(pageState(bare), "incompatible");
  const bareLine = statusLines(bare).find((line) => line.tone === "error");
  assert.ok(bareLine && bareLine.text.includes(FORK) && bareLine.text.includes(BUNDLE) && bareLine.text.includes("Install the pinned driver below"), JSON.stringify(statusLines(bare)));
  assert.ok(permissionRows(NEVER_ASKED).every((row) => row.canGrant), "the pinned bundle grants");
  assert.ok(permissionRows(report({ driver: reported({ bundle: FORK }), permissions: NEVER_ASKED.permissions })).every((row) => !row.canGrant));
  // A report that names no bundle is not a mismatch; the constant stands in.
  const unnamed = report({ driver: reported({ bundle: null }) });
  assert.equal(pageState(unnamed), "ready");
  assert.ok(intro(unnamed).includes(BUNDLE), intro(unnamed));
  assert.ok(statusLines(unnamed).every((line) => line.tone !== "error"));
});

test("no driver offers the install itself, with no rows to grant yet", () => {
  assert.equal(pageState(ABSENT), "absent");
  assert.deepEqual(permissionRows(ABSENT), []);
  const lines = statusLines(ABSENT);
  assert.ok(lines.some((line) => line.tone === "error" && line.text.includes("install it below")), JSON.stringify(lines));
  assert.ok(ABSENT.summary.includes(ACTION));
  // A first install has nothing to overwrite, so it does not force.
  const action = installAction(ABSENT);
  assert.ok(action, "a computer without a driver is offered one");
  assert.equal(action.label, ACTION);
  assert.equal(action.force, false);
  assert.ok(action.note.includes(PIN), action.note);
  assert.ok(action.note.includes("checksum"), action.note);
  assert.ok(action.note.includes("never updates the driver on its own"), action.note);
  // A manifest pointing at a binary that is gone is overwritten.
  assert.equal(installAction(report({ install: { kind: "missing", version: PIN, driver: DRIVER }, driver: { kind: "absent" }, permissions: null })).force, true);
  assert.equal(installAction(report({ install: { kind: "unreadable", detail: "denied" }, driver: { kind: "absent" }, permissions: null })).force, true);
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
    assert.ok(lines[0].text.includes("nothing to install or grant"), lines[0].text);
    assert.equal(installAction(unsupported), null, `${name}: nothing to install`);
  }
});

test("a headless host says the driver is never started and nothing is needed", () => {
  assert.equal(pageState(HEADLESS), "headless");
  assert.deepEqual(permissionRows(HEADLESS), []);
  const [line] = statusLines(HEADLESS);
  assert.equal(line.tone, "muted");
  assert.ok(line.text.includes("never started"), line.text);
  assert.ok(line.text.includes("Headless installs need nothing"), line.text);
  assert.equal(installAction(HEADLESS), null, "a driver that is never started is not worth downloading");
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

test("the settings window renders the report through the pure views and the three commands", () => {
  const page = source("../src/routes/settings/+page.svelte");
  for (const required of ['"cua_permissions"', '"cua_grant_permission"', '"cua_install_driver"', "$lib/cua-permissions", "permissionRows", "statusLines", "pageState", "grantOutcomeText", "installAction", "installProgressText", "installOutcomeText", 'role="alert"', "Retry", "Refresh status"]) {
    assert.ok(page.includes(required), `settings page has ${required}`);
  }
  for (const stale of ["Take screenshots of your screen", "Nolune needs these permissions to control your computer", "nolune cua"]) {
    assert.ok(!page.includes(stale), `settings page no longer says ${JSON.stringify(stale)}`);
  }
});

/**
 * The whole point of the page installing the driver itself: a desktop user
 * has no `nolune` on their PATH (the in-app server install never puts one
 * there, and a desktop bound to a server elsewhere has no binary at all),
 * so no state may answer "run this command".
 */
test("no state on the page ever asks the user to run a terminal command", () => {
  const texts = [];
  for (const sample of Object.values(ALL)) {
    texts.push(sample.summary, intro(sample), ...statusLines(sample).map((line) => line.text));
    const action = installAction(sample);
    if (action) texts.push(action.label, action.note);
    for (const row of permissionRows(sample)) texts.push(row.desc);
    for (const permission of ["accessibility", "screen_recording"]) {
      texts.push(grantOutcomeText({ permission, driver_grant: true, opened_settings: true }, sample));
    }
  }
  for (const text of texts) {
    assert.ok(!text.includes(INSTALL), `${JSON.stringify(text)} sends the user to a terminal`);
    assert.ok(!text.includes("nolune cua"), `${JSON.stringify(text)} names a CLI command`);
  }
});

test("a running install is narrated step by step, in the order the installer reports them", () => {
  assert.equal(
    installProgressText({ step: "downloading", url: "https://example/cua.tar.gz", size: 70_000_000 }),
    "Downloading the driver (70.0 MB)…",
  );
  const verified = installProgressText({ step: "verified", sha256: "a".repeat(64) });
  assert.ok(verified.includes("Checksum matches the pin"), verified);
  const checked = installProgressText({ step: "version_checked", version: PIN });
  assert.ok(checked.includes(PIN) && checked.includes("pinned version"), checked);
});

test("a finished install says what to do next and whether the companion already knows", () => {
  const fresh = installOutcomeText({ version: PIN, driver: DRIVER, already_installed: false, reannounced: true });
  assert.ok(fresh.includes(`Installed Cua Driver ${PIN}`), fresh);
  assert.ok(fresh.includes(DRIVER), fresh);
  assert.ok(fresh.includes("Accessibility") && fresh.includes("Screen recording"), fresh);
  assert.ok(fresh.includes("registering with your companion again"), fresh);

  const offline = installOutcomeText({ version: PIN, driver: DRIVER, already_installed: true, reannounced: false });
  assert.ok(offline.includes("was already installed"), offline);
  assert.ok(offline.includes("next time this app connects"), offline);
});

test("a ready computer is not offered a download it does not need", () => {
  assert.equal(installAction(report()), null);
  // An unreachable driver is reinstalled rather than left wedged.
  const stuck = installAction(UNREACHABLE);
  assert.ok(stuck && stuck.force, JSON.stringify(stuck));
  assert.ok(stuck.label.toLowerCase().includes("reinstall"), stuck.label);
  // A host the pin covers no driver for is never offered one, whatever it reports.
  assert.equal(installAction({ ...ABSENT, can_install: false }), null);
});
