// `local.svelte.ts` drives the in-app server install (#128) through native commands
// and Tauri events. Both are mocked here; the store must never touch anything else.
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

async function setup(options = {}) {
  const calls = [];
  const handlers = new Map();
  const invoke = async (name, args) => {
    calls.push({ name, args });
    if (options.fail === name) throw new Error(options.failMessage ?? "TOP_SECRET native detail");
    if (name === "local_server_status") {
      return options.status ?? {
        supported: true, target: "aarch64-apple-darwin", home: "/Users/me/.nolune",
        binary_installed: false, config_exists: false, gateway_running: false, port_in_use: false,
      };
    }
    if (name === "install_local_server") {
      if (options.duringInstall) await options.duringInstall(emit);
      return { url: "http://localhost:26559", version: "v0.35.0" };
    }
    if (name === "start_local_gateway") return "http://localhost:26559";
  };
  const emit = (event, payload) => {
    for (const handler of handlers.get(event) ?? []) handler({ event, payload });
  };
  const listen = async (event, handler) => {
    handlers.set(event, [...(handlers.get(event) ?? []), handler]);
    return () => handlers.set(event, (handlers.get(event) ?? []).filter((h) => h !== handler));
  };
  const context = vm.createContext({ URL, setTimeout, clearTimeout });
  const module = new vm.SourceTextModule(source, { context });
  await module.link((name) => {
    if (name === "svelte/internal/server") {
      return new vm.SyntheticModule([], function () {}, { context });
    }
    if (name === "@tauri-apps/api/core") {
      return new vm.SyntheticModule(["invoke"], function () { this.setExport("invoke", invoke); }, { context });
    }
    if (name === "@tauri-apps/api/event") {
      return new vm.SyntheticModule(["listen"], function () { this.setExport("listen", listen); }, { context });
    }
    assert.fail(`unexpected import ${name}: the local store must only use core invoke and events`);
  });
  await module.evaluate();
  return { api: module.namespace, calls, emit, handlers };
}

test("status refresh stores what the native side reports", async () => {
  const { api, calls } = await setup();
  const status = await api.refreshLocalStatus();
  assert.deepEqual(calls.map(({ name }) => name), ["local_server_status"]);
  assert.equal(status.supported, true);
  assert.equal(api.local.status.target, "aarch64-apple-darwin");
});

test("unsupported platforms are reported, not hidden", async () => {
  const { api } = await setup({ status: { supported: false, target: null, home: "", binary_installed: false, config_exists: false, gateway_running: false, port_in_use: false } });
  await api.refreshLocalStatus();
  assert.equal(api.local.status.supported, false);
  assert.equal(api.canInstall(), false);
});

test("status failure is a safe message and leaves status null", async () => {
  const { api } = await setup({ fail: "local_server_status" });
  assert.equal(await api.refreshLocalStatus(), null);
  assert.equal(api.local.status, null);
  assert.ok(api.local.error);
  assert.equal(api.local.error.includes("TOP_SECRET"), false);
});

test("install walks download, prepare, start and ends ready with the url", async () => {
  const steps = [];
  const { api, calls } = await setup({
    duringInstall: async (emit) => {
      steps.push(api.local.step);
      emit("local-install-step", "downloading");
      emit("local-install-progress", { downloaded: 5, total: 10 });
      steps.push(`${api.local.step}:${api.local.progress}`);
      emit("local-install-step", "preparing");
      emit("local-install-step", "starting");
      emit("local-gateway-log", "nolune: ready http://localhost:26559");
      steps.push(api.local.step);
    },
  });
  await api.subscribeLocalEvents();
  assert.equal(await api.installLocal(), true);
  assert.equal(JSON.stringify(calls.filter((c) => c.name === "install_local_server")), JSON.stringify([{ name: "install_local_server", args: { channel: "stable" } }]));
  assert.deepEqual(steps, ["downloading", "downloading:0.5", "starting"]);
  assert.equal(api.local.step, "ready");
  assert.equal(api.local.url, "http://localhost:26559");
  assert.equal(api.local.version, "v0.35.0");
  assert.equal(api.local.error, null);
  assert.equal(JSON.stringify(api.local.logs), JSON.stringify(["nolune: ready http://localhost:26559"]));
});

test("the nightly toggle selects the nightly channel", async () => {
  const { api, calls } = await setup();
  api.local.nightly = true;
  await api.installLocal();
  assert.equal(JSON.stringify(calls.at(-1)), JSON.stringify({ name: "install_local_server", args: { channel: "nightly" } }));
});

test("install failure keeps the native message, expands the logs, and allows retry", async () => {
  const { api } = await setup({ fail: "install_local_server", failMessage: "port 26559 is already in use" });
  await api.subscribeLocalEvents();
  assert.equal(await api.installLocal(), false);
  assert.equal(api.local.step, "error");
  assert.equal(api.local.error, "port 26559 is already in use");
  assert.equal(api.local.showLogs, true);
  assert.equal(api.canInstall(), true, "retry must be possible after a failure");
});

test("a second install cannot start while one is running", async () => {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const { api, calls } = await setup({ duringInstall: () => gate });
  const first = api.installLocal();
  assert.equal(await api.installLocal(), false);
  release();
  assert.equal(await first, true);
  assert.equal(calls.filter((c) => c.name === "install_local_server").length, 1);
});

test("gateway log lines are appended and capped", async () => {
  const { api, emit } = await setup();
  await api.subscribeLocalEvents();
  for (let i = 0; i < 600; i++) emit("local-gateway-log", `line ${i}`);
  assert.equal(api.local.logs.length, 500);
  assert.equal(api.local.logs[0], "line 100");
  assert.equal(api.local.logs.at(-1), "line 599");
});

test("toggling logs flips visibility", async () => {
  const { api } = await setup();
  assert.equal(api.local.showLogs, false);
  api.toggleLogs();
  assert.equal(api.local.showLogs, true);
});

test("relaunch starts the app-managed gateway through one native command", async () => {
  const { api, calls } = await setup();
  assert.equal(await api.startLocalGateway(), true);
  assert.equal(JSON.stringify(calls), JSON.stringify([{ name: "start_local_gateway", args: undefined }]));
  assert.equal(api.local.step, "ready");
  assert.equal(api.local.url, "http://localhost:26559");
});

test("relaunch failure is reported without native detail", async () => {
  const { api } = await setup({ fail: "start_local_gateway" });
  assert.equal(await api.startLocalGateway(), false);
  assert.equal(api.local.step, "error");
  assert.equal(api.local.error.includes("TOP_SECRET"), false);
});
