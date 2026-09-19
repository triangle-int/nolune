// "Run in background" (#129): the store hands the gateway to a service and back through
// one native command, and reports which mode is active.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { compileModule } from "svelte/compiler";

const source = compileModule(ts.transpile(
  readFileSync(new URL("../src/lib/local.svelte.ts", import.meta.url), "utf8"),
  { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
), { generate: "server", filename: "local.svelte.js" }).js.code;

const OFF = { supported: true, installed: false, running: true, managed_by_app: true };
const ON = { supported: true, installed: true, running: true, managed_by_app: false };

async function setup(options = {}) {
  const calls = [];
  const invoke = async (name, args) => {
    calls.push({ name, args });
    if (options.fail === name) throw new Error(options.failMessage ?? "TOP_SECRET native detail");
    if (name === "background_service_status") return options.status ?? OFF;
    if (name === "set_background_service") {
      if (options.duringToggle) await options.duringToggle();
      return args.enabled ? ON : OFF;
    }
  };
  const listen = async () => () => {};
  const context = vm.createContext({ URL, setTimeout, clearTimeout });
  const module = new vm.SourceTextModule(source, { context });
  await module.link((name) => {
    if (name === "svelte/internal/server") return new vm.SyntheticModule([], function () {}, { context });
    if (name === "@tauri-apps/api/core") return new vm.SyntheticModule(["invoke"], function () { this.setExport("invoke", invoke); }, { context });
    if (name === "@tauri-apps/api/event") return new vm.SyntheticModule(["listen"], function () { this.setExport("listen", listen); }, { context });
    assert.fail(`unexpected import ${name}`);
  });
  await module.evaluate();
  return { api: module.namespace, calls };
}

test("status refresh reports which mode is active", async () => {
  const { api, calls } = await setup({ status: ON });
  const status = await api.refreshBackgroundStatus();
  assert.deepEqual(calls.map(({ name }) => name), ["background_service_status"]);
  assert.equal(status.installed, true);
  assert.equal(api.background.status.managed_by_app, false);
  assert.equal(api.canToggleBackground(), true);
});

test("unsupported platforms cannot toggle and never invoke", async () => {
  const { api, calls } = await setup({ status: { supported: false, installed: false, running: false, managed_by_app: false } });
  await api.refreshBackgroundStatus();
  assert.equal(api.canToggleBackground(), false);
  assert.equal(await api.setBackgroundService(true), false);
  assert.equal(calls.filter((c) => c.name === "set_background_service").length, 0);
});

test("turning on hands the gateway to the service through one command", async () => {
  const { api, calls } = await setup();
  await api.refreshBackgroundStatus();
  assert.equal(await api.setBackgroundService(true), true);
  assert.equal(JSON.stringify(calls.at(-1)), JSON.stringify({ name: "set_background_service", args: { enabled: true } }));
  assert.equal(api.background.status.installed, true);
  assert.equal(api.background.status.managed_by_app, false);
  assert.equal(api.background.error, null);
  assert.equal(api.background.busy, false);
});

test("turning off takes the gateway back", async () => {
  const { api } = await setup({ status: ON });
  await api.refreshBackgroundStatus();
  assert.equal(await api.setBackgroundService(false), true);
  assert.equal(api.background.status.installed, false);
  assert.equal(api.background.status.managed_by_app, true);
});

test("toggle failure keeps the native message and the previous mode", async () => {
  const { api } = await setup({ fail: "set_background_service", failMessage: "launchctl bootstrap failed (exit 5)" });
  await api.refreshBackgroundStatus();
  assert.equal(await api.setBackgroundService(true), false);
  assert.equal(api.background.error, "launchctl bootstrap failed (exit 5)");
  assert.equal(api.background.status.installed, false, "mode must not flip on failure");
  assert.equal(api.background.busy, false);
  assert.equal(api.canToggleBackground(), true, "retry must remain possible");
});

test("status failure is a safe message", async () => {
  const { api } = await setup({ fail: "background_service_status" });
  assert.equal(await api.refreshBackgroundStatus(), null);
  assert.equal(api.background.status, null);
  assert.equal(api.background.error.includes("TOP_SECRET"), false);
});

test("a second toggle cannot start while one is running", async () => {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const { api, calls } = await setup({ duringToggle: () => gate });
  await api.refreshBackgroundStatus();
  const first = api.setBackgroundService(true);
  assert.equal(api.background.busy, true);
  assert.equal(await api.setBackgroundService(false), false);
  release();
  assert.equal(await first, true);
  assert.equal(calls.filter((c) => c.name === "set_background_service").length, 1);
});
