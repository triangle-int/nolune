<script lang="ts">
	import MoonBirth from "./MoonBirth.svelte";
	import { chooseOnboardingModel, connectOnboardingCodex, listOnboardingModels, resumeOnboarding, saveOnboardingKey, stepAfterProvider, type AvailableModel } from "./provider.js";
	import { CODEX_LOGIN_POLL_MS, codexErrorCopy, loginInstructions, loginProgress, offersDeviceCode } from "$lib/models/codex.js";
	import ArrowRight from "@lucide/svelte/icons/arrow-right";
	import {
		sendMessage,
		fetchSoulTemplates,
		applySoulTemplate,
		fetchSoul,
		setCompanionName,
		fetchConfigStatus,
		fetchModelPresets,
		updateLlmConfig,
		testPreset,
		fetchAvailableModels,
		chooseModel,
		fetchCodexStatus,
		startCodexLogin,
		type CodexLoginMethod,
		type CodexLoginStatus,
	} from "$lib/api/client.js";
	import type { SoulTemplate } from "$lib/api/types.js";
	import { getCompanion } from "$lib/stores/companion.svelte.js";
	import { introGreeting } from "$lib/companion/context.js";
	import { getSceneStore } from "$lib/stores/scene.svelte.js";
	import { getSkinStore, SKINS } from "$lib/stores/skin.svelte.js";
	import { getToasts } from "$lib/stores/toast.svelte.js";
	import { play, playImmediate, preload } from "$lib/sounds.js";
	import { hapticReveal } from "$lib/haptics.js";

	const toast = getToasts();
	const scene = getSceneStore();
	const skinStore = getSkinStore();

	let { slug, oncomplete }: { slug: string; oncomplete: () => void } = $props();

	/** The companion is Nolune until someone renames it; onboarding no longer asks. */
	const COMPANION_NAME = "Nolune";

	// Optional: set by an earlier visit; never falls back to the companion slug.
	function readPreferredName(): string | null {
		try {
			return typeof localStorage !== "undefined" ? localStorage.getItem("nolune:preferredName") : null;
		} catch {
			return null;
		}
	}

	const companion = getCompanion();

	type Stage =
		| "reveal"
		| "intro"
		| "waiting-key"
		| "testing"
		| "being-born"
		| "picking-soul"
		| "picking-provider"
		| "codex-login"
		| "codex-blocked"
		| "picking-model"
		| "models-blocked"
		| "waiting-first"
		| "sending"
		| "departing";

	let stage = $state<Stage>("reveal");
	let revealed = $state(false);
	let firstMessage = $state("");
	type KeyProvider = "anthropic" | "openai" | "openrouter";
	type OnboardingProvider = KeyProvider | "codex";
	const providerInfo: Record<OnboardingProvider, { label: string; keyUrl: string; placeholder: string }> = {
		anthropic: { label: "Anthropic", keyUrl: "https://console.anthropic.com/settings/keys", placeholder: "sk-ant-..." },
		openai: { label: "OpenAI", keyUrl: "https://platform.openai.com/api-keys", placeholder: "sk-..." },
		openrouter: { label: "OpenRouter", keyUrl: "https://openrouter.ai/settings/keys", placeholder: "sk-or-..." },
		codex: { label: "Codex", keyUrl: "", placeholder: "" },
	};
	let selectedProvider = $state<OnboardingProvider>("anthropic");
	const providerLabel = $derived(providerInfo[selectedProvider].label);
	const providerKeyUrl = $derived(providerInfo[selectedProvider].keyUrl);
	/** Providers whose key the server already holds (an earlier visit, or its environment). */
	let configuredKeys = $state<string[]>([]);
	let apiKeyInput = $state("");
	let apiKeyError = $state("");
	let messageInput: HTMLTextAreaElement | undefined = $state();
	let apiKeyInputEl: HTMLInputElement | undefined = $state();
	let lines = $state<{ text: string; revealed: string; done: boolean }[]>([]);
	let soulTemplates = $state<SoulTemplate[]>([]);
	/** What the spinner says while the server works. */
	let busyLabel = $state("connecting");
	// The model step: what the provider lists, a filter for long lists, and
	// why the last listing or pick did not go through.
	let models = $state<AvailableModel[]>([]);
	let modelFilter = $state("");
	let modelError = $state("");
	const MODEL_FILTER_FROM = 9;
	const shownModels = $derived.by(() => {
		const query = modelFilter.trim().toLowerCase();
		if (!query) return models;
		return models.filter((m) => m.name.toLowerCase().includes(query) || m.id.toLowerCase().includes(query));
	});

	function typewrite(text: string, speed = 38): Promise<void> {
		return new Promise((resolve) => {
			const entry = { text, revealed: "", done: false };
			lines = [...lines, entry];
			const idx = lines.length - 1;
			let i = 0;
			function tick() {
				if (i >= text.length) { lines[idx].done = true; resolve(); return; }
				const char = text[i];
				lines[idx].revealed += char;
				i++;
				if (i % 3 === 0) playImmediate("typewriter", { pitchRange: [0.88, 1.15] });
				let delay = speed;
				if (char === "." || char === "?" || char === "!") delay = speed * 8;
				else if (char === ",") delay = speed * 3;
				else if (char === "\u2014" || char === "\u2013") delay = speed * 4;
				setTimeout(tick, delay);
			}
			setTimeout(tick, speed);
		});
	}

	function pause(ms: number): Promise<void> {
		return new Promise((r) => setTimeout(r, ms));
	}

	async function runSequence() {
		skinStore.setSlug(slug);

		preload("intro_reveal", "typewriter");
		hapticReveal();
		revealed = true;
		stage = "intro";
		await pause(400);
		await typewrite(introGreeting(readPreferredName()));
		await pause(400);
		await typewrite("i'm nolune. a new space, just for us.");
		await pause(600);

		// Little Moon is the only skin and Nolune the default name: both are
		// saved as the choices the removed steps used to make.
		skinStore.setSkin(SKINS[0].id);
		try { await setCompanionName(slug, COMPANION_NAME); } catch {}

		play("intro_reveal");
		stage = "being-born";
	}

	async function finishBirth() {
		if (stage !== "being-born") return;
		stage = "intro";
		scene.enterOnboarding(slug);

		let hasSoul = false;
		try {
			const soul = await fetchSoul(slug);
			hasSoul = soul.exists && soul.content.trim().length > 0;
		} catch {}

		if (hasSoul) {
			await checkKeyThenAsk();
		} else {
			try { soulTemplates = await fetchSoulTemplates(); } catch { soulTemplates = []; }
			if (soulTemplates.length > 0) {
				await typewrite("who should i be for you?");
				stage = "picking-soul";
			} else {
				await askFirstMessage();
			}
		}
	}

	async function pickSoul(template: SoulTemplate) {
		stage = "intro";
		await pause(200);
		if (template.id !== "custom") {
			try { await applySoulTemplate(slug, template.id); } catch {}
			await typewrite(`${template.name}. i can be that.`);
		} else {
			await typewrite("a blank canvas. you can shape me later.");
		}
		await pause(400);
		await checkKeyThenAsk();
	}

	async function checkKeyThenAsk() {
		// A provider may already be configured (self-hosted users write
		// config.toml, and a model picked on an earlier visit stays picked).
		// `llm_configured` only says a key and a Chat preset exist (#28): the
		// preset is tested again, and only an answer skips this step.
		let next: ReturnType<typeof resumeOnboarding> = { step: "provider", reason: null };
		try {
			const status = await fetchConfigStatus();
			configuredKeys = status.configured_keys ?? [];
			if (status.llm_configured && status.chat_preset) {
				busyLabel = "connecting";
				stage = "testing";
				const [outcome, presets] = await Promise.all([
					testPreset(status.chat_preset).catch(() => null),
					fetchModelPresets().catch(() => null),
				]);
				const preset = presets?.presets.find((p) => p.id === status.chat_preset) ?? null;
				next = resumeOnboarding(status, outcome, preset);
				stage = "intro";
			}
		} catch {}
		if (next.step === "first-message") {
			await askFirstMessage();
			return;
		}

		// Ask how they want to connect, after what went wrong when something did.
		if (next.reason) {
			await typewrite(next.reason);
			await pause(300);
		}
		await typewrite("one more thing — how should i think?");
		stage = "picking-provider";
	}

	async function pickProvider(provider: OnboardingProvider) {
		selectedProvider = provider;
		apiKeyInput = "";
		apiKeyError = "";
		const step = stepAfterProvider(provider, configuredKeys);
		if (step === "codex") {
			await startCodex();
			return;
		}
		stage = "intro";
		if (step === "models") {
			await typewrite(`got it. your ${providerLabel} key is already here.`);
			await loadModels();
			return;
		}
		await typewrite(`got it. i'll need an ${providerLabel} API key.`);
		stage = "waiting-key";
		await pause(100);
		apiKeyInputEl?.focus();
	}

	/** Back from the key, codex or model step: a provider that will not answer is not the only way on. */
	async function chooseAnotherProvider() {
		if (!["waiting-key", "codex-login", "codex-blocked", "picking-model", "models-blocked"].includes(stage)) return;
		stopCodexPoll();
		apiKeyInput = "";
		apiKeyError = "";
		codexError = "";
		codexLogin = null;
		modelError = "";
		models = [];
		stage = "intro";
		await typewrite("how should i think, then?");
		stage = "picking-provider";
	}

	// --- Codex (#27): a ChatGPT login through the local codex binary ---
	// The gate is the login (`connectOnboardingCodex`), then a model that
	// answers, like any provider: no key is typed here; a login is started
	// on the server, its URL and code are shown, and the status is polled
	// until codex has the login.
	let codexLogin = $state<CodexLoginStatus | null>(null);
	let codexError = $state("");
	let codexPoll: ReturnType<typeof setInterval> | null = null;
	const codexSteps = $derived(loginInstructions(codexLogin));
	// The browser flow needs a browser on the server's machine; from another
	// device the way through is a device code, offered while it waits.
	const codexDeviceCodeOffered = $derived(offersDeviceCode(codexLogin));

	function stopCodexPoll() {
		if (codexPoll) clearInterval(codexPoll);
		codexPoll = null;
	}

	/** Codex was picked: say so, then check the binary and the login. */
	async function startCodex() {
		codexError = "";
		codexLogin = null;
		stage = "intro";
		await typewrite("got it. i'll think through your ChatGPT login, via codex.");
		await checkCodex("auto");
	}

	/**
	 * Check the binary and the login, then list the models or log in. "try
	 * again" comes back here, so a binary installed meanwhile or a login done
	 * elsewhere (`codex login`) is picked up without starting another.
	 */
	async function checkCodex(method: "auto" | CodexLoginMethod) {
		stopCodexPoll();
		codexError = "";
		busyLabel = "connecting";
		stage = "testing";
		try {
			await finishCodex();
		} catch (e) {
			const error = e as Error & { codex?: "binary" | "unavailable" | "login" };
			if (error.codex === "login") {
				await beginCodexLogin(method);
				return;
			}
			codexError = error.message;
			stage = "codex-blocked";
		}
	}

	/** Codex holds a login: on to its models. */
	async function finishCodex() {
		await connectOnboardingCodex({ fetchCodexStatus });
		stopCodexPoll();
		codexLogin = null;
		stage = "intro";
		await pause(200);
		await typewrite("logged in.");
		await loadModels();
	}

	async function beginCodexLogin(method: "auto" | CodexLoginMethod) {
		stopCodexPoll();
		codexError = "";
		busyLabel = "connecting";
		stage = "testing";
		try {
			const started = await startCodexLogin(method);
			if (!started.ok) {
				codexError = codexErrorCopy(started);
				stage = "codex-blocked";
				return;
			}
			codexLogin = started.value;
			stage = "codex-login";
			const loginId = started.value.id;
			const poll = setInterval(async () => {
				let progress: ReturnType<typeof loginProgress>;
				try {
					progress = loginProgress(await fetchCodexStatus(), loginId);
				} catch {
					return; // a missed poll is not an outcome
				}
				// "use a device code" replaced this login while the status was
				// being read: the new login's poll owns the outcome.
				if (codexPoll !== poll) return;
				if (progress === "pending") return;
				stopCodexPoll();
				if (progress === "completed") {
					stage = "testing";
					try {
						await finishCodex();
					} catch (e) {
						codexError = e instanceof Error ? e.message : "Codex did not answer.";
						stage = "codex-blocked";
					}
					return;
				}
				codexError = progress === "failed" ? "The login did not finish. Try again, or use a device code." : "Another login replaced this one. Try again.";
				codexLogin = null;
				stage = "codex-blocked";
			}, CODEX_LOGIN_POLL_MS);
			codexPoll = poll;
		} catch (e) {
			codexError = e instanceof Error ? e.message : "The login could not start.";
			stage = "codex-blocked";
		}
	}

	async function submitApiKey() {
		const key = apiKeyInput.trim();
		if (!key || stage !== "waiting-key") return;
		apiKeyError = "";
		busyLabel = "checking the key";
		stage = "testing";

		try {
			// The key is probed before it is saved; a wrong one keeps this
			// step open with the reason in the error line.
			await saveOnboardingKey(selectedProvider as KeyProvider, key, { updateLlmConfig });
			apiKeyInput = "";
			if (!configuredKeys.includes(selectedProvider)) configuredKeys = [...configuredKeys, selectedProvider];
			stage = "intro";
			await pause(200);
			await typewrite("that works.");
			await loadModels();
		} catch (e) {
			apiKeyError = e instanceof Error ? e.message : "invalid key";
			stage = "waiting-key";
			await pause(100);
			apiKeyInputEl?.focus();
		}
	}

	function handleKeyKeydown(e: KeyboardEvent) {
		if (e.key === "Enter") { e.preventDefault(); submitApiKey(); }
	}

	/** Ask the provider which models this account has, then offer them. */
	async function loadModels() {
		modelError = "";
		modelFilter = "";
		busyLabel = "looking for models";
		stage = "testing";
		try {
			models = await listOnboardingModels(selectedProvider, { fetchAvailableModels });
		} catch (e) {
			models = [];
			modelError = e instanceof Error ? e.message : `${providerLabel} did not list its models.`;
			stage = "models-blocked";
			return;
		}
		stage = "intro";
		await typewrite("which model should i think with?");
		stage = "picking-model";
	}

	/**
	 * The server tests the model and saves it only when it answers (#28);
	 * one that does not keeps the list open, with what went wrong.
	 */
	async function pickModel(model: AvailableModel) {
		if (stage !== "picking-model") return;
		modelError = "";
		busyLabel = "connecting";
		stage = "testing";
		try {
			await chooseOnboardingModel(selectedProvider, model, { chooseModel });
		} catch (e) {
			modelError = e instanceof Error ? e.message : `${model.name} did not answer.`;
			stage = "picking-model";
			return;
		}
		stage = "intro";
		await pause(200);
		await typewrite(`${model.name}. connected.`);
		await pause(400);
		await askFirstMessage();
	}

	async function askFirstMessage() {
		await typewrite("tell me something.");
		stage = "waiting-first";
		await pause(100);
		messageInput?.focus();
	}

	async function submitFirst() {
		const content = firstMessage.trim();
		if (!content) return;
		stage = "sending";
		try {
			await sendMessage(slug, content);
			await companion.refresh();
		} catch {
			toast.error("setup failed — try sending a message after");
		}
		// Start cinematic intro, then complete
		scene.finishOnboarding();
		stage = "departing";
		await pause(600);
		oncomplete();
	}

	function handleMessageKeydown(e: KeyboardEvent) {
		if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submitFirst(); }
	}

	$effect(() => { runSequence(); });
	$effect(() => stopCodexPoll);
</script>

{#if stage === "being-born"}
	<MoonBirth name={COMPANION_NAME} oncomplete={finishBirth} />
{/if}
<div class="ob" inert={stage === "being-born"} class:ob-birthing={stage === "being-born"} class:ob-depart={stage === "departing"} class:ob-hidden={stage === "reveal" && !revealed}>
	<div class="ob-content">
		<!-- Typewriter lines -->
		<div class="ob-lines" class:ob-lines-hidden={stage === "reveal"}>
			{#each lines as line, i}
				<div class="ob-line" style="animation-delay: {i * 40}ms">
					{#if i === 0 && line.done}
						<p class="ob-title">{line.revealed}</p>
					{:else if i === 0}
						<p class="ob-title">{line.revealed}<span class="ob-cursor"></span></p>
					{:else if !line.done}
						<p class="ob-text">{line.revealed}<span class="ob-cursor"></span></p>
					{:else}
						<p class="ob-text">{line.revealed}</p>
					{/if}
				</div>
			{/each}
		</div>

		<!-- Interactive sections -->
		<div class="ob-input-area">
			{#if stage === "sending"}
				<div class="ob-enter ob-center">
					<div class="ob-spinner"></div>
					<span class="ob-spinner-label">thinking</span>
				</div>
			{/if}

			{#if stage === "picking-soul"}
				<div class="ob-enter">
					<div class="ob-pills ob-pills-soul">
						{#each soulTemplates as template (template.id)}
							<button onclick={() => pickSoul(template)} class="ob-pill ob-pill-col ob-pill-soul">
								<span class="ob-pill-label">{template.name}</span>
								<span class="ob-pill-note">{template.description}</span>
							</button>
						{/each}
					</div>
				</div>
			{/if}

			{#if stage === "picking-provider"}
				<div class="ob-enter">
					<div class="ob-pills ob-pills-soul ob-pills-providers">
						<button onclick={() => pickProvider("anthropic")} class="ob-pill ob-pill-col ob-pill-soul">
							<span class="ob-pill-label">Anthropic</span>
							<span class="ob-pill-note">pay-per-use</span>
						</button>
						<button onclick={() => pickProvider("openai")} class="ob-pill ob-pill-col ob-pill-soul">
							<span class="ob-pill-label">OpenAI</span>
							<span class="ob-pill-note">pay-per-use</span>
						</button>
						<button onclick={() => pickProvider("openrouter")} class="ob-pill ob-pill-col ob-pill-soul">
							<span class="ob-pill-label">OpenRouter</span>
							<span class="ob-pill-note">many models</span>
						</button>
						<button onclick={() => pickProvider("codex")} class="ob-pill ob-pill-col ob-pill-soul">
							<span class="ob-pill-label">Codex</span>
							<span class="ob-pill-note">your ChatGPT login</span>
						</button>
					</div>
				</div>
			{/if}

			{#if stage === "codex-login" && codexSteps}
				<div class="ob-enter">
					<div class="ob-codex" aria-live="polite">
						<a href={codexSteps.url} target="_blank" rel="noopener" class="ob-codex-link">{codexSteps.url}</a>
						{#if codexSteps.code}
							<span class="ob-codex-code" aria-label="Device code">{codexSteps.code}</span>
						{/if}
						<p class="ob-codex-note">{codexSteps.note} i'll notice once codex has the login.</p>
					</div>
					{#if codexDeviceCodeOffered}
						<button type="button" onclick={() => beginCodexLogin("device_code")} class="ob-hint ob-hint-button">on another device? use a device code</button>
					{/if}
					<button type="button" onclick={chooseAnotherProvider} class="ob-hint ob-hint-button">choose another provider</button>
				</div>
			{/if}

			{#if stage === "codex-blocked"}
				<div class="ob-enter">
					<p class="ob-error" role="alert">{codexError}</p>
					<div class="ob-pills ob-pills-soul ob-pills-providers">
						<button type="button" onclick={() => checkCodex("auto")} class="ob-pill">try again</button>
						<button type="button" onclick={() => checkCodex("device_code")} class="ob-pill">use a device code</button>
					</div>
					<button type="button" onclick={chooseAnotherProvider} class="ob-hint ob-hint-button">choose another provider</button>
				</div>
			{/if}

			{#if stage === "waiting-key"}
				<div class="ob-enter">
					<div class="ob-field">
						<label class="ob-label" for="provider-key">{providerLabel} API key</label>
                        <!-- svelte-ignore a11y_autofocus -->
						<input
							id="provider-key"
                            bind:this={apiKeyInputEl}
							bind:value={apiKeyInput}
							onkeydown={handleKeyKeydown}
							placeholder={providerInfo[selectedProvider].placeholder}
							class="ob-input ob-input-mono"
							type="password"
							autofocus
						/>
						{#if apiKeyInput.trim()}
							<button onclick={submitApiKey} class="ob-go" aria-label="Submit"><ArrowRight size={18} aria-hidden="true" /></button>
						{/if}
					</div>
					{#if apiKeyError}
						<p class="ob-error" role="alert">{apiKeyError}</p>
					{/if}
					<a href={providerKeyUrl} target="_blank" rel="noopener" class="ob-hint">
						Get your {providerLabel} API key
					</a>
					<button type="button" onclick={chooseAnotherProvider} class="ob-hint ob-hint-button">choose another provider</button>
				</div>
			{/if}

			{#if stage === "testing"}
				<div class="ob-enter ob-center" role="status">
					<div class="ob-spinner"></div>
					<span class="ob-spinner-label">{busyLabel}</span>
				</div>
			{/if}

			{#if stage === "picking-model"}
				<div class="ob-enter">
					{#if models.length >= MODEL_FILTER_FROM}
						<div class="ob-field ob-model-filter">
							<label class="ob-label" for="model-filter">Find a model</label>
							<input id="model-filter" bind:value={modelFilter} placeholder="Name or model id" class="ob-input" autocomplete="off" spellcheck="false" />
						</div>
					{/if}
					{#if modelError}
						<p class="ob-error ob-model-error" role="alert">{modelError}</p>
					{/if}
					<ul class="ob-models" aria-label="{providerLabel} models">
						{#each shownModels as model (model.id)}
							<li>
								<button type="button" onclick={() => pickModel(model)} class="ob-pill ob-pill-col ob-pill-soul ob-model">
									<span class="ob-pill-label">{model.name}</span>
									{#if model.name !== model.id}
										<span class="ob-model-id">{model.id}</span>
									{/if}
									{#if model.description}
										<span class="ob-pill-note">{model.description}</span>
									{/if}
								</button>
							</li>
						{:else}
							<li class="ob-models-empty">No model matches “{modelFilter.trim()}”.</li>
						{/each}
					</ul>
					<p class="ob-models-note">You can add more models or a separate background model later in Settings.</p>
					<button type="button" onclick={chooseAnotherProvider} class="ob-hint ob-hint-button">choose another provider</button>
				</div>
			{/if}

			{#if stage === "models-blocked"}
				<div class="ob-enter">
					<p class="ob-error" role="alert">{modelError}</p>
					<div class="ob-pills ob-pills-soul ob-pills-providers">
						<button type="button" onclick={loadModels} class="ob-pill">try again</button>
						<button type="button" onclick={chooseAnotherProvider} class="ob-pill">another provider</button>
					</div>
				</div>
			{/if}

			{#if stage === "waiting-first"}
				<div class="ob-enter">
					<div class="ob-field">
						<label class="ob-label" for="first-message">Your first message</label>
                        <textarea id="first-message" bind:this={messageInput} bind:value={firstMessage} onkeydown={handleMessageKeydown} placeholder="What’s on your mind?" rows={3} class="ob-input ob-textarea"></textarea>
						{#if firstMessage.trim()}
							<button onclick={submitFirst} class="ob-go ob-go-textarea" aria-label="Send"><ArrowRight size={18} aria-hidden="true" /></button>
						{/if}
					</div>
				</div>
			{/if}
		</div>
	</div>
</div>

<style>
	.ob-birthing{visibility:hidden;}
	.ob {
		position: relative;
		display: flex;
		height: 100%;
		align-items: center;
		justify-content: center;
		overflow: hidden;
		pointer-events: none;
		z-index: 10;
		transition: opacity 0.6s ease;
	}
	.ob-hidden { opacity: 0; }

	.ob-content {
		position: relative;
		width: 100%;
		max-width: 420px;
		padding: 0 1.5rem;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 1.5rem;
		pointer-events: auto;
	}

	/* ── Lines ── */
	.ob-lines { display: flex; flex-direction: column; gap: 0.625rem; width: 100%; }
	.ob-lines-hidden { visibility: hidden; }

	.ob-line { animation: ob-fade-in 0.4s cubic-bezier(0.16, 1, 0.3, 1) both; }
	@keyframes ob-fade-in {
		from { opacity: 0; transform: translateY(6px); }
		to { opacity: 1; transform: translateY(0); }
	}

	.ob-title {
		font-family: var(--font-display);
		font-size: 1.35rem;
		font-weight: 400;
		font-style: normal;
		letter-spacing: -0.01em;
		color: var(--foreground);
		text-align: center;
	}

	.ob-text {
		font-family: var(--font-body);
		font-size: 0.85rem;
		line-height: 1.6;
		color: var(--text-secondary);
		text-align: center;
	}

	.ob-cursor {
		display: inline-block;
		width: 1.5px;
		height: 1.05em;
		margin-left: 1px;
		vertical-align: text-bottom;
		background: var(--accent);
		animation: blink 0.8s steps(2) infinite;
	}
	@keyframes blink { 0% { opacity: 1; } 100% { opacity: 0; } }

	/* ── Input area ── */
	.ob-input-area { width: 100%; }
	.ob-enter {
		animation: ob-slide-in 0.5s cubic-bezier(0.16, 1, 0.3, 1) both;
		animation-delay: 80ms;
	}
	@keyframes ob-slide-in {
		from { opacity: 0; transform: translateY(10px); }
		to { opacity: 1; transform: translateY(0); }
	}

	.ob-center { display: flex; align-items: center; justify-content: center; gap: 0.625rem; }

	/* Companion preferences */
	.ob-pills { display: flex; gap: 0.5rem; flex-wrap: wrap; }
	.ob-pills-soul { display: grid; grid-template-columns: repeat(2, 1fr); }
	.ob-pills-providers { grid-template-columns: repeat(2, 1fr); }

	.ob-pill {
		min-height: 44px;
		display: flex;
		align-items: center;
		justify-content: center;
		flex: 1;
		padding: 0.55rem 0.75rem;
		border-radius: 8px;
		background: var(--card);
		backdrop-filter: none;
		-webkit-backdrop-filter: none;
		border: 1px solid var(--border);
		border-top-color: var(--border);
		font-family: var(--font-body);
		font-size: 0.875rem;
		font-style: normal;
		color: var(--text-secondary);
		cursor: pointer;
		transition: all 0.3s ease;
		box-shadow: none;
	}
	.ob-pill:hover {
		border-color: var(--text-secondary);
		background: var(--accent);
		color: var(--text-secondary);
		box-shadow: none;
	}

	.ob-pill-col {
		flex-direction: column;
		gap: 0.2rem;
		padding: 0.65rem 0.75rem;
	}
	.ob-pill-soul {
		border-radius: 1rem;
		align-items: flex-start;
		padding: 0.75rem 1rem;
		gap: 0.25rem;
	}
	.ob-pill-label {
		font-family: var(--font-body);
		font-size: 0.875rem;
		font-style: normal;
		color: var(--text-secondary);
	}
	.ob-pill-note {
		font-family: var(--font-body);
		font-size: 0.75rem;
		font-style: normal;
		color: var(--text-secondary);
	}

	/* Inputs */
	.ob-field { position: relative; }

	.ob-input {
		width: 100%;
		padding: 0.75rem 3rem 0.75rem 1.25rem;
		border-radius: 8px;
		background: var(--card);
		backdrop-filter: none;
		-webkit-backdrop-filter: none;
		border: 1px solid var(--border);
		border-top-color: var(--border);
		font-family: var(--font-body);
		font-size: 1rem;
		font-style: normal;
		color: var(--text-secondary);
		outline: none;
		text-align: center;
		transition: all 0.3s ease;
		box-shadow: none;
	}
	.ob-input::placeholder { color: var(--text-secondary); font-style: normal; }
	.ob-input:focus {
		border-color: var(--text-secondary);
		box-shadow: none;
	}
	.ob-input-mono { font-family: var(--font-mono); font-size: 0.8rem; font-style: normal; text-align: left; }

	.ob-textarea {
		border-radius: 1rem;
		resize: none;
		text-align: left;
		font-family: var(--font-body);
		font-size: 0.85rem;
		font-style: normal;
		line-height: 1.6;
		padding: 0.875rem 1.25rem;
		overflow-x: hidden;
	}
	/* Onboarding scrolls on short screens, but without a visible bar. */
	.ob, .ob-textarea { scrollbar-width: none; }
	.ob::-webkit-scrollbar, .ob-textarea::-webkit-scrollbar { display: none; }

	.ob-go {
		position: absolute;
		right: 0.5rem;
		top: 50%;
		transform: translateY(-50%);
		display: flex;
		align-items: center;
		justify-content: center;
		width: 44px;
		height: 44px;
		border-radius: 50%;
		font-size: 0.9rem;
		color: var(--text-secondary);
		transition: all 0.3s ease;
		cursor: pointer;
	}
	.ob-go:hover { color: var(--text-secondary); background: var(--accent); }
	.ob-go-textarea { top: auto; bottom: 0.5rem; transform: none; }

	.ob-error { margin-top: 0.5rem; font-size: 0.72rem; color: var(--destructive); font-style: normal; text-align: center; }

	.ob-hint {
		display: block;
		margin-top: 0.5rem;
		font-size: 0.75rem;
		color: var(--primary);
		text-decoration: none;
		text-align: center;
		transition: color 0.2s ease;
	}
	.ob-hint:hover { color: var(--primary); }

	/* ── Spinner ── */
	.ob-spinner {
		width: 12px; height: 12px;
		border: 1.5px solid var(--border);
		border-top-color: var(--text-secondary);
		border-radius: 50%;
		animation: spin 0.7s linear infinite;
	}
	@keyframes spin { to { transform: rotate(360deg); } }
	.ob-spinner-label {
		font-family: var(--font-display);
		font-size: 0.72rem;
		font-style: normal;
		color: var(--text-secondary);
	}

	/* ── Depart ── */
	.ob-depart { animation: depart 0.5s cubic-bezier(0.55, 0, 1, 0.45) forwards; }
	@keyframes depart { to { opacity: 0; transform: scale(0.98); } }

    .ob { overflow-y: auto; padding: 32px 0; }
    .ob-content { max-width: 520px; margin-block: auto; }
    .ob-title { font-size: 2rem; line-height: 1.2; }
    .ob-text { font-size: 1rem; }
    .ob-pill { background: var(--card); border-color: var(--border); box-shadow: none; backdrop-filter: none; }
    .ob-pill:hover { background: var(--popover); border-color: var(--primary); box-shadow: none; color: var(--foreground); }
    .ob-pill-label { color: var(--foreground); }
    .ob-pill-note { color: var(--text-secondary); line-height: 1.5; }
    .ob-label { display: block; margin-bottom: 8px; color: var(--text-secondary); font-size: 14px; }
    .ob-input { min-height: 56px; padding: 12px 60px 12px 16px; background: var(--card); border-color: var(--input); color: var(--foreground); text-align: left; box-shadow: none; backdrop-filter: none; }
    .ob-input:focus { border-color: var(--ring); box-shadow: none; }
    .ob-input::placeholder { color: var(--text-muted); opacity: 1; }
    .ob-input-mono, .ob-textarea { font-size: 1rem; }
    .ob-textarea { padding-bottom: 60px; }
    .ob-go { top: auto; bottom: 6px; right: 6px; transform: none; border-radius: 8px; background: var(--primary); color: var(--primary-foreground); }
    .ob-go:hover { background: var(--primary); color: var(--primary-foreground); filter: brightness(1.06); }
    .ob-error { color: var(--destructive); font-size: 14px; }
    .ob-hint, .ob-hint:hover { color: var(--primary); font-size: 13px; min-height: 44px; padding-top: 12px; text-decoration: underline; text-underline-offset: 3px; }
    .ob-hint-button { width: 100%; margin-top: 0; padding-bottom: 0; background: none; border: 0; font: inherit; font-size: 13px; cursor: pointer; }
    .ob-hint-button:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; border-radius: 4px; }
    .ob-spinner-label { font-family: var(--font-body); font-size: 14px; }
    .ob-spinner { width: 16px; height: 16px; border-color: var(--border); border-top-color: var(--primary); }
    .ob-cursor { background: var(--primary); }
    /* Codex login (#27): the URL to open and the code to type, in the pairing panel's shape. */
    .ob-codex { display: flex; flex-direction: column; align-items: center; gap: 8px; padding: 16px; border-radius: 8px; border: 1px solid var(--primary); background: var(--accent); }
    .ob-codex-link { max-width: 100%; font-family: var(--font-mono); font-size: 0.8125rem; color: var(--primary); text-decoration: underline; text-underline-offset: 3px; overflow-wrap: anywhere; text-align: center; }
    .ob-codex-code { font-family: var(--font-mono); font-size: 1.75rem; letter-spacing: 0.14em; color: var(--foreground); }
    .ob-codex-note { margin: 0; font-size: 13px; line-height: 1.5; color: var(--text-secondary); text-align: center; }
    .ob-codex-note, .ob-codex-link { user-select: text; }
    /* The model step: one column of the provider's models, scrolling on its own when long. */
    .ob-models { list-style: none; margin: 0; padding: 2px; display: flex; flex-direction: column; gap: 8px; max-height: min(52vh, 440px); overflow-y: auto; overscroll-behavior: contain; }
    .ob-model { width: 100%; text-align: left; }
    .ob-model:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
    .ob-model-id { font-family: var(--font-mono); font-size: 12px; line-height: 1.4; color: var(--text-muted); overflow-wrap: anywhere; }
    .ob-model-filter { margin-bottom: 12px; }
    .ob-model-filter .ob-input { padding-right: 16px; }
    .ob-model-error { margin: 0 0 12px; }
    .ob-models-empty { padding: 12px; font-size: 14px; color: var(--text-muted); text-align: center; }
    .ob-models-note { margin: 12px 0 0; font-size: 13px; line-height: 1.5; color: var(--text-muted); text-align: center; }
    @media (max-width: 480px) { .ob-pills-soul { grid-template-columns: 1fr; } .ob-pills-providers { grid-template-columns: repeat(2, 1fr); } }
</style>
