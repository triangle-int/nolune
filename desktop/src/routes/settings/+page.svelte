<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { local, background, canToggleBackground, refreshLocalStatus, refreshBackgroundStatus, setBackgroundService } from "$lib/local.svelte";
  import { DRIVER_BUNDLE, grantOutcomeText, intro, pageState, permissionRows, statusLines } from "$lib/cua-permissions";

  type CuaReport = import("$lib/cua-permissions").CuaPermissionsReport;
  type GrantOutcome = import("$lib/cua-permissions").GrantOutcome;
  type PermissionKey = "accessibility" | "screen_recording";

  /** This app's own grants: reported to the companion beside the driver's, used by nothing inside the app (#19). */
  type AppPermissions = {
    screen_recording: boolean;
    accessibility: boolean;
  };

  let report = $state<CuaReport | null>(null);
  let checking = $state(false);
  let error = $state<string | null>(null);
  let grantNote = $state<string | null>(null);
  let granting = $state<PermissionKey | null>(null);
  let appPermissions = $state<AppPermissions | null>(null);
  let appError = $state<string | null>(null);

  onMount(async () => {
    refresh();
    refreshServer();
  });

  async function refreshServer() {
    await refreshLocalStatus();
    if (local.status?.binary_installed) await refreshBackgroundStatus();
  }

  const serverInstalled = $derived(!!local.status?.binary_installed && !!local.status?.config_exists);
  const inBackground = $derived(background.status?.installed === true);
  const backgroundLabel = $derived(
    background.busy ? (inBackground ? "Turning off…" : "Turning on…")
    : inBackground ? "Turn off"
    : "Turn on",
  );

  /** The driver's own report: the grants macOS gave CuaDriver, never this app. */
  async function refresh() {
    checking = true;
    error = null;
    try {
      report = await invoke<CuaReport>("cua_permissions");
    } catch (e) {
      console.error("cua_permissions failed", e);
      error = "Could not read the driver's permission status. Retry, or run `nolune cua status` in a terminal.";
    } finally {
      checking = false;
    }
    await refreshApp();
  }

  async function refreshApp() {
    appError = null;
    try {
      appPermissions = await invoke<AppPermissions>("check_permissions");
    } catch (e) {
      console.error("check_permissions failed", e);
      appError = "Could not read this app's own permission status.";
    }
  }

  /** The driver asks macOS itself, so the prompt and the pane entry name CuaDriver. */
  async function grant(permission: PermissionKey) {
    if (!report) return;
    granting = permission;
    grantNote = null;
    try {
      const outcome = await invoke<GrantOutcome>("cua_grant_permission", { permission });
      grantNote = grantOutcomeText(outcome, report);
    } catch (e) {
      grantNote = typeof e === "string" ? e : "The grant could not be started.";
    } finally {
      granting = null;
    }
    setTimeout(refresh, 4000);
  }

  async function openAppSettings(permission: PermissionKey) {
    await invoke("open_permission_settings", { permission });
    setTimeout(refreshApp, 3000);
  }

  const kind = $derived(report ? pageState(report) : null);
  const lines = $derived(report ? statusLines(report) : []);
  const rows = $derived(report ? permissionRows(report) : []);
  const lead = $derived(intro(report ?? { driver_bundle: DRIVER_BUNDLE }));
  const onMacos = $derived(report?.platform.os === "macos");

  const appItems = $derived([
    {
      key: "screen_recording" as const,
      name: "Screen recording",
      desc: "This app takes no screenshots of its own; every one-shot window snapshot is the driver's.",
      granted: appPermissions?.screen_recording ?? false,
    },
    {
      key: "accessibility" as const,
      name: "Accessibility",
      desc: "This app moves no pointer or keyboard of its own; every window action is the driver's.",
      granted: appPermissions?.accessibility ?? false,
    },
  ]);
</script>

<div class="settings">
  <header class="header">
    <h1 class="title">Settings</h1>
  </header>

  <main class="content">
    <section class="section" aria-labelledby="server-title">
      <p class="nl-eyebrow">This computer</p>
      <h2 id="server-title" class="section-title">Server</h2>
      <p class="section-desc">
        When Nolune is installed on this computer, its server normally runs only while this app is open.
      </p>

      {#if background.error}
        <p class="section-error" role="alert">{background.error}</p>
      {/if}

      {#if !local.status}
        <p class="loading" role="status"><span class="spinner" aria-hidden="true"></span>Checking the server…</p>
      {:else if !serverInstalled}
        <p class="section-desc">Nolune is not installed on this computer. Use <strong>Install on this computer</strong> in the main window first.</p>
      {:else}
        <ul class="perm-list">
          <li class="perm-row">
            <div class="perm-icon" aria-hidden="true">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                <rect x="3" y="4" width="18" height="6" rx="1.5" /><rect x="3" y="14" width="18" height="6" rx="1.5" /><path d="M7 7h.01M7 17h.01" />
              </svg>
            </div>
            <div class="perm-info">
              <span class="perm-name">Run in background</span>
              <span class="perm-desc">
                {#if background.status?.supported === false}
                  Not available on this platform yet. Start <code>nolune gateway</code> yourself to keep it running.
                {:else if inBackground}
                  Keeps running when this app is closed, and starts when you log in.
                {:else}
                  Keep the server running after you quit this app.
                {/if}
              </span>
            </div>
            <div class="perm-status">
              {#if background.status?.supported === false}
                <span class="badge badge-off">Unavailable</span>
              {:else}
                <span class="badge" class:badge-off={!inBackground}>{inBackground ? "On" : "Off"}</span>
                <button class="nl-button perm-grant" onclick={() => setBackgroundService(!inBackground)} disabled={!canToggleBackground()} aria-pressed={inBackground}>
                  {backgroundLabel}
                </button>
              {/if}
            </div>
          </li>
        </ul>
        <p class="section-hint">
          Server updates: run <code>~/.nolune/bin/update</code>, then restart the server (<code>nolune gateway restart</code> in background mode, or reopen this app otherwise). The app's own updater only updates this app.
        </p>
      {/if}
    </section>

    <section class="section" aria-labelledby="permissions-title">
      <p class="nl-eyebrow">Computer use</p>
      <h2 id="permissions-title" class="section-title">Permissions</h2>
      <p class="section-desc">{lead}</p>

      {#if error}
        <p class="section-error" role="alert">{error}</p>
      {/if}

      {#if report}
        <ul class="status-list" aria-label="Driver status">
          {#each lines as line, index (index)}
            <li class="status-line" class:status-ok={line.tone === "ok"} class:status-muted={line.tone === "muted"} class:status-error={line.tone === "error"} role={line.tone === "error" ? "alert" : undefined}>{line.text}</li>
          {/each}
        </ul>

        {#if rows.length > 0}
          <ul class="perm-list">
            {#each rows as row (row.key)}
              <li class="perm-row">
                <div class="perm-icon" aria-hidden="true">
                  {#if row.key === "screen_recording"}
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                      <rect x="2" y="3" width="20" height="14" rx="2" /><path d="M8 21h8M12 17v4" />
                    </svg>
                  {:else}
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                    </svg>
                  {/if}
                </div>
                <div class="perm-info">
                  <span class="perm-name">{row.name}</span>
                  <span class="perm-desc">{row.desc}</span>
                  {#if row.state !== "granted" && row.hint}
                    <span class="perm-hint">{row.hint}</span>
                  {/if}
                </div>
                <div class="perm-status" class:perm-status-stacked={row.canGrant}>
                  <span class="badge" class:badge-off={row.state !== "granted"}>{row.stateText}</span>
                  {#if row.canGrant}
                    <button class="nl-button perm-grant" onclick={() => grant(row.key)} disabled={granting !== null}>
                      {granting === row.key ? "Asking…" : "Grant"}
                    </button>
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
        {:else if kind === "incompatible" || kind === "absent" || kind === "unreachable"}
          <p class="section-hint">Nothing can be granted until the pinned driver reports; the lines above say what to run.</p>
        {/if}

        {#if grantNote}
          <p class="section-hint" role="status">{grantNote}</p>
        {/if}

        <button class="nl-button-secondary refresh" onclick={refresh} disabled={checking}>
          {checking ? "Checking…" : "Refresh status"}
        </button>
      {:else if error}
        <button class="nl-button-secondary refresh" onclick={refresh} disabled={checking}>Retry</button>
      {:else}
        <p class="loading" role="status"><span class="spinner" aria-hidden="true"></span>Asking the driver…</p>
      {/if}
    </section>

    {#if onMacos}
      <section class="section section-secondary" aria-labelledby="app-permissions-title">
        <h3 id="app-permissions-title" class="subsection-title">This app's own grants</h3>
        <p class="section-desc">
          What macOS has granted this app itself, reported to your companion beside the driver's. Nothing inside this app captures the screen or moves the pointer; every window action runs through the driver above.
        </p>

        {#if appError}
          <p class="section-error" role="alert">{appError}</p>
        {/if}

        {#if appPermissions}
          <ul class="perm-list">
            {#each appItems as item (item.key)}
              <li class="perm-row">
                <div class="perm-info">
                  <span class="perm-name">{item.name}</span>
                  <span class="perm-desc">{item.desc}</span>
                </div>
                <div class="perm-status">
                  {#if item.granted}
                    <span class="badge">Granted</span>
                  {:else}
                    <button class="nl-button-secondary perm-grant" onclick={() => openAppSettings(item.key)}>Open System Settings</button>
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
  </main>
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--background);
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px 24px;
    -webkit-app-region: drag;
  }

  .title {
    font: 400 18px/1.2 var(--font-display);
    color: var(--foreground);
    margin: 0;
  }

  .content {
    flex: 1;
    padding: 8px 24px 32px;
    overflow-y: auto;
  }

  .section {
    max-width: 480px;
    margin: 0 auto;
  }

  .section-title {
    font: 400 28px/1.15 var(--font-display);
    letter-spacing: -0.02em;
    color: var(--foreground);
    margin: 8px 0 8px;
  }

  .section-desc {
    font-size: 14px;
    line-height: 1.6;
    color: var(--text-secondary);
    margin: 0 0 24px;
  }

  .section-error {
    font-size: 14px;
    line-height: 1.5;
    color: var(--destructive);
    margin: 0 0 16px;
  }

  .perm-list {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    background: var(--card);
    overflow: hidden;
  }

  .perm-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 16px;
  }

  .perm-row + .perm-row {
    border-top: 1px solid var(--border);
  }

  .perm-icon {
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-control);
    background: var(--accent);
    color: var(--accent-foreground);
    flex-shrink: 0;
  }

  .perm-info {
    flex: 1;
    min-width: 0;
  }

  .perm-name {
    display: block;
    font-size: 14px;
    font-weight: 500;
    color: var(--foreground);
  }

  .perm-desc {
    display: block;
    font-size: 13px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .perm-status {
    flex-shrink: 0;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-foreground);
    font: 500 13px/1.5 var(--font-body);
  }

  .badge::before {
    content: "";
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--primary);
  }

  .perm-grant {
    min-height: 36px;
    padding: 6px 14px;
  }

  .perm-status {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .perm-status-stacked {
    flex-direction: column;
    align-items: flex-end;
    gap: 6px;
  }

  .badge-off {
    background: var(--card);
    color: var(--text-muted);
  }

  .badge-off::before {
    background: var(--border);
  }

  .section-hint {
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-muted);
    margin: 0 0 32px;
  }

  .section-secondary {
    margin-top: 32px;
  }

  .subsection-title {
    font: 500 16px/1.3 var(--font-body);
    color: var(--foreground);
    margin: 0 0 6px;
  }

  .status-list {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .status-line {
    font-size: 13px;
    line-height: 1.6;
    padding-left: 16px;
    position: relative;
    overflow-wrap: anywhere;
  }

  .status-line::before {
    content: "";
    position: absolute;
    left: 0;
    top: 8px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--border);
  }

  .status-ok {
    color: var(--text-secondary);
  }

  .status-ok::before {
    background: var(--primary);
  }

  .status-muted {
    color: var(--text-muted);
  }

  .status-error {
    color: var(--destructive);
  }

  .status-error::before {
    background: var(--destructive);
  }

  .perm-hint {
    display: block;
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 4px;
  }

  .section-hint code,
  .perm-desc code {
    font: 400 12px/1.5 var(--font-mono);
    color: var(--text-secondary);
  }

  .refresh {
    display: flex;
    margin: 0 auto;
  }

  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
    margin: 0;
    font-size: 14px;
    color: var(--text-muted);
  }

  .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid var(--border);
    border-top-color: var(--primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
