<script lang="ts">
	// The Codex tile (#27): whether the pinned codex binary is on this
	// server, who codex is logged in as, and a login or logout. Everything
	// shown comes from GET /api/config/codex/status; the copy for each state
	// is the pure `codexView`. A pending login is polled until it ends.
	import { fetchCodexStatus, logoutCodex, startCodexLogin, type CodexLoginMethod, type CodexStatus } from "$lib/api/client.js";
	import { CODEX_LOGIN_POLL_MS, codexErrorCopy, codexView, loginProgress } from "$lib/models/codex.js";

	let {
		sample = null,
		onchange,
	}: {
		/** A status to show instead of fetching one (the design-system example); the actions then do nothing. */
		sample?: CodexStatus | null;
		/** Called with every status change that came from a login or logout here. */
		onchange?: (status: CodexStatus) => void;
	} = $props();

	let status = $state<CodexStatus | null>(null);
	let loadError = $state("");
	let busy = $state<"" | "login" | "device_code" | "logout">("");
	let actionError = $state("");
	const view = $derived(codexView(sample ?? status));
	let poll: ReturnType<typeof setInterval> | null = null;

	function stopPolling() {
		if (poll) clearInterval(poll);
		poll = null;
	}

	/** Poll while the login stays pending; stop when it ends or is replaced. */
	function watch(loginId: string) {
		stopPolling();
		poll = setInterval(async () => {
			try {
				const next = await fetchCodexStatus();
				status = next;
				if (loginProgress(next, loginId) !== "pending") {
					stopPolling();
					onchange?.(next);
				}
			} catch {
				// A missed poll is not an outcome; the next one reads again.
			}
		}, CODEX_LOGIN_POLL_MS);
	}

	async function load() {
		if (sample) return;
		loadError = "";
		try {
			status = await fetchCodexStatus();
			if (status.login?.state === "pending") watch(status.login.id);
		} catch (e) {
			loadError = e instanceof Error ? e.message : "Could not read the codex status.";
		}
	}

	async function login(method: "auto" | CodexLoginMethod) {
		if (sample || busy) return;
		busy = method === "device_code" ? "device_code" : "login";
		actionError = "";
		try {
			const started = await startCodexLogin(method);
			if (!started.ok) {
				actionError = codexErrorCopy(started);
				return;
			}
			// The status names the login until it ends; read it and watch.
			status = await fetchCodexStatus();
			watch(started.value.id);
		} catch (e) {
			actionError = e instanceof Error ? e.message : "The login could not start.";
		} finally {
			busy = "";
		}
	}

	async function logout() {
		if (sample || busy) return;
		busy = "logout";
		actionError = "";
		try {
			const answer = await logoutCodex();
			if (!answer.ok) {
				actionError = codexErrorCopy(answer);
				return;
			}
			stopPolling();
			status = answer.value;
			onchange?.(answer.value);
		} catch (e) {
			actionError = e instanceof Error ? e.message : "The logout could not run.";
		} finally {
			busy = "";
		}
	}

	$effect(() => {
		load();
		return stopPolling;
	});
</script>

<div class="codex" data-state={view.state}>
	<div class="codex-row">
		<div class="key-info">
			<span class="key-name">Codex</span>
			<span class="key-hint">{view.binary || "ChatGPT login through the local codex binary"}</span>
		</div>
		<div class="key-action">
			{#if view.state === "logged_in"}
				<span class="key-badge key-badge-ok">Logged in</span>
			{:else if view.state === "pending"}
				<span class="key-badge key-badge-ok">Waiting for you</span>
			{/if}
			{#if view.canLogin}
				<button class="key-change key-change-add" type="button" onclick={() => login("auto")} disabled={!!busy}>
					{busy === "login" ? "Starting..." : view.state === "unavailable" ? "Retry" : "Log in"}
				</button>
				<button class="key-change" type="button" onclick={() => login("device_code")} disabled={!!busy} title="For a server you reach from another computer: a code to enter on any device">
					{busy === "device_code" ? "Starting..." : "Use a device code"}
				</button>
			{/if}
			{#if view.canLogout}
				<button class="key-change key-change-remove" type="button" onclick={logout} disabled={!!busy}>
					{busy === "logout" ? "..." : view.state === "pending" ? "Cancel" : "Log out"}
				</button>
			{/if}
		</div>
	</div>
	<p class="codex-headline" role="status">{view.headline}</p>
	{#if view.detail}
		<p class="setting-hint" class:setting-warning={view.state !== "logged_in"}>{view.detail}</p>
	{/if}
	{#if view.instructions}
		{@const steps = view.instructions}
		<div class="pairing-panel codex-panel" aria-live="polite">
			<a class="codex-link" href={steps.url} target="_blank" rel="noopener">{steps.url}</a>
			{#if steps.code}
				<span class="pairing-code" aria-label="Device code">{steps.code}</span>
			{/if}
			<p class="setting-hint">{steps.note} This page updates by itself once codex has the login.</p>
		</div>
	{/if}
	{#if loadError}
		<p class="key-error" role="alert">{loadError}</p>
	{/if}
	{#if actionError}
		<p class="key-error" role="alert">{actionError}</p>
	{/if}
</div>
