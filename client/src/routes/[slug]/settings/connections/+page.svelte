<script lang="ts">
	import { page } from "$app/state";
	import { embeddingStatusText } from "$lib/embedding-status.js";
	import {
		fetchConfigStatus,
		updateProvider,
		updateLlmConfig,
		fetchPairedDevices,
		createPairingCode,
		revokePairedDevice,
		logoutSession,
		type EmbeddingStatus,
		type PairedDevice,
		type PairingCode,
		type AuthKind,
	} from "$lib/api/client.js";
	import ConnectedComputers from "$lib/components/computers/ConnectedComputers.svelte";

	// Connections (#98): what this server talks to. The provider and keys are
	// server-global; computers and browsers are the places the companion is.
	const slug = $derived(page.params.slug!);

	// --- provider + API keys ---
	const apiKeyDefs = [
		{ id: "api_key", name: "Anthropic", hint: "sk-ant-...", required: false, configKey: "anthropic" },
		{ id: "openai", name: "OpenAI", hint: "Chat + semantic memory (independent of chat provider)", required: false, configKey: "openai" },
		{ id: "elevenlabs", name: "ElevenLabs", hint: "Text-to-speech voice", required: false, configKey: "elevenlabs" },
	];
	let embeddingStatus = $state<EmbeddingStatus | undefined>(undefined);
	let provider = $state("anthropic");
	let setupRequired = $state<string | null>(null);
	let providerSaving = $state(false);
	let configuredKeys = $state<string[]>([]);
	let keySaving = $state("");
	let keyError = $state("");
	let keyEditing = $state("");
	let keyEditValue = $state("");

	$effect(() => {
		fetchConfigStatus().then((s) => {
			if (s.configured_keys) configuredKeys = s.configured_keys;
			if (s.provider) provider = s.provider === "api" ? "anthropic" : s.provider;
			setupRequired = s.setup_required ?? null;
			embeddingStatus = s.embedding;
		}).catch(() => {});
	});

	async function setProvider(p: "anthropic" | "openai") {
		providerSaving = true;
		try {
			await updateProvider(p);
			provider = p;
			setupRequired = (await fetchConfigStatus()).setup_required ?? null;
		} catch {
			// keep the previous provider
		} finally {
			providerSaving = false;
		}
	}

	async function saveKey(field: string, value: string) {
		keySaving = field;
		keyError = "";
		try {
			await updateLlmConfig({ [field]: value.trim() });
			const s = await fetchConfigStatus();
			setupRequired = s.setup_required ?? null;
			embeddingStatus = s.embedding;
			if (s.configured_keys) configuredKeys = s.configured_keys;
		} catch (e) {
			keyError = e instanceof Error ? e.message : "failed";
		} finally {
			keySaving = "";
		}
	}

	// --- paired browsers (#112) ---
	let sessionAuth = $state<AuthKind>("disabled");
	let devices = $state<PairedDevice[]>([]);
	let devicesLoading = $state(true);
	let devicesError = $state("");
	let pairingCode = $state<PairingCode | null>(null);
	let pairingBusy = $state(false);
	let pairingSecondsLeft = $state(0);
	let pairingTimer: ReturnType<typeof setInterval> | null = null;
	let revokingId = $state("");

	async function loadDevices() {
		devicesLoading = true;
		devicesError = "";
		try {
			const res = await fetchPairedDevices();
			sessionAuth = res.auth;
			devices = res.devices;
		} catch {
			devicesError = "Could not load paired browsers.";
		} finally {
			devicesLoading = false;
		}
	}

	async function startPairing() {
		pairingBusy = true;
		devicesError = "";
		try {
			pairingCode = await createPairingCode();
			pairingSecondsLeft = pairingCode.expires_in_secs;
			if (pairingTimer) clearInterval(pairingTimer);
			pairingTimer = setInterval(() => {
				pairingSecondsLeft = Math.max(0, pairingSecondsLeft - 1);
				if (pairingSecondsLeft === 0) dismissPairingCode();
			}, 1000);
		} catch {
			devicesError = "Could not create a pairing code.";
		} finally {
			pairingBusy = false;
		}
	}

	function dismissPairingCode() {
		pairingCode = null;
		pairingSecondsLeft = 0;
		if (pairingTimer) {
			clearInterval(pairingTimer);
			pairingTimer = null;
		}
		// A code that was used shows up as a new device.
		loadDevices();
	}

	async function revokeDevice(device: PairedDevice) {
		revokingId = device.id;
		devicesError = "";
		try {
			await revokePairedDevice(device.id);
			if (device.current) {
				location.href = "/";
				return;
			}
			devices = devices.filter((d) => d.id !== device.id);
		} catch {
			devicesError = "Could not revoke that browser.";
		} finally {
			revokingId = "";
		}
	}

	async function signOut() {
		try {
			await logoutSession();
		} finally {
			location.href = "/";
		}
	}

	function timeAgo(unixSeconds: number): string {
		const delta = Math.max(0, Math.floor(Date.now() / 1000) - unixSeconds);
		if (delta < 90) return "just now";
		if (delta < 3600) return `${Math.round(delta / 60)} min ago`;
		if (delta < 86400 * 2) return `${Math.round(delta / 3600)} h ago`;
		return `${Math.round(delta / 86400)} days ago`;
	}

	function pairingCountdown(seconds: number): string {
		const m = Math.floor(seconds / 60);
		const s = seconds % 60;
		return `${m}:${String(s).padStart(2, "0")}`;
	}

	function pairedViaText(device: PairedDevice): string {
		if (device.paired_via === "cli") return "paired from the command line";
		if (device.paired_via.startsWith("browser:")) return "paired from another browser";
		if (device.paired_via === "desktop") return "paired from the desktop app";
		return "paired with the API token";
	}

	$effect(() => {
		loadDevices();
		return () => {
			if (pairingTimer) clearInterval(pairingTimer);
		};
	});
</script>

<!-- Provider -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Provider</h3>
			<p class="section-desc">Choose which AI powers your companion.</p>
		</div>
	</div>
	<div class="section-body">
		{#if setupRequired}<p class="section-desc">{setupRequired}</p>{/if}
		<div class="model-mode-options" class:disabled={providerSaving}>
			<button class="mode-option" class:mode-active={provider === "anthropic"} onclick={() => setProvider("anthropic")} disabled={providerSaving}>
				<span class="mode-name">Anthropic</span>
				<span class="mode-desc">Pay-per-use with your own Anthropic API key</span>
			</button>
			<button class="mode-option" class:mode-active={provider === "openai"} onclick={() => setProvider("openai")} disabled={providerSaving}>
				<span class="mode-name">OpenAI</span>
				<span class="mode-desc">Pay-per-use with your own OpenAI API key</span>
			</button>
	</div>
	</div>
</section>

<!-- API keys -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">API keys</h3>
			<p class="section-desc">Your own keys, stored on this server. The chat provider needs one; the others unlock semantic memory and voice.</p>
		</div>
	</div>
	<div class="section-body">
		<p class="section-desc">{embeddingStatusText(embeddingStatus)}</p>
		<div class="keys-list">
			{#each apiKeyDefs as key (key.id)}
				{@const configured = configuredKeys.includes(key.configKey)}
				<div class="key-row">
					<div class="key-info">
						<span class="key-name">{key.name}{key.required ? " *" : ""}</span>
						<span class="key-hint">{key.hint}</span>
					</div>
					<div class="key-action">
						{#if keyEditing === key.id}
							<input
								class="key-input"
								aria-label={`${key.name} key`}
								type="password"
								placeholder="{key.name} key..."
								bind:value={keyEditValue}
								onkeydown={(e) => {
									if (e.key === "Enter" && keyEditValue.trim()) {
										saveKey(key.id, keyEditValue);
										keyEditing = "";
										keyEditValue = "";
									}
									if (e.key === "Escape") { keyEditing = ""; keyEditValue = ""; }
								}}
							/>
							<button
								class="key-change"
								onclick={() => {
									if (keyEditValue.trim()) saveKey(key.id, keyEditValue);
									keyEditing = "";
									keyEditValue = "";
								}}
								disabled={keySaving === key.id}
							>{keySaving === key.id ? "Saving..." : "Save"}</button>
							<button class="key-change" onclick={() => { keyEditing = ""; keyEditValue = ""; }}>Cancel</button>
						{:else if configured}
							<span class="key-badge key-badge-ok">Connected</span>
							<button class="key-change" onclick={() => { keyEditing = key.id; keyEditValue = ""; }} disabled={keySaving === key.id}>Change</button>
							<button class="key-change key-change-remove" onclick={() => saveKey(key.id, "")} disabled={keySaving === key.id}>Remove</button>
						{:else}
							<button class="key-change key-change-add" onclick={() => { keyEditing = key.id; keyEditValue = ""; }} disabled={keySaving === key.id}>{keySaving === key.id ? "Saving..." : "Add key"}</button>
						{/if}
					</div>
				</div>
			{/each}
	</div>
	{#if keyError}
		<p class="key-error" role="alert">{keyError}</p>
	{/if}
	<p class="setting-hint" style="margin-top: 12px;">Changing the OpenAI key restarts semantic indexing on the next server start. Endpoint details live under <a class="settings-link" href={`/${slug}/settings/advanced`}>Advanced</a>.</p>
	</div>
</section>

<!-- Connected computers -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Computers</h3>
			<p class="section-desc">Desktops running the Nolune app that your companion can see and act on.</p>
		</div>
	</div>
	<div class="section-body">
		{#key slug}
			<ConnectedComputers {slug} compact />
		{/key}
		<div class="settings-links"><a class="nl-button-secondary" href={`/${slug}/computers`}>Open computers</a></div>
	</div>
</section>

<!-- Paired browsers -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Paired browsers</h3>
			<p class="section-desc">Browsers that stay signed in to this server.</p>
		</div>
	</div>
	<div class="section-body">
		{#if devicesLoading}
			<p class="dim-text">Loading...</p>
		{:else if sessionAuth === "disabled"}
			<p class="setting-hint">This server accepts any browser. Set an API token under <a class="settings-link" href={`/${slug}/settings/advanced`}>Advanced</a> to require pairing.</p>
		{:else}
			{#if devices.length === 0}
				<p class="setting-hint">No browsers are paired yet.</p>
			{:else}
				<ul class="device-list">
					{#each devices as device (device.id)}
						<li class="device-row">
							<div class="device-info">
								<span class="device-label">
									{device.label}
									{#if device.current}<span class="device-current">This browser</span>{/if}
								</span>
								<span class="device-meta">{device.host} · {pairedViaText(device)} · last seen {timeAgo(device.last_seen_at)}</span>
							</div>
							<button class="setting-btn setting-btn-danger" onclick={() => revokeDevice(device)} disabled={revokingId === device.id}>
								{device.current ? "Sign out" : "Revoke"}
							</button>
						</li>
					{/each}
				</ul>
			{/if}
			{#if pairingCode}
				<div class="pairing-panel" aria-live="polite">
					<span class="pairing-code">{pairingCode.code}</span>
					<p class="setting-hint">
						Enter this code on the new device within {pairingCountdown(pairingSecondsLeft)}. It works once.
						{#if pairingCode.bound_host}Open Nolune there at the same address, <strong>{pairingCode.bound_host}</strong>.{/if}
					</p>
					<button class="setting-btn" onclick={dismissPairingCode}>Done</button>
				</div>
			{:else}
				<div class="setting-input-row">
					<button class="setting-btn" onclick={startPairing} disabled={pairingBusy}>
						{pairingBusy ? "..." : "Pair another browser"}
					</button>
					{#if sessionAuth === "session" && devices.length === 0}
						<button class="setting-btn setting-btn-danger" onclick={signOut}>Sign out</button>
					{/if}
				</div>
			{/if}
			{#if devicesError}
				<p class="setting-hint setting-warning">{devicesError}</p>
			{/if}
		{/if}
	</div>
</section>
