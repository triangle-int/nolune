<script lang="ts">
	import { enhance } from '$app/forms';
	import BoxIcon from '@lucide/svelte/icons/box';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import TerminalIcon from '@lucide/svelte/icons/terminal';
	import StarIcon from '@lucide/svelte/icons/star';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import Rich from '$lib/components/Rich.svelte';
	import TopBar from '$lib/components/TopBar.svelte';
	import CopyButton from '$lib/components/chat/CopyButton.svelte';
	import { formatTokens } from '$lib/format';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	let { data, form } = $props();
	const { m } = getI18n();
	let adding = $state(false);
	/** Whose model the new preset runs. */
	let provider = $state('anthropic');
	const EXAMPLE_MODELS: Record<string, string> = {
		anthropic: 'claude-opus-5-5',
		openai: 'gpt-6-astra',
		'claude-plan': 'claude-opus-5-5'
	};
	/** Whether the Claude Code sign-in is being checked. */
	let checkingPlan = $state(false);

	type KeyStatus = (typeof data.keys)[number];
	/** The key being pasted, the one being checked, and the one about to be removed. */
	let editing = $state<string | null>(null);
	let checking = $state<string | null>(null);
	let removing = $state<KeyStatus | null>(null);

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

			<section class="space-y-3" aria-labelledby="plan-heading">
				<div class="space-y-1">
					<h2 id="plan-heading" class="text-lg font-medium">{m.admin.plan}</h2>
					<p class="text-muted-foreground">
						{m.admin.planHint}
					</p>
				</div>
				<div class="space-y-3 rounded-2xl border px-4 py-3 text-sm">
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
							<div class="font-medium">Claude Code</div>
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
					{#if form?.planError}
						<p class="text-destructive sm:pl-12" role="alert">{form.planError}</p>
					{:else if form?.planMessage}
						<p class="text-muted-foreground sm:pl-12" role="status">{form.planMessage}</p>
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
				</div>
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
						<h2 class="font-medium">{m.admin.addPreset}</h2>
						<ToggleGroup.Root
							type="single"
							variant="outline"
							size="sm"
							value={provider}
							onValueChange={(value) => value && (provider = value)}
							aria-label={m.admin.provider}
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
						placeholder={m.admin.modelIdExample(EXAMPLE_MODELS[provider] ?? '')}
						aria-label={m.admin.modelId}
						class="h-10 rounded-full px-4"
					/>
					<div class="flex flex-col gap-3 sm:flex-row">
						<Input
							name="name"
							placeholder={m.admin.nameDefault}
							aria-label={m.admin.name}
							class="h-10 flex-1 rounded-full px-4"
						/>
						<Input
							name="contextWindow"
							type="number"
							min="1"
							placeholder={provider === 'openai'
								? m.admin.contextKnown
								: provider === 'claude-plan'
									? m.admin.contextNotReported
									: m.admin.contextOptional}
							aria-label={m.admin.contextWindow}
							class="h-10 rounded-full px-4 sm:w-60"
						/>
					</div>
					<Button type="submit" disabled={adding} class="h-10 px-5">
						{adding
							? provider === 'claude-plan'
								? m.admin.checkingClaude
								: m.admin.checkingModel
							: m.common.add}
					</Button>
				</form>
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
