<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { local, background, canToggleBackground, refreshLocalStatus, refreshBackgroundStatus, setBackgroundService } from "$lib/local.svelte";

  type Permissions = {
    screen_recording: boolean;
    accessibility: boolean;
  };

  let permissions = $state<Permissions | null>(null);
  let checking = $state(false);
  let error = $state<string | null>(null);

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

  async function refresh() {
    checking = true;
    error = null;
    try {
      permissions = await invoke<Permissions>("check_permissions");
    } catch (e) {
      console.error("check_permissions failed", e);
      error = "Could not read permission status. Retry, or check System Settings directly.";
    } finally {
      checking = false;
    }
  }

  async function openSettings(permission: string) {
    await invoke("open_permission_settings", { permission });
    setTimeout(refresh, 3000);
  }

  const items = $derived([
    {
      key: "screen_recording",
      name: "Screen recording",
      desc: "Take screenshots of your screen",
      granted: permissions?.screen_recording ?? false,
    },
    {
      key: "accessibility",
      name: "Accessibility",
      desc: "Control the mouse and keyboard",
      granted: permissions?.accessibility ?? false,
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
      <p class="nl-eyebrow">macOS</p>
      <h2 id="permissions-title" class="section-title">Permissions</h2>
      <p class="section-desc">
        Nolune needs these permissions to control your computer when you ask it to.
      </p>

      {#if error}
        <p class="section-error" role="alert">{error}</p>
      {/if}

      {#if permissions}
        <ul class="perm-list">
          {#each items as item (item.key)}
            <li class="perm-row">
              <div class="perm-icon" aria-hidden="true">
                {#if item.key === "screen_recording"}
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
                <span class="perm-name">{item.name}</span>
                <span class="perm-desc">{item.desc}</span>
              </div>
              <div class="perm-status">
                {#if item.granted}
                  <span class="badge">Granted</span>
                {:else}
                  <button class="nl-button perm-grant" onclick={() => openSettings(item.key)}>Grant</button>
                {/if}
              </div>
            </li>
          {/each}
        </ul>

        <button class="nl-button-secondary refresh" onclick={refresh} disabled={checking}>
          {checking ? "Checking…" : "Refresh status"}
        </button>
      {:else if error}
        <button class="nl-button-secondary refresh" onclick={refresh} disabled={checking}>Retry</button>
      {:else}
        <p class="loading" role="status"><span class="spinner" aria-hidden="true"></span>Checking permissions…</p>
      {/if}
    </section>
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
