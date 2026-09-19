<script lang="ts">
	import AudioLines from "@lucide/svelte/icons/audio-lines";
	import Mail from "@lucide/svelte/icons/mail";
	import Github from "@lucide/svelte/icons/github";
	import Server from "@lucide/svelte/icons/server";
	import * as Select from "$lib/components/ui/select/index.js";
	import { page } from "$app/state";
	import {
		fetchServerConfig,
		updateServerConfig,
		fetchMeta,
		fetchChangelog,
		getUpdateChannel,
		setUpdateChannel,
		fetchVoiceId,
		updateVoiceId,
		fetchEmailAccounts,
		saveEmailAccounts,
		deleteAllEmailAccounts,
		fetchGithubConfig,
		updateGithubToken,
		type ChangelogEntry,
		type EmailConfig,
	} from "$lib/api/client.js";
	import DOMPurify from "dompurify";
	import { Marked } from "marked";
	import { getToasts } from "$lib/stores/toast.svelte.js";

	// Advanced (#98): server wiring and raw integration fields for self-hosters.
	// Nothing here is needed to set up a companion; everything here is still
	// reachable and documented in docs/settings.md.
	const slug = $derived(page.params.slug!);

	// --- server (host, port, API token) ---
	let serverHost = $state("0.0.0.0");
	let serverPort = $state(26559);
	let serverAuthSet = $state(false);
	let serverLoading = $state(true);
	let serverSaving = $state(false);
	let serverNeedsRestart = $state(false);
	let serverAuthInput = $state("");
	let serverPortInput = $state("26559");

	async function loadServer() {
		serverLoading = true;
		try {
			const res = await fetchServerConfig();
			serverHost = res.host;
			serverPort = res.port;
			serverPortInput = String(res.port);
			serverAuthSet = res.auth_token_set;
		} catch {
			// not critical
		} finally {
			serverLoading = false;
		}
	}

	async function saveServerPort() {
		const port = parseInt(serverPortInput);
		if (isNaN(port) || port < 1 || port > 65535) return;
		serverSaving = true;
		try {
			const res = await updateServerConfig({ port });
			serverPort = port;
			serverNeedsRestart = res.needs_restart;
		} catch {
			// ignore
		} finally {
			serverSaving = false;
		}
	}

	async function saveServerAuth() {
		serverSaving = true;
		try {
			await updateServerConfig({ auth_token: serverAuthInput.trim() });
			serverAuthSet = serverAuthInput.trim().length > 0;
			serverAuthInput = "";
		} catch {
			// ignore
		} finally {
			serverSaving = false;
		}
	}

	async function clearServerAuth() {
		serverSaving = true;
		try {
			await updateServerConfig({ auth_token: "" });
			serverAuthSet = false;
		} catch {
			// ignore
		} finally {
			serverSaving = false;
		}
	}

	// --- updates (version, channel, what's new) ---
	let version = $state("");
	let commit = $state("");
	let channel = $state("stable");
	let channelSaving = $state(false);
	let changelog = $state<ChangelogEntry[]>([]);
	let showChangelog = $state(false);
	const changelogMd = new Marked({ breaks: true, gfm: true });
	$effect(() => {
		fetchMeta().then((meta) => { version = meta.version; commit = meta.commit; }).catch(() => {});
		getUpdateChannel().then((r) => (channel = r.channel)).catch(() => {});
		fetchChangelog().then((c) => (changelog = c)).catch(() => {});
	});
	async function changeChannel(next: string) {
		if (!next || channelSaving) return;
		channelSaving = true;
		try { await setUpdateChannel(next); channel = next; }
		catch { getToasts().error("Could not change update channel."); }
		finally { channelSaving = false; }
	}

	// --- voice id ---
	let voiceId = $state("");
	let voiceLoading = $state(true);
	let voiceSaving = $state(false);
	let voiceInput = $state("");

	async function loadVoice() {
		voiceLoading = true;
		try {
			const res = await fetchVoiceId(slug);
			voiceId = res.voice_id || "";
			voiceInput = voiceId;
		} catch {
			// not critical
		} finally {
			voiceLoading = false;
		}
	}

	async function saveVoice() {
		voiceSaving = true;
		try {
			await updateVoiceId(slug, voiceInput.trim());
			voiceId = voiceInput.trim();
		} catch {
			// ignore
		} finally {
			voiceSaving = false;
		}
	}

	async function clearVoice() {
		voiceSaving = true;
		try {
			await updateVoiceId(slug, "");
			voiceId = "";
			voiceInput = "";
		} catch {
			// ignore
		} finally {
			voiceSaving = false;
		}
	}

	// --- email (SMTP/IMAP) ---
	let emailAccounts = $state<Partial<EmailConfig>[]>([]);
	let emailLoading = $state(true);
	let emailSaving = $state(false);
	let emailError = $state("");
	let emailAdding = $state(false);

	function emptyEmailForm(): EmailConfig {
		return {
			smtp_host: "", smtp_port: 587, smtp_user: "", smtp_password: "", smtp_from: "",
			imap_host: "", imap_port: 993, imap_user: "", imap_password: "",
		};
	}
	let emailForm = $state<EmailConfig>(emptyEmailForm());

	async function loadEmail() {
		emailLoading = true;
		try {
			const res = await fetchEmailAccounts(slug);
			emailAccounts = res.accounts || [];
		} catch {
			// not critical
		} finally {
			emailLoading = false;
		}
	}

	async function saveNewEmail() {
		emailSaving = true;
		emailError = "";
		try {
			// Merge existing accounts (fill in missing passwords) + new one
			const existing: EmailConfig[] = emailAccounts.map((a) => ({
				...emptyEmailForm(),
				...a,
			}));
			existing.push(emailForm);
			await saveEmailAccounts(slug, existing);
			emailAdding = false;
			emailForm = emptyEmailForm();
			await loadEmail();
		} catch (e: any) {
			emailError = e?.message || "failed to save email account";
		} finally {
			emailSaving = false;
		}
	}

	async function removeEmailAccount(index: number) {
		emailSaving = true;
		emailError = "";
		try {
			const remaining = emailAccounts.filter((_, i) => i !== index).map((a) => ({
				...emptyEmailForm(),
				...a,
			}));
			if (remaining.length > 0) {
				await saveEmailAccounts(slug, remaining);
			} else {
				await deleteAllEmailAccounts(slug);
			}
			await loadEmail();
		} catch (e: any) {
			emailError = e?.message || "failed to remove email account";
		} finally {
			emailSaving = false;
		}
	}

	// --- github ---
	let ghConfigured = $state(false);
	let ghLoading = $state(true);
	let ghToken = $state("");
	let ghSaving = $state(false);
	let ghError = $state("");
	let ghEditing = $state(false);

	async function loadGithub() {
		ghLoading = true;
		try {
			const res = await fetchGithubConfig();
			ghConfigured = res.configured;
		} catch {
			// not critical
		} finally {
			ghLoading = false;
		}
	}

	async function saveGithubToken() {
		const token = ghToken.trim();
		ghSaving = true;
		ghError = "";
		try {
			const res = await updateGithubToken(token);
			ghConfigured = res.configured;
			ghToken = "";
			ghEditing = false;
		} catch (e: any) {
			ghError = e?.message || "failed to save token";
		} finally {
			ghSaving = false;
		}
	}

	async function disconnectGithub() {
		ghSaving = true;
		ghError = "";
		try {
			const res = await updateGithubToken("");
			ghConfigured = res.configured;
		} catch (e: any) {
			ghError = e?.message || "failed to disconnect";
		} finally {
			ghSaving = false;
		}
	}

	$effect(() => {
		slug;
		loadServer();
		loadVoice();
		loadEmail();
		loadGithub();
	});
</script>

<!-- Server -->
<section class="settings-section">
	<div class="section-header">
		<div class="section-icon" aria-hidden="true"><Server size={20} strokeWidth={1.75} /></div>
		<div>
			<h3 class="section-label">Server <span class="owner-badge">This server</span></h3>
			<p class="section-desc">Network and API token. More lives in <code>config.toml</code>.</p>
		</div>
	</div>

	{#if serverLoading}
		<p class="dim-text">Loading...</p>
	{:else}
		<div class="setting-row">
			<label class="setting-label" for="server-port">Port</label>
			<div class="setting-input-row">
				<input class="setting-input" id="server-port" type="number" min="1" max="65535" bind:value={serverPortInput} />
				{#if String(serverPort) !== serverPortInput}
					<button class="setting-btn" onclick={saveServerPort} disabled={serverSaving}>
						{serverSaving ? "..." : "Save"}
					</button>
				{/if}
			</div>
			{#if serverNeedsRestart}
				<p class="setting-hint setting-warning">Restart nolune to apply port change</p>
			{/if}
		</div>

		<div class="setting-row">
			<span class="setting-label">Auth token</span>
			{#if serverAuthSet}
				<div class="setting-input-row">
					<span class="dim-text">Configured</span>
					<button class="setting-btn setting-btn-danger" onclick={clearServerAuth} disabled={serverSaving}>Remove</button>
				</div>
			{:else}
				<div class="setting-input-row">
					<input class="setting-input" type="password" placeholder="optional — protects your API" aria-label="Auth token" bind:value={serverAuthInput} />
					{#if serverAuthInput.trim()}
						<button class="setting-btn" onclick={saveServerAuth} disabled={serverSaving}>
							{serverSaving ? "..." : "Set"}
						</button>
					{/if}
				</div>
			{/if}
			<p class="setting-hint">API token for automation, the CLI and the desktop app. Browsers pair for a session instead (see <a class="settings-link" href={`/${slug}/settings/connections`}>Connections</a>) and keep working when it changes. Leave empty for no authentication.</p>
		</div>

		<div class="setting-row">
			<span class="setting-label">Host</span>
			<span class="dim-text">{serverHost}</span>
		</div>
	{/if}
</section>

<!-- Updates -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Updates <span class="owner-badge">This server</span></h3>
			<p class="section-desc">Version, release channel, and what changed.</p>
		</div>
	</div>

	<div class="setting-row">
		<span class="setting-label">Version</span>
		<span class="dim-text">{version ? `v${version}` : "…"}{commit && commit !== "dev" ? ` · ${commit.slice(0, 7)}` : ""}</span>
	</div>

	<div class="setting-row">
		<span class="setting-label" id="update-channel-label">Release channel</span>
		<Select.Root type="single" value={channel} onValueChange={changeChannel} disabled={channelSaving}>
			<Select.Trigger aria-labelledby="update-channel-label" class="h-11 w-40"><span>{channel === "nightly" ? "Nightly" : "Stable"}</span></Select.Trigger>
			<Select.Content class="z-[210]"><Select.Item value="stable" class="min-h-11">Stable</Select.Item><Select.Item value="nightly" class="min-h-11">Nightly</Select.Item></Select.Content>
		</Select.Root>
	</div>

	{#if changelog.length > 0}
		<div class="setting-row">
			<button class="setting-btn" onclick={() => (showChangelog = !showChangelog)} aria-expanded={showChangelog}>
				{showChangelog ? "Hide what’s new" : "What’s new"}
			</button>
			{#if showChangelog}
				<div class="changelog-list">
					{#each changelog.slice(0, 5) as entry (entry.version)}
						<div class="changelog-entry">
							<div class="changelog-version">{entry.version}</div>
							<div class="changelog-body">{@html DOMPurify.sanitize(changelogMd.parse(entry.body) as string)}</div>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</section>

<!-- Voice -->
<section class="settings-section">
	<div class="section-header">
		<div class="section-icon" aria-hidden="true"><AudioLines size={20} strokeWidth={1.75} /></div>
		<div>
			<h3 class="section-label">Voice <span class="owner-badge">Your companion</span></h3>
			<p class="section-desc">ElevenLabs voice ID for text-to-speech. Leave empty to use the default voice.</p>
		</div>
	</div>

	{#if voiceLoading}
		<div class="ext-loading"><div class="loading-dot"></div></div>
	{:else}
		<div class="gh-token-form">
			<label for="voice-id" class="setting-label">ElevenLabs voice ID</label>
			<input class="ext-input" type="text" id="voice-id" placeholder="e.g. TWutjvRaJqAX89preB4e" bind:value={voiceInput} onkeydown={(e) => e.key === "Enter" && saveVoice()} />
			<div class="ext-form-actions">
				<button class="ext-form-btn ext-form-add" disabled={voiceSaving || voiceInput.trim() === voiceId} onclick={saveVoice}>
					{voiceSaving ? "Saving..." : "Save"}
				</button>
				{#if voiceId}
					<button class="ext-form-btn ext-form-cancel" disabled={voiceSaving} onclick={clearVoice}>reset to default</button>
				{/if}
			</div>
		</div>
	{/if}
</section>

<!-- Email (SMTP/IMAP) -->
<section class="settings-section">
	<div class="section-header">
		<div class="section-icon" aria-hidden="true"><Mail size={20} strokeWidth={1.75} /></div>
		<div>
			<h3 class="section-label">Email <span class="owner-badge">Your companion</span></h3>
			<p class="section-desc">Connect any email via SMTP/IMAP (iCloud, Outlook, Yahoo, etc.) to enable send and read email tools.</p>
		</div>
	</div>

	{#if emailLoading}
		<div class="ext-loading"><div class="loading-dot"></div></div>
	{:else}
		{#if emailAccounts.length > 0}
			<div class="accounts-list">
				{#each emailAccounts as acct, i (acct.smtp_from ?? acct.smtp_user ?? acct.imap_user ?? i)}
					<div class="account-row">
						<span class="account-email">{acct.smtp_from || acct.smtp_user || acct.imap_user || "account"}</span>
						<button class="ext-remove-btn" disabled={emailSaving} onclick={() => removeEmailAccount(i)}>
							{emailSaving ? "..." : "Remove"}
						</button>
					</div>
				{/each}
			</div>
		{/if}

		{#if emailAdding}
			<div class="email-form">
				<div class="email-form-group">
					<span class="email-form-label">Outgoing (SMTP)</span>
					<label class="integration-field">SMTP host<input class="ext-input" type="text" placeholder="smtp host (e.g. smtp.mail.me.com)" bind:value={emailForm.smtp_host} /></label>
					<div class="email-form-row">
						<label class="integration-field">Port<input class="ext-input" type="number" placeholder="port" bind:value={emailForm.smtp_port} style="width: 5rem;" /></label>
						<label class="integration-field">Username or email<input class="ext-input" style="flex:1" type="text" placeholder="username / email" bind:value={emailForm.smtp_user} /></label>
					</div>
					<label class="integration-field">Password or app password<input class="ext-input" type="password" placeholder="password / app-specific password" bind:value={emailForm.smtp_password} /></label>
					<label class="integration-field">From address<input class="ext-input" type="email" placeholder="from address (e.g. user@icloud.com)" bind:value={emailForm.smtp_from} /></label>
				</div>

				<div class="email-form-group">
					<span class="email-form-label">Incoming (IMAP)</span>
					<label class="integration-field">IMAP host<input class="ext-input" type="text" placeholder="imap host (e.g. imap.mail.me.com)" bind:value={emailForm.imap_host} /></label>
					<div class="email-form-row">
						<label class="integration-field">Port<input class="ext-input" type="number" placeholder="port" bind:value={emailForm.imap_port} style="width: 5rem;" /></label>
						<label class="integration-field">Username or email<input class="ext-input" style="flex:1" type="text" placeholder="username / email" bind:value={emailForm.imap_user} /></label>
					</div>
					<label class="integration-field">Password or app password<input class="ext-input" type="password" placeholder="password / app-specific password" bind:value={emailForm.imap_password} /></label>
				</div>

				<div class="ext-form-actions">
					<button class="ext-form-btn ext-form-add" disabled={emailSaving || (!emailForm.smtp_host && !emailForm.imap_host)} onclick={saveNewEmail}>
						{emailSaving ? "Saving..." : "Add account"}
					</button>
					<button class="ext-form-btn ext-form-cancel" onclick={() => { emailAdding = false; emailError = ""; emailForm = emptyEmailForm(); }}>Cancel</button>
				</div>
			</div>
		{:else}
			<button class="ext-form-btn ext-form-add" onclick={() => (emailAdding = true)}>+ add email account</button>
		{/if}
	{/if}

	{#if emailError}
		<p class="error-msg" role="alert">{emailError}</p>
	{/if}
</section>

<!-- GitHub -->
<section class="settings-section">
	<div class="section-header">
		<div class="section-icon" aria-hidden="true"><Github size={20} strokeWidth={1.75} /></div>
		<div>
			<h3 class="section-label">GitHub <span class="owner-badge">This server</span></h3>
			<p class="section-desc">Connect GitHub to enable cloning repos, creating branches, PRs, and managing issues.</p>
		</div>
	</div>

	{#if ghLoading}
		<div class="ext-loading"><div class="loading-dot"></div></div>
	{:else if ghConfigured && !ghEditing}
		<div class="gh-status">
			<div class="gh-status-info">
				<span class="gh-status-dot"></span>
				<span class="gh-status-text">Token configured</span>
			</div>
			<div class="gh-status-actions">
				<button class="ext-form-btn ext-form-cancel" onclick={() => (ghEditing = true)}>Change</button>
				<button class="ext-remove-btn" disabled={ghSaving} onclick={disconnectGithub}>{ghSaving ? "..." : "Remove"}</button>
			</div>
		</div>
	{:else}
		<div class="gh-token-form">
			<label for="github-token" class="setting-label">GitHub access token</label>
			<input class="ext-input" type="password" id="github-token" placeholder="ghp_... or github_pat_..." bind:value={ghToken} onkeydown={(e) => e.key === "Enter" && saveGithubToken()} />
			<div class="ext-form-actions">
				<button class="ext-form-btn ext-form-add" disabled={ghSaving || !ghToken.trim()} onclick={saveGithubToken}>
					{ghSaving ? "Saving..." : "Save token"}
				</button>
				{#if ghEditing}
					<button class="ext-form-btn ext-form-cancel" onclick={() => { ghEditing = false; ghToken = ""; ghError = ""; }}>Cancel</button>
				{/if}
			</div>
		</div>
	{/if}

	{#if ghError}
		<p class="error-msg" role="alert">{ghError}</p>
	{/if}
</section>
