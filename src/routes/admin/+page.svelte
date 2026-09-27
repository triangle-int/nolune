<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidate } from '$app/navigation';
	import BoxIcon from '@lucide/svelte/icons/box';
	import CircleUserRoundIcon from '@lucide/svelte/icons/circle-user-round';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import TerminalIcon from '@lucide/svelte/icons/terminal';
	import StarIcon from '@lucide/svelte/icons/star';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import TopBar from '$lib/components/TopBar.svelte';
	import CopyButton from '$lib/components/chat/CopyButton.svelte';
	import { formatTokens } from '$lib/format';
	import { cn } from '$lib/utils';

	let { data, form } = $props();
	let adding = $state(false);
	/** Whose model the new preset runs. */
	let provider = $state('anthropic');
	const EXAMPLE_MODELS: Record<string, string> = {
		anthropic: 'claude-opus-5-5',
		openai: 'gpt-6-astra',
		'claude-plan': 'claude-opus-5-5',
		'chatgpt-plan': 'gpt-6-astra'
	};
	/** Whether the Claude Code sign-in is being checked. */
	let checkingPlan = $state(false);

	type KeyStatus = (typeof data.keys)[number];
	/** The key being pasted, the one being checked, and the one about to be removed. */
	let editing = $state<string | null>(null);
	let checking = $state<string | null>(null);
	let removing = $state<KeyStatus | null>(null);

	/** A ChatGPT sign-in being started, and one about to be signed out. */
	let signingIn = $state(false);
	let signingOut = $state(false);
	const chatgpt = $derived(data.chatgpt);
	/** What the last plan action said, for the row of the plan it was about. */
	const claudeResult = $derived(form?.plan === 'claude-plan' ? form : null);
	const chatgptResult = $derived(form?.plan === 'chatgpt-plan' ? form : null);
	const chatgptError = $derived(chatgptResult?.planError ?? chatgpt.signInError);

	// The code is entered on another page, often another device: ask until it has been.
	$effect(() => {
		if (!chatgpt.pending) return;
		const timer = setInterval(() => invalidate('btw:chatgpt-plan'), 3000);
		return () => clearInterval(timer);
	});

	/** "signed in as …" at the start of a line. */
	function sentence(text: string): string {
		return `${text[0].toUpperCase()}${text.slice(1)}`;
	}

	function sourceText(key: KeyStatus): string {
		const end = key.hint ? ` ending in ${key.hint}` : '';
		if (key.source === 'config') return `Saved in btw${end}`;
		if (key.source === 'env') return `From the ${key.env} environment variable${end}`;
		return 'Not set';
	}
</script>

<div class="flex h-full flex-col">
	<TopBar />
	<main class="min-h-0 flex-1 overflow-y-auto">
		<div class="mx-auto max-w-2xl space-y-8 px-4 py-8 sm:py-12">
			<h1 class="text-2xl font-semibold">Models & keys</h1>

			<section class="space-y-3" aria-labelledby="keys-heading">
				<div class="space-y-1">
					<h2 id="keys-heading" class="text-lg font-medium">API keys</h2>
					<p class="text-muted-foreground">
						Shared by every profile and kept in btw's config file on this computer. A new key is
						checked with its provider before it's saved, and used right away.
					</p>
				</div>
				<ul class="overflow-hidden rounded-2xl border">
					{#each data.keys as key (key.provider)}
						{@const open = editing === key.provider || (!key.source && !key.envSet)}
						{@const busy = checking === key.provider}
						{@const result = form?.provider === key.provider ? form : null}
						<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
							<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
								<span
									class={cn(
										'flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted max-sm:self-start',
										key.source ? 'text-foreground' : 'text-muted-foreground'
									)}
								>
									<KeyRoundIcon class="size-4" />
								</span>
								<div class="min-w-0 flex-1">
									<div class="font-medium">{key.label}</div>
									<div class="text-muted-foreground">{key.purpose}</div>
									<div class={cn(key.source ? 'text-muted-foreground' : 'text-warning')}>
										{sourceText(key)}
									</div>
								</div>
								<!-- Under the text on phones, so it keeps the width. -->
								<div class="flex gap-1 max-sm:basis-full max-sm:pl-9">
									{#if !open}
										<Button
											variant="ghost"
											size="sm"
											class="text-muted-foreground"
											onclick={() => (editing = key.provider)}
										>
											Replace
										</Button>
									{/if}
									{#if key.source === 'config'}
										<Button
											variant="ghost"
											size="sm"
											class="text-muted-foreground"
											onclick={() => (removing = key)}
										>
											Remove
										</Button>
									{/if}
								</div>
							</div>

							{#if open}
								<form
									method="POST"
									action="?/saveKey"
									class="space-y-2 sm:pl-12"
									use:enhance={() => {
										checking = key.provider;
										return async ({ result, update }) => {
											await update();
											checking = null;
											if (result.type === 'success') editing = null;
										};
									}}
								>
									<input type="hidden" name="provider" value={key.provider} />
									<div class="flex flex-col gap-2 sm:flex-row">
										<Input
											name="key"
											type="password"
											required
											autocomplete="off"
											spellcheck="false"
											placeholder={`Paste the ${key.label} API key`}
											aria-label={`${key.label} API key`}
											class="h-10 flex-1 rounded-full px-4 font-mono placeholder:font-sans"
										/>
										<div class="flex gap-2">
											<Button type="submit" disabled={busy} class="h-10 px-5 max-sm:flex-1">
												{busy ? 'Checking the key…' : 'Save'}
											</Button>
											{#if key.source || key.envSet}
												<Button
													type="button"
													variant="ghost"
													class="h-10"
													onclick={() => (editing = null)}
												>
													Cancel
												</Button>
											{/if}
										</div>
									</div>
									<p class="text-muted-foreground">
										Make one at
										<a href={key.consoleUrl} target="_blank" rel="noreferrer" class="underline"
											>{new URL(key.consoleUrl).host}</a
										>.
										{#if key.source}
											Use a key from the same {key.provider === 'openai' ? 'project' : 'workspace'}:
											pictures and PDFs already sent in chats live there, and those chats can't go
											on without them.
										{/if}
									</p>
								</form>
							{/if}

							{#if result?.keyError && open}
								<p class="text-destructive sm:pl-12" role="alert">{result.keyError}</p>
							{:else if result?.keyMessage && !open}
								<p class="text-muted-foreground sm:pl-12" role="status">{result.keyMessage}</p>
							{/if}
						</li>
					{/each}
				</ul>
			</section>

			<section class="space-y-3" aria-labelledby="plans-heading">
				<div class="space-y-1">
					<h2 id="plans-heading" class="text-lg font-medium">Plans</h2>
					<p class="text-muted-foreground">
						Chats on a plan preset run on someone's own subscription instead of an API key. Plan
						limits assume one person's ordinary use, so keep busy automations and subagents on an
						API key preset.
					</p>
				</div>
				<ul class="overflow-hidden rounded-2xl border">
					<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
						<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
							<span
								class={cn(
									'flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted max-sm:self-start',
									data.claude.installed ? 'text-foreground' : 'text-muted-foreground'
								)}
							>
								<TerminalIcon class="size-4" />
							</span>
							<div class="min-w-0 flex-1">
								<div class="font-medium">Claude plan</div>
								<div class="text-muted-foreground">
									Pro or Max, through Claude Code on this computer. btw runs it and never sees its
									sign-in.
								</div>
								{#if data.claude.installed}
									<div class="truncate font-mono text-muted-foreground">{data.claude.path}</div>
								{:else if data.claude.path}
									<div class="text-warning">
										Not at <span class="font-mono">{data.claude.path}</span>, where
										<code>btw config set claude-path</code> says it is.
									</div>
								{:else}
									<div class="text-warning">Claude Code isn't installed on this computer.</div>
								{/if}
							</div>
							<form
								method="POST"
								action="?/checkPlan"
								class="max-sm:basis-full max-sm:pl-9"
								use:enhance={() => {
									checkingPlan = true;
									return async ({ update }) => {
										await update();
										checkingPlan = false;
									};
								}}
							>
								<Button
									type="submit"
									variant="ghost"
									size="sm"
									class="text-muted-foreground"
									disabled={checkingPlan}
								>
									{checkingPlan ? 'Checking…' : 'Check sign-in'}
								</Button>
							</form>
						</div>
						{#if claudeResult?.planError}
							<p class="text-destructive sm:pl-12" role="alert">{claudeResult.planError}</p>
						{:else if claudeResult?.planMessage}
							<p class="text-muted-foreground sm:pl-12" role="status">
								{claudeResult.planMessage}
							</p>
						{:else if !data.claude.installed}
							<div class="space-y-2 text-muted-foreground sm:pl-12">
								<p>
									In a terminal on this computer, run <code>btw claude-plan setup</code>: it
									installs Claude Code with Anthropic's installer and signs it in to your Claude
									account, asking first. Or install it yourself, then run <code>claude</code> and sign
									in:
								</p>
								<div class="flex items-center gap-1 rounded-xl bg-muted py-1 pr-1 pl-3">
									<code class="min-w-0 flex-1 truncate font-mono text-foreground"
										>{data.claude.installCommand}</code
									>
									<CopyButton text={data.claude.installCommand} label="Copy the command" />
								</div>
							</div>
						{:else}
							<p class="text-muted-foreground sm:pl-12">
								To sign in, run <code>btw claude-plan setup</code> in a terminal on this computer,
								or run <code>claude</code> there and use <code>/login</code> with your Claude account.
							</p>
						{/if}
					</li>

					<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
						<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
							<span
								class={cn(
									'flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted max-sm:self-start',
									chatgpt.signedIn ? 'text-foreground' : 'text-muted-foreground'
								)}
							>
								<CircleUserRoundIcon class="size-4" />
							</span>
							<div class="min-w-0 flex-1">
								<div class="font-medium">ChatGPT plan</div>
								<div class="text-muted-foreground">
									Plus, Pro or Business, through Codex's backend, the way OpenAI's Codex does. btw
									keeps the sign-in.
								</div>
								{#if chatgpt.signedIn}
									<div class="break-words text-muted-foreground">{sentence(chatgpt.signedIn)}</div>
								{:else}
									<div class="text-warning">Not signed in</div>
								{/if}
							</div>
							<div class="flex gap-1 max-sm:basis-full max-sm:pl-9">
								{#if chatgpt.pending}
									<form method="POST" action="?/chatgptCancel" use:enhance>
										<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground">
											Cancel
										</Button>
									</form>
								{:else}
									<form
										method="POST"
										action="?/chatgptSignIn"
										use:enhance={() => {
											signingIn = true;
											return async ({ update }) => {
												await update();
												signingIn = false;
											};
										}}
									>
										<Button
											type="submit"
											variant={chatgpt.signedIn ? 'ghost' : 'default'}
											size="sm"
											disabled={signingIn}
											class={cn(chatgpt.signedIn && 'text-muted-foreground')}
										>
											{signingIn
												? 'Asking ChatGPT…'
												: chatgpt.signedIn
													? 'Sign in again'
													: 'Sign in with ChatGPT'}
										</Button>
									</form>
									{#if chatgpt.signedIn}
										<Button
											variant="ghost"
											size="sm"
											class="text-muted-foreground"
											onclick={() => (signingOut = true)}
										>
											Sign out
										</Button>
									{/if}
								{/if}
							</div>
						</div>

						{#if chatgpt.pending}
							<ol class="list-inside list-decimal space-y-2 sm:pl-12" aria-live="polite">
								<li>
									Open
									<a
										href={chatgpt.pending.verificationUrl}
										target="_blank"
										rel="noreferrer"
										class="underline"
										>{new URL(chatgpt.pending.verificationUrl).host}{new URL(
											chatgpt.pending.verificationUrl
										).pathname}</a
									>
									on any device and sign in to ChatGPT.
								</li>
								<li>
									Enter this code:
									<span class="ml-1 font-mono text-lg font-medium tracking-widest select-all"
										>{chatgpt.pending.userCode}</span
									>
								</li>
							</ol>
							<p class="text-muted-foreground sm:pl-12">
								The code works for 15 minutes. This page updates once it's entered.
							</p>
						{:else if chatgptError}
							<p class="text-destructive sm:pl-12" role="alert">{chatgptError}</p>
						{:else if chatgptResult?.planMessage}
							<p class="text-muted-foreground sm:pl-12" role="status">
								{chatgptResult.planMessage}
							</p>
						{/if}
					</li>
				</ul>
			</section>

			<section class="space-y-3" aria-labelledby="models-heading">
				<div class="space-y-1">
					<h2 id="models-heading" class="text-lg font-medium">Models</h2>
					<p class="text-muted-foreground">
						Presets are shared by every profile. New chats start with the default one. Removing a
						preset doesn't affect existing chats.
					</p>
				</div>

				{#if form?.message}
					<p class="rounded-2xl bg-muted px-4 py-3 text-sm">{form.message}</p>
				{/if}

				<ul class="overflow-hidden rounded-2xl border">
					{#each data.presets as preset (preset.id)}
						<li class="flex items-center gap-3 border-b px-4 py-3 text-sm last:border-b-0">
							<span
								class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground"
							>
								<BoxIcon class="size-4" />
							</span>
							<div class="min-w-0 flex-1">
								<div class="flex items-center gap-2">
									<span class="truncate font-medium">{preset.name}</span>
									{#if preset.isDefault}
										<Badge variant="secondary">Default</Badge>
									{/if}
								</div>
								<div class="truncate text-muted-foreground">
									{preset.provider} / {preset.model} · context {formatTokens(
										preset.contextWindow
									)}{preset.overridden ? ' (override)' : ''}
								</div>
							</div>
							{#if !preset.isDefault}
								<form method="POST" action="?/setDefault" use:enhance>
									<input type="hidden" name="id" value={preset.id} />
									<Button
										type="submit"
										variant="ghost"
										size="sm"
										title="Make default"
										class="text-muted-foreground max-sm:px-2"
									>
										<StarIcon />
										<span class="max-sm:sr-only">Make default</span>
									</Button>
								</form>
							{/if}
							<form method="POST" action="?/remove" use:enhance>
								<input type="hidden" name="id" value={preset.id} />
								<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground"
									>Remove</Button
								>
							</form>
						</li>
					{:else}
						<li class="px-4 py-3 text-sm text-muted-foreground">No presets yet.</li>
					{/each}
				</ul>

				<form
					method="POST"
					action="?/add"
					class="space-y-3"
					use:enhance={() => {
						adding = true;
						return async ({ update }) => {
							await update();
							adding = false;
						};
					}}
				>
					<div class="flex flex-wrap items-center justify-between gap-3">
						<h2 class="font-medium">Add a preset</h2>
						<ToggleGroup.Root
							type="single"
							variant="outline"
							size="sm"
							value={provider}
							onValueChange={(value) => value && (provider = value)}
							aria-label="Provider"
						>
							{#each data.providers as p (p.id)}
								<ToggleGroup.Item value={p.id}>{p.label}</ToggleGroup.Item>
							{/each}
						</ToggleGroup.Root>
					</div>
					<input type="hidden" name="provider" value={provider} />
					<Input
						name="model"
						required
						placeholder="Model id, e.g. {EXAMPLE_MODELS[provider] ?? ''}"
						aria-label="Model id"
						class="h-10 rounded-full px-4"
					/>
					<div class="flex flex-col gap-3 sm:flex-row">
						<Input
							name="name"
							placeholder="Name (default: model + provider)"
							aria-label="Name"
							class="h-10 flex-1 rounded-full px-4"
						/>
						<Input
							name="contextWindow"
							type="number"
							min="1"
							placeholder={provider === 'openai'
								? 'Context window (flagships: known)'
								: provider === 'claude-plan'
									? 'Context window (not reported)'
									: 'Context window (optional)'}
							aria-label="Context window"
							class="h-10 rounded-full px-4 sm:w-60"
						/>
					</div>
					<Button type="submit" disabled={adding} class="h-10 px-5">
						{adding
							? provider === 'claude-plan'
								? 'Checking Claude Code…'
								: 'Checking the model…'
							: 'Add'}
					</Button>
				</form>
			</section>
		</div>
	</main>
</div>

<AlertDialog.Root open={removing !== null} onOpenChange={(open) => !open && (removing = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Remove the {removing?.label} key?</AlertDialog.Title>
			<AlertDialog.Description>
				{#if removing?.envSet}
					btw will use the key in the {removing.env} environment variable instead.
				{:else}
					{removing?.withoutIt}
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/removeKey"
			use:enhance={() => {
				return async ({ update }) => {
					removing = null;
					await update();
				};
			}}
		>
			<input type="hidden" name="provider" value={removing?.provider ?? ''} />
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">Cancel</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive">Remove</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>

<AlertDialog.Root open={signingOut} onOpenChange={(open) => !open && (signingOut = false)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Sign out of ChatGPT?</AlertDialog.Title>
			<AlertDialog.Description>
				Chats on ChatGPT plan presets stop working until someone signs in again.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/chatgptSignOut"
			use:enhance={() => {
				return async ({ update }) => {
					signingOut = false;
					await update();
				};
			}}
		>
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">Cancel</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive">Sign out</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
