<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidate } from '$app/navigation';
	import BoxIcon from '@lucide/svelte/icons/box';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import MessageCircleIcon from '@lucide/svelte/icons/message-circle';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import TerminalIcon from '@lucide/svelte/icons/terminal';
	import StarIcon from '@lucide/svelte/icons/star';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Rich from '$lib/components/Rich.svelte';
	import TopBar from '$lib/components/TopBar.svelte';
	import AddModelForm from '$lib/components/admin/AddModelForm.svelte';
	import AddCustomProvider from '$lib/components/admin/AddCustomProvider.svelte';
	import CommandSafety from '$lib/components/admin/CommandSafety.svelte';
	import IdleCompaction from '$lib/components/admin/IdleCompaction.svelte';
	import CustomProviderRow from '$lib/components/admin/CustomProviderRow.svelte';
	import MemorySearch from '$lib/components/admin/MemorySearch.svelte';
	import PresetForm from '$lib/components/admin/PresetForm.svelte';
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
	/** The preset being edited. */
	let editingPreset = $state<string | null>(null);

	/**
	 * A ChatGPT sign-in being started, finished with a pasted address, and signed out; and the
	 * sign-in being checked with OpenAI.
	 */
	let signingIn = $state(false);
	let finishing = $state(false);
	let signingOut = $state(false);
	let checkingChatgpt = $state(false);
	const chatgpt = $derived(data.chatgpt);
	/** Who's signed in with ChatGPT; null when nobody is. */
	const chatgptSignedIn = $derived(chatgpt.status?.signedIn ?? null);
	/** What the last plan action said, for the row of the plan it was about. */
	const claudeResult = $derived(form?.plan === 'claude-plan' ? form : null);
	const chatgptResult = $derived(form?.plan === 'chatgpt-plan' ? form : null);
	const chatgptError = $derived(chatgptResult?.planError ?? chatgpt.signInError);

	// The sign-in finishes in a browser, on this computer or another device: ask until it has.
	$effect(() => {
		if (!chatgpt.pending) return;
		const timer = setInterval(() => invalidate('nolune:chatgpt-plan'), 3000);
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
		if (key.source === 'config') return m.admin.savedInNolune(key.hint);
		if (key.source === 'env') return m.admin.fromEnv(key.env, key.hint);
		return key.optional ? m.admin.notSetFree : m.admin.notSet;
	}
</script>

<div class="flex h-full flex-col">
	<TopBar />
	<main class="min-h-0 flex-1 overflow-y-auto">
		<div
			class="mx-auto max-w-2xl space-y-8 px-4 pt-8 pb-[max(2rem,env(safe-area-inset-bottom))] sm:py-12"
		>
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
						<!-- Open while a key is missing, unless what it's for works without one. -->
						{@const open =
							editing === key.provider || (!key.source && !key.envSet && !key.optional)}
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
									<div
										class={cn(
											key.source || key.optional ? 'text-muted-foreground' : 'text-warning'
										)}
									>
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
											{key.source || key.envSet ? m.admin.replace : m.common.add}
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
											{#if key.source || key.envSet || key.optional}
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
										<!-- Where the chats' pictures and PDFs are kept. xAI keeps no files of nolune's,
										so any key of the account does, and Firecrawl keeps none. -->
										{#if key.source && key.provider !== 'xai' && key.provider !== 'firecrawl'}
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
					{#each data.customProviders as provider (provider.id)}
						<CustomProviderRow {provider} presets={data.presets} result={form} />
					{/each}
				</ul>
				<AddCustomProvider result={form} />
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
											{#snippet command()}<code>nolune config set claude-path</code>{/snippet}
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
										{#snippet setup()}<code>nolune claude-plan setup</code>{/snippet}
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
									{#snippet setup()}<code>nolune claude-plan setup</code>{/snippet}
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
								<MessageCircleIcon class="size-4" />
							</span>
							<div class="min-w-0 flex-1">
								<div class="font-medium">{m.admin.chatgptPlan}</div>
								<div class="text-muted-foreground">{m.admin.chatgptPlanAbout}</div>
								{#if chatgptSignedIn}
									<div class="break-words text-muted-foreground">{sentence(chatgptSignedIn)}</div>
								{:else if chatgpt.status?.problem}
									<div class="text-warning">{firstSentence(chatgpt.status.problem)}</div>
								{/if}
							</div>
							<div class="flex flex-wrap gap-1 max-sm:basis-full max-sm:pl-9">
								{#if chatgpt.pending}
									<form method="POST" action="?/chatgptCancel" use:enhance>
										<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground">
											{m.common.cancel}
										</Button>
									</form>
								{:else}
									{#if chatgptSignedIn}
										<form
											method="POST"
											action="?/chatgptCheck"
											use:enhance={() => {
												checkingChatgpt = true;
												return async ({ update }) => {
													await update();
													checkingChatgpt = false;
												};
											}}
										>
											<Button
												type="submit"
												variant="ghost"
												size="sm"
												class="text-muted-foreground"
												disabled={checkingChatgpt}
											>
												{checkingChatgpt ? m.common.checking : m.admin.checkSignIn}
											</Button>
										</form>
									{/if}
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
												? m.admin.chatgptStarting
												: chatgptSignedIn
													? m.admin.chatgptSignInAgain
													: chatgpt.previous
														? m.admin.chatgptContinueAs(chatgpt.previous)
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
									{:else if chatgpt.previous}
										<form method="POST" action="?/chatgptSignIn" use:enhance>
											<input type="hidden" name="account" value="another" />
											<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground">
												{m.admin.chatgptAnotherAccount}
											</Button>
										</form>
									{/if}
								{/if}
							</div>
						</div>

						{#if chatgpt.pending}
							<ol class="list-inside list-decimal space-y-2 sm:pl-12" aria-live="polite">
								<li>
									<!-- eslint-disable svelte/no-navigation-without-resolve -- OpenAI's sign-in page -->
									<Rich text={m.admin.chatgptOpen}>
										{#snippet link()}<a
												href={chatgpt.pending?.url}
												target="_blank"
												rel="noreferrer"
												class="underline">{m.admin.chatgptSignInPage}</a
											>{/snippet}
									</Rich>
									<!-- eslint-enable svelte/no-navigation-without-resolve -->
								</li>
								<li>{m.admin.chatgptHere}</li>
							</ol>
							<form
								method="POST"
								action="?/chatgptFinish"
								class="space-y-2 sm:pl-12"
								use:enhance={() => {
									finishing = true;
									return async ({ update }) => {
										await update();
										finishing = false;
									};
								}}
							>
								<label for="chatgpt-address" class="block text-muted-foreground">
									{m.admin.chatgptElsewhere}
								</label>
								<div class="flex gap-2">
									<Input
										id="chatgpt-address"
										name="address"
										required
										autocomplete="off"
										spellcheck="false"
										placeholder="http://127.0.0.1:…/auth/callback?code=…"
										class="min-w-0 flex-1 font-mono placeholder:font-sans"
									/>
									<Button type="submit" disabled={finishing}>{m.admin.chatgptFinish}</Button>
								</div>
							</form>
							{#if chatgptError}
								<p class="text-destructive sm:pl-12" role="alert">{chatgptError}</p>
							{/if}
						{:else if chatgptError}
							<p class="text-destructive sm:pl-12" role="alert">{chatgptError}</p>
						{:else if chatgptResult?.planMessage}
							<p class="text-muted-foreground sm:pl-12" role="status">
								{chatgptResult.planMessage}
							</p>
						{:else if chatgptSignedIn}
							<p class="text-muted-foreground sm:pl-12">
								<!-- eslint-disable svelte/no-navigation-without-resolve -- ChatGPT's settings -->
								<Rich text={m.admin.chatgptUsing}>
									{#snippet link()}<a
											href={chatgpt.usageUrl}
											target="_blank"
											rel="noreferrer"
											class="underline">{m.admin.chatgptManageUsage}</a
										>{/snippet}
								</Rich>
								<!-- eslint-enable svelte/no-navigation-without-resolve -->
							</p>
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
						<li class="border-b px-4 py-3 text-sm last:border-b-0">
							<div class="flex items-center gap-3">
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
											preset.shownProvider,
											preset.shownModel,
											formatTokens(preset.contextWindow),
											preset.override != null
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
								{#if editingPreset !== preset.id}
									<Button
										variant="ghost"
										size="sm"
										title={m.admin.editPreset(preset.name)}
										aria-label={m.admin.editPreset(preset.name)}
										class="text-muted-foreground max-sm:px-2"
										onclick={() => (editingPreset = preset.id)}
									>
										<PencilIcon />
										<span class="max-sm:sr-only">{m.common.edit}</span>
									</Button>
								{/if}
								<form method="POST" action="?/remove" use:enhance>
									<input type="hidden" name="id" value={preset.id} />
									<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground"
										>{m.common.remove}</Button
									>
								</form>
							</div>
							{#if editingPreset === preset.id}
								<PresetForm
									providers={data.providers}
									keys={data.keys}
									customProviders={data.customProviders}
									claudeInstalled={data.claude.installed}
									chatgptSignedIn={!!chatgptSignedIn}
									{preset}
									problem={form?.editId === preset.id ? form.editError : null}
									class="pt-4 sm:pl-12"
									onclose={() => (editingPreset = null)}
								/>
							{/if}
						</li>
					{:else}
						<li class="px-4 py-3 text-sm text-muted-foreground">{m.admin.noPresets}</li>
					{/each}
				</ul>

				<AddModelForm
					providers={data.providers}
					keys={data.keys}
					customProviders={data.customProviders}
					claudeInstalled={data.claude.installed}
					chatgptSignedIn={!!chatgptSignedIn}
					problem={form?.addError}
					startOpen={data.presets.length === 0}
				/>
			</section>

			<MemorySearch
				setting={data.embeddings}
				defaults={data.embeddingDefaults}
				keys={data.keys}
				customProviders={data.customProviders}
				result={form}
			/>

			<CommandSafety
				setting={data.commands}
				presets={data.presets.map(({ id, name }) => ({ id, name }))}
				result={form}
			/>

			<IdleCompaction setting={data.idleCompaction} result={form} />
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
