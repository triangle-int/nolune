<script lang="ts">
	import { page } from "$app/state";
	import { embeddingStatusText } from "$lib/embedding-status.js";
	import {
		fetchConfigStatus,
		updateLlmConfig,
		fetchModelPresets,
		updateModelPresets,
		seedModelPresets,
		type ModelPreset,
		type ModelPresets,
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
	import Companions from "$lib/components/federation/Companions.svelte";
	import { PROVIDERS, suggestPresetId, validatePresets } from "$lib/models/presets.js";

	// Connections (#98): what this server talks to. The provider and keys are
	// server-global; computers and browsers are the places the companion is.
	const slug = $derived(page.params.slug!);

	// --- model presets (#156) + API keys ---
	const apiKeyDefs = [
		{ id: "api_key", name: "Anthropic", hint: "sk-ant-...", required: false, configKey: "anthropic" },
		{ id: "openai", name: "OpenAI", hint: "Chat + semantic memory (independent of chat provider)", required: false, configKey: "openai" },
		{ id: "openrouter", name: "OpenRouter", hint: "One key for many vendors; models are named vendor/model", required: false, configKey: "openrouter" },
		{ id: "elevenlabs", name: "ElevenLabs", hint: "Text-to-speech voice", required: false, configKey: "elevenlabs" },
	];
	let embeddingStatus = $state<EmbeddingStatus | undefined>(undefined);
	let setupRequired = $state<string | null>(null);
	let configuredKeys = $state<string[]>([]);
	let keySaving = $state("");
	let keyError = $state("");
	let keyEditing = $state("");
	let keyEditValue = $state("");

	$effect(() => {
		fetchConfigStatus().then((s) => {
			if (s.configured_keys) configuredKeys = s.configured_keys;
			setupRequired = s.setup_required ?? null;
			embeddingStatus = s.embedding;
		}).catch(() => {});
	});

	type Draft = { presets: ModelPreset[]; chat_preset: string; background_preset: string };
	let saved = $state<ModelPresets | null>(null);
	let draft = $state<Draft>({ presets: [], chat_preset: "", background_preset: "" });
	let modelsLoading = $state(true);
	let modelsSaving = $state(false);
	let modelsSaved = $state(false);
	let modelsError = $state("");
	/** Presets added in this session; their id follows the name until saved. */
	let freshIds = $state<Set<string>>(new Set());

	const modelErrors = $derived(saved ? validatePresets(draft.presets, draft, saved.keyed_providers) : []);
	const modelsDirty = $derived(
		!!saved && JSON.stringify(draft) !== JSON.stringify({ presets: saved.presets, chat_preset: saved.chat_preset, background_preset: saved.background_preset }),
	);

	function applyModels(models: ModelPresets) {
		saved = models;
		draft = { presets: models.presets.map((p) => ({ ...p })), chat_preset: models.chat_preset, background_preset: models.background_preset };
		freshIds = new Set();
	}

	async function loadModels() {
		modelsLoading = true;
		modelsError = "";
		try {
			applyModels(await fetchModelPresets());
		} catch {
			modelsError = "Could not load model presets.";
		} finally {
			modelsLoading = false;
		}
	}

	function addPreset(provider: ModelPreset["provider"] = "anthropic") {
		const ids = draft.presets.map((p) => p.id);
		const id = suggestPresetId("new preset", ids);
		draft.presets = [...draft.presets, { id, name: "", provider, model: "" }];
		freshIds = new Set([...freshIds, id]);
	}

	function renamePreset(index: number, name: string) {
		const preset = draft.presets[index];
		const wasFresh = freshIds.has(preset.id);
		const next = { ...preset, name };
		if (wasFresh) {
			const others = draft.presets.filter((_, i) => i !== index).map((p) => p.id);
			const oldId = preset.id;
			next.id = suggestPresetId(name || "new preset", others);
			freshIds = new Set([...[...freshIds].filter((f) => f !== oldId), next.id]);
			if (draft.chat_preset === oldId) draft.chat_preset = next.id;
			if (draft.background_preset === oldId) draft.background_preset = next.id;
		}
		draft.presets = draft.presets.map((p, i) => (i === index ? next : p));
	}

	function removePreset(index: number) {
		const id = draft.presets[index].id;
		draft.presets = draft.presets.filter((_, i) => i !== index);
		if (draft.chat_preset === id) draft.chat_preset = draft.presets[0]?.id ?? "";
		if (draft.background_preset === id) draft.background_preset = draft.presets[0]?.id ?? "";
	}

	async function saveModels() {
		if (modelsSaving || modelErrors.length > 0) return;
		modelsSaving = true;
		modelsError = "";
		try {
			applyModels(await updateModelPresets(draft));
			modelsSaved = true;
			setTimeout(() => (modelsSaved = false), 3000);
			setupRequired = (await fetchConfigStatus()).setup_required ?? null;
		} catch (e) {
			modelsError = e instanceof Error ? e.message : "Could not save model presets.";
		} finally {
			modelsSaving = false;
		}
	}

	async function seedDefaults(provider: ModelPreset["provider"]) {
		modelsSaving = true;
		modelsError = "";
		try {
			applyModels(await seedModelPresets(provider));
			setupRequired = (await fetchConfigStatus()).setup_required ?? null;
		} catch (e) {
			modelsError = e instanceof Error ? e.message : "Could not add default presets.";
		} finally {
			modelsSaving = false;
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
			await loadModels();
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
		loadModels();
		return () => {
			if (pairingTimer) clearInterval(pairingTimer);
		};
	});
</script>

<!-- Model presets (#156) -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Models</h3>
			<p class="section-desc">Presets name the models your companion may use. Pick one for conversations and one for background work; any chat can switch to another preset from its composer.</p>
		</div>
	</div>
	<div class="section-body">
	{#if setupRequired}<p class="setting-hint setting-warning">{setupRequired}</p>{/if}

	{#if modelsLoading}
		<p class="dim-text">Loading...</p>
	{:else}
		{#if draft.presets.length > 0}
			<div class="setting-row">
				<label class="setting-label" for="chat-preset-slot">Chat</label>
				<select id="chat-preset-slot" class="setting-input" bind:value={draft.chat_preset} disabled={modelsSaving}>
					{#each draft.presets as preset (preset.id)}
						<option value={preset.id}>{preset.name || "(unnamed)"} · {preset.model || "no model"}</option>
					{/each}
				</select>
				<p class="setting-hint">Used for conversations unless a chat picks another preset.</p>
			</div>
			<div class="setting-row">
				<label class="setting-label" for="background-preset-slot">Background</label>
				<select id="background-preset-slot" class="setting-input" bind:value={draft.background_preset} disabled={modelsSaving}>
					{#each draft.presets as preset (preset.id)}
						<option value={preset.id}>{preset.name || "(unnamed)"} · {preset.model || "no model"}</option>
					{/each}
				</select>
				<p class="setting-hint">Memory extraction, chat titles, check-ins, and reflection. Never the chat preset unless you choose it here.</p>
			</div>
		{/if}

		<div class="setting-row">
			<span class="setting-label" id="presets-label">Presets</span>
			{#if draft.presets.length === 0}
				<p class="setting-hint">No presets yet. Add your provider's defaults or create one.</p>
			{:else}
				<ul class="preset-list" aria-labelledby="presets-label">
					{#each draft.presets as preset, index (preset.id)}
						{@const inUse = draft.chat_preset === preset.id || draft.background_preset === preset.id}
						<li class="preset-row">
							<label class="preset-field">Name<input class="ext-input" type="text" placeholder="Claude Sonnet" value={preset.name} oninput={(e) => renamePreset(index, (e.currentTarget as HTMLInputElement).value)} disabled={modelsSaving} /></label>
							<label class="preset-field">Provider<select class="setting-input" bind:value={preset.provider} disabled={modelsSaving}>{#each PROVIDERS as provider (provider.id)}<option value={provider.id}>{provider.label}</option>{/each}</select></label>
							<label class="preset-field preset-field-model">Model id<input class="ext-input" type="text" placeholder={preset.provider === "openrouter" ? "vendor/model" : "claude-sonnet-4-6"} bind:value={preset.model} disabled={modelsSaving} spellcheck="false" /></label>
							<button class="setting-btn setting-btn-danger preset-remove" onclick={() => removePreset(index)} disabled={modelsSaving} title={inUse ? "In use by a slot; the slot moves to the first preset" : "Remove preset"}>Remove</button>
						</li>
					{/each}
				</ul>
			{/if}
			<div class="settings-links">
				<button class="nl-button-secondary" onclick={() => addPreset()} disabled={modelsSaving}>Add preset</button>
				{#each PROVIDERS as provider (provider.id)}
					<button class="nl-button-secondary" onclick={() => seedDefaults(provider.id as ModelPreset["provider"])} disabled={modelsSaving}>Add {provider.label} defaults</button>
				{/each}
			</div>
		</div>

		{#if modelErrors.length > 0}
			<ul class="key-error preset-errors" role="alert">
				{#each modelErrors as error (error)}<li>{error}</li>{/each}
			</ul>
		{/if}
		{#if modelsError}<p class="key-error" role="alert">{modelsError}</p>{/if}
		<div class="setting-input-row" style="margin-top: 12px;">
			<button class="setting-btn" onclick={saveModels} disabled={!modelsDirty || modelsSaving || modelErrors.length > 0}>
				{modelsSaving ? "Saving..." : "Save models"}
			</button>
			{#if modelsSaved}<span class="dim-text" role="status">Saved</span>{/if}
			{#if modelsDirty && !modelsSaved}<span class="dim-text">Unsaved changes</span>{/if}
		</div>
	{/if}
	</div>
</section>

<!-- API keys -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">API keys</h3>
			<p class="section-desc">Your own keys, stored on this server. A preset can only be used once its provider has a key; OpenAI also unlocks semantic memory, OpenRouter reaches many vendors with one key, ElevenLabs unlocks voice.</p>
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
			<p class="section-desc">Where your companion can act: this server and the desktops running the Nolune app. Rename a computer here; its state and hints match the Computers tab.</p>
		</div>
	</div>
	<div class="section-body">
		{#key slug}
			<ConnectedComputers {slug} compact />
		{/key}
		<div class="settings-links"><a class="nl-button-secondary" href={`/${slug}/computers`}>Open computers</a></div>
	</div>
</section>

<!-- Companions (#108): peer companions this owner paired with -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Companions</h3>
			<p class="section-desc">Other companions this one is paired with, on this machine or elsewhere. Pairing is an invite both owners confirm; sharing a host grants nothing.</p>
		</div>
	</div>
	<div class="section-body">
		{#key slug}
			<Companions />
		{/key}
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
