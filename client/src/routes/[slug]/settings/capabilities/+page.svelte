<script lang="ts">
	import {
		fetchMcpServers,
		addMcpServer,
		removeMcpServer,
		updateMcpToolGrants,
		fetchSuggestedMcp,
		type McpServerInfo,
	} from "$lib/api/client.js";
	import { grantSummary, trustLabel } from "$lib/extensions/trust.js";
	import SkillsView from "$lib/components/skills/SkillsView.svelte";

	// Capabilities (#98): what the companion may use. Skills and extensions are
	// server-global; custom, unreviewed servers stay behind Advanced (#97).

	interface SuggestedMcp {
		name: string;
		description: string;
		url: string;
		requires_key: boolean;
		key_env: string;
		key_url: string;
		installed: boolean;
	}
	let suggestedMcp = $state<SuggestedMcp[]>([]);

	let mcpServers = $state<McpServerInfo[]>([]);
	let mcpLoading = $state(true);
	let mcpBusy = $state<string | null>(null);
	let mcpError = $state("");
	let mcpNewName = $state("");
	let mcpNewUrl = $state("");
	let showCustomForm = $state(false);
	let acknowledgeUntrusted = $state(false);
	let grantBusy = $state<string | null>(null);

	// Custom servers = installed servers that aren't in the catalog
	let customServers = $derived(
		mcpServers.filter((s) => !suggestedMcp.some((c) => c.name === s.name)),
	);

	function isInstalled(name: string): boolean {
		return mcpServers.some((s) => s.name === name);
	}

	function isConnected(name: string): boolean {
		return mcpServers.some((s) => s.name === name && s.connected);
	}

	async function loadMcpServers() {
		mcpLoading = true;
		mcpError = "";
		try {
			[mcpServers, suggestedMcp] = await Promise.all([
				fetchMcpServers(),
				fetchSuggestedMcp(),
			]);
		} catch (e) {
			console.error("Failed to load MCP servers:", e);
		} finally {
			mcpLoading = false;
		}
	}

	async function addSuggested(entry: SuggestedMcp) {
		if (entry.requires_key) {
			const key = prompt(`${entry.name} API key:\n\nGet one at ${entry.key_url}`);
			if (!key) return;
		}
		mcpBusy = entry.name;
		try {
			await addMcpServer(entry.name, entry.url);
			await loadMcpServers();
		} catch (e) {
			mcpError = e instanceof Error ? e.message : "failed";
		} finally {
			mcpBusy = null;
		}
	}

	async function toggleGrant(server: McpServerInfo, tool: string, enabled: boolean) {
		grantBusy = `${server.name}/${tool}`;
		mcpError = "";
		try {
			const next = enabled
				? [...server.tools.filter((t) => t.enabled).map((t) => t.name), tool]
				: server.tools.filter((t) => t.enabled && t.name !== tool).map((t) => t.name);
			const updated = await updateMcpToolGrants(server.name, next);
			mcpServers = mcpServers.map((s) => (s.name === updated.name ? updated : s));
		} catch (e) {
			mcpError = e instanceof Error ? e.message : "could not update tool access";
		} finally {
			grantBusy = null;
		}
	}

	async function handleAddCustom() {
		const name = mcpNewName.trim();
		const url = mcpNewUrl.trim();
		if (!name || !url) {
			mcpError = "name and url are required";
			return;
		}
		if (!acknowledgeUntrusted) {
			mcpError = "acknowledge that this server is not reviewed before adding it";
			return;
		}
		mcpBusy = name;
		mcpError = "";
		try {
			await addMcpServer(name, url, true);
			mcpNewName = "";
			mcpNewUrl = "";
			acknowledgeUntrusted = false;
			showCustomForm = false;
			await loadMcpServers();
		} catch (e: any) {
			mcpError = e?.message || "failed to add server";
		} finally {
			mcpBusy = null;
		}
	}

	async function handleRemoveCustom(name: string) {
		mcpBusy = name;
		mcpError = "";
		try {
			await removeMcpServer(name);
			mcpServers = mcpServers.filter((s) => s.name !== name);
		} catch {
			mcpError = `failed to remove ${name}`;
		} finally {
			mcpBusy = null;
		}
	}

	$effect(() => {
		loadMcpServers();
	});
</script>

<!-- Skills -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Skills</h3>
			<p class="section-desc">Reviewed bundles from the registry that teach your companion how to do specific things.</p>
		</div>
	</div>
	<div class="section-body">
		<SkillsView embedded />
	</div>
</section>

<!-- Extensions (MCP servers) -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Extensions</h3>
			<p class="section-desc">Reviewed integrations your companion can use. Each one lists exactly which tools are allowed in chat.</p>
		</div>
	</div>
	<div class="section-body">

		{#if mcpLoading}
			<div class="ext-loading"><div class="loading-dot"></div></div>
		{:else}
			<!-- Suggested servers -->
			{#if suggestedMcp.length > 0}
				<div class="keys-list">
					{#each suggestedMcp as entry (entry.name)}
						{@const installed = isInstalled(entry.name)}
						{@const connected = isConnected(entry.name)}
						{@const busy = mcpBusy === entry.name}
						<div class="key-row">
							<div class="key-info">
								<span class="key-name">{entry.name}</span>
								<span class="key-hint">{entry.description}</span>
							</div>
							<div class="key-action">
								{#if installed && connected}
									<span class="key-badge key-badge-ok">Connected</span>
									<button class="key-change" disabled={busy} onclick={() => handleRemoveCustom(entry.name)}>
										{busy ? "..." : "Remove"}
									</button>
								{:else if installed}
									<span class="key-badge" style="background: color-mix(in srgb, var(--primary) 12%, transparent); color: var(--primary);">Reconnecting</span>
								{:else}
									<button class="key-change key-change-add" disabled={busy} onclick={() => addSuggested(entry)}>
										{busy ? "Connecting..." : "Add"}
									</button>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			{/if}

			<!-- Tool grants: exactly which tools each connected server may offer in chat -->
			{#each mcpServers as server (server.name)}
				<div class="ext-grants" aria-label={`Tools allowed for ${server.name}`}>
					<div class="ext-grants-head">
						<span class="key-name">{server.name}</span>
						<span class="key-badge" class:key-badge-ok={server.trust === "curated"}>{trustLabel(server)}</span>
						<span class="key-hint">{grantSummary(server)}</span>
					</div>
					{#if server.connected && server.tools.length > 0}
						<ul class="ext-tool-list">
							{#each server.tools as tool (tool.name)}
								<li>
									<label class="ext-tool">
										<input type="checkbox" checked={tool.enabled} disabled={grantBusy === `${server.name}/${tool.name}`} onchange={(e) => toggleGrant(server, tool.name, (e.currentTarget as HTMLInputElement).checked)} />
										<span class="ext-tool-name">{tool.name}</span>
										{#if tool.description}<span class="key-hint">{tool.description}</span>{/if}
									</label>
								</li>
							{/each}
						</ul>
					{:else if !server.connected}
						<p class="key-hint">Not connected. Tools can be allowed once it connects.</p>
					{/if}
				</div>
			{/each}

			<!-- Custom/user-added servers -->
			{#if customServers.length > 0}
				<div class="keys-list" style="margin-top: 12px;">
					{#each customServers as server (server.name)}
						<div class="key-row">
							<div class="key-info">
								<span class="key-name">{server.name}</span>
								<span class="key-hint">{server.url ?? "local process"} · {trustLabel(server)}</span>
							</div>
							<div class="key-action">
								<span class="key-badge" class:key-badge-ok={server.connected} style={server.connected ? "" : "background: var(--accent); color: var(--destructive);"}>
									{server.connected ? "Connected" : "Disconnected"}
								</span>
								<button class="key-change" disabled={mcpBusy === server.name} onclick={() => handleRemoveCustom(server.name)}>
									{mcpBusy === server.name ? "..." : "Remove"}
								</button>
							</div>
						</div>
					{/each}
				</div>
			{/if}

			<!-- Advanced: unreviewed servers need an explicit acknowledgement (#97) -->
			<details class="ext-advanced" style="margin-top: 12px;" bind:open={showCustomForm}>
				<summary class="ext-advanced-toggle">Advanced: connect a custom MCP server</summary>
				<div class="ext-custom-form">
					<p class="key-hint">Custom servers are not reviewed by Nolune. Their tools can read what you send them and act on your behalf. After connecting, no tool is allowed in chat until you enable it above. Local command servers are configured in <code>config.toml</code>.</p>
					<label class="integration-field">Server name<input class="ext-input" type="text" placeholder="name" bind:value={mcpNewName} /></label>
					<label class="integration-field">Server URL<input class="ext-input" type="url" placeholder="https://…/mcp" bind:value={mcpNewUrl} /></label>
					<label class="ext-ack"><input type="checkbox" bind:checked={acknowledgeUntrusted} /> I understand this server is not reviewed and I have checked where it comes from.</label>
					<div class="ext-form-actions">
						<button class="ext-form-btn ext-form-add" disabled={mcpBusy !== null || !acknowledgeUntrusted} onclick={handleAddCustom}>
							{mcpBusy ? "Connecting..." : "Add"}
						</button>
						<button class="ext-form-btn ext-form-cancel" onclick={() => { showCustomForm = false; mcpError = ""; }}>
							Cancel
						</button>
					</div>
				</div>
			</details>
		{/if}

		{#if mcpError}
			<p class="key-error" role="alert">{mcpError}</p>
		{/if}
	</div>
</section>
