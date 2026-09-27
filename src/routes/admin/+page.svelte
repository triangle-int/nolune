<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidate } from '$app/navigation';
	import BoxIcon from '@lucide/svelte/icons/box';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import TerminalIcon from '@lucide/svelte/icons/terminal';
	import StarIcon from '@lucide/svelte/icons/star';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Rich from '$lib/components/Rich.svelte';
	import TopBar from '$lib/components/TopBar.svelte';
	import AddModelForm from '$lib/components/admin/AddModelForm.svelte';
	import CopyButton from '$lib/components/chat/CopyButton.svelte';
	import { formatTokens } from '$lib/format';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	let { data, form } = $props();
	const { m } = getI18n();
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
	/** Who Codex is signed in as; null while a sign-in waits for its code, or when it isn't. */
	const chatgptSignedIn = $derived(chatgpt.status?.signedIn ?? null);
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

	/** A problem's first sentence: the rest says how to sign in, which the buttons do here. */
	function firstSentence(text: string): string {
		return text.split('. ')[0].replace(/\.?$/, '.');
	}

	function sourceText(key: KeyStatus): string {
		if (key.source === 'config') return m.admin.savedInBtw(key.hint);
		if (key.source === 'env') return m.admin.fromEnv(key.env, key.hint);
		return m.admin.notSet;
	}
</script>

<div class="flex h-full flex-col">
	<TopBar />
	<main class="min-h-0 flex-1 overflow-y-auto">
		<div class="mx-auto max-w-2xl space-y-8 px-4 py-8 sm:py-12">
			<h1 class="text-2xl font-semibold">{m.admin.title}</h1>

			<section class="space-y-3" aria-labelledby="keys-heading">
				<div class="space-y-1">
					<h2 id="keys-heading" class="text-lg font-medium">{m.admin.keys}</h2>
					<p class="text-muted-foreground">
						{m.admin.keysHint}
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
									<div class="text-muted-foreground">{m.admin.purposes[key.provider]}</div>
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
											{m.admin.replace}
										</Button>
									{/if}
									{#if key.source === 'config'}
										<Button
											variant="ghost"
											size="sm"
											class="text-muted-foreground"
											onclick={() => (removing = key)}
										>
											{m.common.remove}
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
											placeholder={m.admin.pasteKey(key.label)}
											aria-label={m.admin.keyLabel(key.label)}
											class="h-10 flex-1 rounded-full px-4 font-mono placeholder:font-sans"
										/>
										<div class="flex gap-2">
											<Button type="submit" disabled={busy} class="h-10 px-5 max-sm:flex-1">
												{busy ? m.admin.checkingKey : m.common.save}
											</Button>
											{#if key.source || key.envSet}
												<Button
													type="button"
													variant="ghost"
													class="h-10"
													onclick={() => (editing = null)}
												>
													{m.common.cancel}
												</Button>
											{/if}
										</div>
									</div>
									<p class="text-muted-foreground">
										<Rich text={m.admin.makeOneAt}>
											{#snippet link()}<a
													href={key.consoleUrl}
													target="_blank"
													rel="noreferrer"
													class="underline">{new URL(key.consoleUrl).host}</a
												>{/snippet}
										</Rich>
										{#if key.source}
											{key.provider === 'openai' ? m.admin.sameProject : m.admin.sameWorkspace}
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
					<h2 id="plans-heading" class="text-lg font-medium">{m.admin.plans}</h2>
					<p class="text-muted-foreground">
						{m.admin.plansHint}
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
								<div class="font-medium">{m.admin.plan}</div>
								<div class="text-muted-foreground">{m.admin.claudePlanAbout}</div>
								{#if data.claude.installed}
									<div class="truncate font-mono text-muted-foreground">{data.claude.path}</div>
								{:else if data.claude.path}
									<div class="text-warning">
										<Rich text={m.admin.notAt}>
											{#snippet path()}<span class="font-mono">{data.claude.path}</span>{/snippet}
											{#snippet command()}<code>btw config set claude-path</code>{/snippet}
										</Rich>
									</div>
								{:else}
									<div class="text-warning">{m.admin.notInstalled}</div>
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
									{checkingPlan ? m.common.checking : m.admin.checkSignIn}
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
									<Rich text={m.admin.install}>
										{#snippet setup()}<code>btw claude-plan setup</code>{/snippet}
										{#snippet claude()}<code>claude</code>{/snippet}
									</Rich>
								</p>
								<div class="flex items-center gap-1 rounded-xl bg-muted py-1 pr-1 pl-3">
									<code class="min-w-0 flex-1 truncate font-mono text-foreground"
										>{data.claude.installCommand}</code
									>
									<CopyButton text={data.claude.installCommand} label={m.admin.copyCommand} />
								</div>
							</div>
						{:else}
							<p class="text-muted-foreground sm:pl-12">
								<Rich text={m.admin.signIn}>
									{#snippet setup()}<code>btw claude-plan setup</code>{/snippet}
									{#snippet claude()}<code>claude</code>{/snippet}
									{#snippet login()}<code>/login</code>{/snippet}
								</Rich>
							</p>
						{/if}
					</li>

					<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
						<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
							<span
								class={cn(
									'flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted max-sm:self-start',
									chatgptSignedIn ? 'text-foreground' : 'text-muted-foreground'
								)}
							>
								<TerminalIcon class="size-4" />
							</span>
							<div class="min-w-0 flex-1">
								<div class="font-medium">{m.admin.chatgptPlan}</div>
								<div class="text-muted-foreground">{m.admin.chatgptPlanAbout}</div>
								{#if chatgpt.installed}
									<div class="truncate font-mono text-muted-foreground">{chatgpt.path}</div>
									{#if chatgptSignedIn}
										<div class="break-words text-muted-foreground">{sentence(chatgptSignedIn)}</div>
									{:else if chatgpt.status?.problem}
										<div class="text-warning">{firstSentence(chatgpt.status.problem)}</div>
									{/if}
								{:else if chatgpt.path}
									<div class="text-warning">
										<Rich text={m.admin.notAt}>
											{#snippet path()}<span class="font-mono">{chatgpt.path}</span>{/snippet}
											{#snippet command()}<code>btw config set codex-path</code>{/snippet}
										</Rich>
									</div>
								{:else}
									<div class="text-warning">{m.admin.notInstalled}</div>
								{/if}
							</div>
							{#if chatgpt.installed}
								<div class="flex gap-1 max-sm:basis-full max-sm:pl-9">
									{#if chatgpt.pending}
										<form method="POST" action="?/chatgptCancel" use:enhance>
											<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground">
												{m.common.cancel}
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
												variant={chatgptSignedIn ? 'ghost' : 'default'}
												size="sm"
												disabled={signingIn}
												class={cn(chatgptSignedIn && 'text-muted-foreground')}
											>
												{signingIn
													? m.admin.chatgptAsking
													: chatgptSignedIn
														? m.admin.chatgptSignInAgain
														: m.admin.chatgptSignIn}
											</Button>
										</form>
										{#if chatgptSignedIn}
											<Button
												variant="ghost"
												size="sm"
												class="text-muted-foreground"
												onclick={() => (signingOut = true)}
											>
												{m.admin.signOut}
											</Button>
										{/if}
									{/if}
								</div>
							{/if}
						</div>

						{#if chatgpt.pending}
							{@const url = new URL(chatgpt.pending.verificationUrl)}
							<ol class="list-inside list-decimal space-y-2 sm:pl-12" aria-live="polite">
								<li>
									<Rich text={m.admin.chatgptOpen}>
										{#snippet link()}<a
												href={chatgpt.pending?.verificationUrl}
												target="_blank"
												rel="noreferrer"
												class="underline">{url.host}{url.pathname}</a
											>{/snippet}
									</Rich>
								</li>
								<li>
									<Rich text={m.admin.chatgptCode}>
										{#snippet code()}<span
												class="ml-1 font-mono text-lg font-medium tracking-widest select-all"
												>{chatgpt.pending?.userCode}</span
											>{/snippet}
									</Rich>
								</li>
							</ol>
							<p class="text-muted-foreground sm:pl-12">{m.admin.chatgptCodeHint}</p>
						{:else if chatgptError}
							<p class="text-destructive sm:pl-12" role="alert">{chatgptError}</p>
						{:else if chatgptResult?.planMessage}
							<p class="text-muted-foreground sm:pl-12" role="status">
								{chatgptResult.planMessage}
							</p>
						{:else if !chatgpt.installed}
							<div class="space-y-2 text-muted-foreground sm:pl-12">
								<p>
									<Rich text={m.admin.chatgptInstall}>
										{#snippet setup()}<code>btw chatgpt-plan setup</code>{/snippet}
									</Rich>
								</p>
								<div class="flex items-center gap-1 rounded-xl bg-muted py-1 pr-1 pl-3">
									<code class="min-w-0 flex-1 truncate font-mono text-foreground"
										>{chatgpt.installCommand}</code
									>
									<CopyButton text={chatgpt.installCommand} label={m.admin.copyCommand} />
								</div>
							</div>
						{/if}
					</li>
				</ul>
			</section>

			<section class="space-y-3" aria-labelledby="models-heading">
				<div class="space-y-1">
					<h2 id="models-heading" class="text-lg font-medium">{m.admin.models}</h2>
					<p class="text-muted-foreground">
						{m.admin.modelsHint}
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
										<Badge variant="secondary">{m.common.default}</Badge>
									{/if}
								</div>
								<div class="truncate text-muted-foreground">
									{m.admin.presetDetails(
										preset.provider,
										preset.model,
										formatTokens(preset.contextWindow),
										preset.overridden
									)}
								</div>
							</div>
							{#if !preset.isDefault}
								<form method="POST" action="?/setDefault" use:enhance>
									<input type="hidden" name="id" value={preset.id} />
									<Button
										type="submit"
										variant="ghost"
										size="sm"
										title={m.admin.makeDefault}
										class="text-muted-foreground max-sm:px-2"
									>
										<StarIcon />
										<span class="max-sm:sr-only">{m.admin.makeDefault}</span>
									</Button>
								</form>
							{/if}
							<form method="POST" action="?/remove" use:enhance>
								<input type="hidden" name="id" value={preset.id} />
								<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground"
									>{m.common.remove}</Button
								>
							</form>
						</li>
					{:else}
						<li class="px-4 py-3 text-sm text-muted-foreground">{m.admin.noPresets}</li>
					{/each}
				</ul>

				<AddModelForm
					providers={data.providers}
					keys={data.keys}
					claudeInstalled={data.claude.installed}
					codexInstalled={data.chatgpt.installed}
					problem={form?.addError}
					startOpen={data.presets.length === 0}
				/>
			</section>
		</div>
	</main>
</div>

<AlertDialog.Root open={removing !== null} onOpenChange={(open) => !open && (removing = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.admin.removeKeyTitle(removing?.label ?? '')}</AlertDialog.Title>
			<AlertDialog.Description>
				{#if removing?.envSet}
					{m.admin.useEnvInstead(removing.env)}
				{:else if removing}
					{m.admin.withoutIt[removing.provider]}
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
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive"
					>{m.common.remove}</AlertDialog.Action
				>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>

<AlertDialog.Root open={signingOut} onOpenChange={(open) => !open && (signingOut = false)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.admin.chatgptSignOutTitle}</AlertDialog.Title>
			<AlertDialog.Description>{m.admin.chatgptSignOutBody}</AlertDialog.Description>
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
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive"
					>{m.admin.signOut}</AlertDialog.Action
				>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
