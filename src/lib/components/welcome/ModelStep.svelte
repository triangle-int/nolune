<script lang="ts">
	import { untrack } from 'svelte';
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import CheckIcon from '@lucide/svelte/icons/check';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import MessageCircleIcon from '@lucide/svelte/icons/message-circle';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import type { ApiKeyProvider, ModelChoice } from '@btw/core';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import CopyButton from '$lib/components/chat/CopyButton.svelte';
	import Rich from '$lib/components/Rich.svelte';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';
	import { play } from '$lib/welcome/sounds';

	interface Props {
		isAdmin: boolean;
		keys: { provider: ApiKeyProvider; label: string; consoleUrl: string; set: boolean }[];
		/** The model is saved, or someone who can't add one carries on without. */
		ondone: () => void;
	}

	let { isAdmin, keys, ondone }: Props = $props();
	const { m } = getI18n();

	type Choice = 'claude-plan' | 'chatgpt-plan' | ApiKeyProvider;
	const CHOICES: { id: Choice; icon: typeof KeyRoundIcon }[] = [
		{ id: 'claude-plan', icon: SparklesIcon },
		{ id: 'chatgpt-plan', icon: MessageCircleIcon },
		{ id: 'anthropic', icon: KeyRoundIcon },
		{ id: 'openai', icon: KeyRoundIcon },
		{ id: 'openrouter', icon: KeyRoundIcon }
	];
	/** How many of the provider's models are offered as chips; the rest can be typed. */
	const SHOWN = 6;

	/** A key that's already set (in the environment, say) is the likely pick. */
	let choice = $state<Choice>(untrack(() => keys.find((k) => k.set)?.provider ?? 'claude-plan'));
	let view = $state<'choose' | 'key' | 'plan' | 'models'>('choose');
	let busy = $state(false);
	let problem = $state<string | null>(null);
	let installCommand = $state<string | null>(null);
	let note = $state<string | null>(null);

	let models = $state<ModelChoice[] | null>(null);
	let listProblem = $state<string | null>(null);
	let model = $state('');

	const key = $derived(keys.find((k) => k.provider === choice));
	const isPlan = $derived(choice === 'claude-plan' || choice === 'chatgpt-plan');

	function pick(id: Choice) {
		if (choice !== id) play('click');
		choice = id;
		problem = null;
	}

	async function listModels() {
		view = 'models';
		models = null;
		listProblem = null;
		try {
			const res = await fetch(`/api/models?provider=${encodeURIComponent(choice)}`);
			const body = res.ok
				? ((await res.json()) as { models: ModelChoice[]; problem: string | null })
				: { models: [], problem: m.admin.addModel.couldNotList(res.status) };
			models = body.models.slice(0, SHOWN);
			listProblem = body.problem;
			model = models[0]?.id ?? '';
		} catch {
			models = [];
			listProblem = m.admin.addModel.unreachable;
		}
	}

	/** API keys already set go straight to the models; plans are checked first. */
	function next() {
		problem = null;
		note = null;
		if (key?.set) listModels();
		else if (key) view = 'key';
	}
</script>

{#if !isAdmin}
	<div class="space-y-6 text-center">
		<h2 class="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
			{m.welcome.model.title}
		</h2>
		<p class="mx-auto max-w-md text-muted-foreground">{m.welcome.model.askAdmin}</p>
		<Button size="lg" class="h-11 px-8" onclick={ondone}>{m.common.continue}</Button>
	</div>
{:else if view === 'choose'}
	<form
		method="POST"
		action="?/plan"
		class="space-y-8"
		use:enhance={({ cancel }) => {
			if (!isPlan) {
				cancel();
				next();
				return;
			}
			busy = true;
			return async ({ result }) => {
				busy = false;
				if (result.type === 'success') {
					play('confirm');
					note = String(result.data?.signedIn ?? '');
					listModels();
				} else if (result.type === 'failure') {
					problem = String(result.data?.planError ?? '');
					installCommand = (result.data?.installCommand as string | null) ?? null;
					view = 'plan';
				}
			};
		}}
	>
		<div class="space-y-3 text-center">
			<h2 class="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
				{m.welcome.model.title}
			</h2>
			<p class="text-muted-foreground">{m.welcome.model.subtitle}</p>
		</div>
		<input type="hidden" name="plan" value={choice} />
		<!-- The two plans side by side, the API keys in a row under them. -->
		<div class="grid gap-3 sm:grid-cols-6" role="radiogroup" aria-label={m.welcome.model.title}>
			{#each CHOICES as { id, icon: Icon } (id)}
				{@const picked = choice === id}
				<button
					type="button"
					role="radio"
					aria-checked={picked}
					onclick={() => pick(id)}
					class={cn(
						'relative flex flex-col gap-3 rounded-3xl border bg-card p-5 text-left transition duration-200 outline-none hover:border-foreground/25 focus-visible:ring-3 focus-visible:ring-ring/30 motion-safe:hover:-translate-y-0.5',
						id === 'claude-plan' || id === 'chatgpt-plan' ? 'sm:col-span-3' : 'sm:col-span-2',
						picked && 'border-foreground/60 shadow-lg ring-3 ring-foreground/10'
					)}
				>
					<Icon class="size-6" />
					<span class="space-y-0.5">
						<span class="block font-medium">{m.welcome.model.choices[id].title}</span>
						<span class="block text-sm text-muted-foreground">
							{m.welcome.model.choices[id].about}
						</span>
					</span>
					{#if picked}
						<span
							class="absolute top-4 right-4 flex size-6 items-center justify-center rounded-full bg-foreground text-background"
						>
							<CheckIcon class="size-3.5" />
						</span>
					{/if}
				</button>
			{/each}
		</div>
		<div class="flex justify-center">
			<Button type="submit" size="lg" class="h-11 min-w-44 px-8" disabled={busy}>
				{busy ? m.welcome.model.checkingPlan : m.common.continue}
			</Button>
		</div>
	</form>
{:else if view === 'key' && key}
	<form
		method="POST"
		action="?/key"
		class="space-y-6"
		use:enhance={() => {
			busy = true;
			problem = null;
			return async ({ result }) => {
				busy = false;
				if (result.type === 'success') {
					play('confirm');
					const warning = result.data?.keyWarning;
					note = warning ? String(warning) : m.welcome.model.keyWorks;
					listModels();
				} else if (result.type === 'failure') {
					problem = String(result.data?.keyError ?? '');
				}
			};
		}}
	>
		<h2 class="text-center text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
			{m.welcome.model.pasteKey(key.label)}
		</h2>
		<input type="hidden" name="provider" value={key.provider} />
		<div class="space-y-2">
			<Input
				name="key"
				type="password"
				required
				autocomplete="off"
				spellcheck="false"
				aria-label={m.admin.keyLabel(key.label)}
				placeholder={m.admin.pasteKey(key.label)}
				class={cn(
					'h-12 rounded-full px-5 font-mono placeholder:font-sans',
					problem && 'border-destructive'
				)}
			/>
			{#if problem}
				<p class="px-5 text-sm text-destructive" role="alert">{problem}</p>
			{:else}
				<p class="px-5 text-sm text-muted-foreground">
					<!-- eslint-disable svelte/no-navigation-without-resolve -- the provider's console, another site -->
					<Rich text={m.admin.makeOneAt}>
						{#snippet link()}<a
								href={key.consoleUrl}
								target="_blank"
								rel="noreferrer"
								class="underline">{new URL(key.consoleUrl).host}</a
							>{/snippet}
					</Rich>
					<!-- eslint-enable svelte/no-navigation-without-resolve -->
				</p>
			{/if}
		</div>
		<div class="flex justify-center gap-2">
			<Button
				type="button"
				variant="ghost"
				size="lg"
				class="h-11"
				onclick={() => (view = 'choose')}
			>
				{m.welcome.back}
			</Button>
			<Button type="submit" size="lg" class="h-11 min-w-44 px-8" disabled={busy}>
				{busy ? m.admin.checkingKey : m.welcome.model.checkKey}
			</Button>
		</div>
	</form>
{:else if view === 'plan'}
	<form
		method="POST"
		action="?/plan"
		class="space-y-6 text-center"
		use:enhance={() => {
			busy = true;
			return async ({ result }) => {
				busy = false;
				if (result.type === 'success') {
					play('confirm');
					note = String(result.data?.signedIn ?? '');
					listModels();
				} else if (result.type === 'failure') {
					problem = String(result.data?.planError ?? '');
				}
			};
		}}
	>
		<h2 class="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
			{m.welcome.model.choices[choice].title}
		</h2>
		<input type="hidden" name="plan" value={choice} />
		<p class="mx-auto max-w-md text-muted-foreground" role="alert">{problem}</p>
		{#if installCommand}
			<div
				class="mx-auto flex max-w-md items-center gap-1 rounded-xl bg-muted py-1 pr-1 pl-3 text-sm"
			>
				<code class="min-w-0 flex-1 truncate text-left font-mono">{installCommand}</code>
				<CopyButton text={installCommand} label={m.admin.copyCommand} />
			</div>
		{/if}
		<p class="text-sm text-muted-foreground">
			<Rich text={m.welcome.model.finishOnAdmin}>
				{#snippet link()}<a href={resolve('/admin')} target="_blank" class="underline"
						>{m.userMenu.modelsAndKeys}</a
					>{/snippet}
			</Rich>
		</p>
		<div class="flex justify-center gap-2">
			<Button
				type="button"
				variant="ghost"
				size="lg"
				class="h-11"
				onclick={() => (view = 'choose')}
			>
				{m.welcome.back}
			</Button>
			<Button type="submit" size="lg" class="h-11 min-w-44 px-8" disabled={busy}>
				{busy ? m.common.checking : m.welcome.model.checkAgain}
			</Button>
		</div>
	</form>
{:else if view === 'models'}
	<form
		method="POST"
		action="?/model"
		class="space-y-6"
		use:enhance={() => {
			busy = true;
			problem = null;
			return async ({ result }) => {
				busy = false;
				if (result.type === 'success') {
					play('confirm');
					ondone();
				} else if (result.type === 'failure') {
					problem = String(result.data?.modelError ?? '');
				}
			};
		}}
	>
		<div class="space-y-3 text-center">
			<h2 class="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
				{m.welcome.model.pickModel}
			</h2>
			{#if note}
				<p class="flex items-center justify-center gap-1.5 text-sm text-muted-foreground">
					<CheckIcon class="size-4 text-emerald-600 dark:text-emerald-400" />
					{note}
				</p>
			{/if}
		</div>
		<input type="hidden" name="provider" value={choice} />
		{#if models === null}
			<p class="text-center text-sm text-muted-foreground" aria-live="polite">
				{m.welcome.model.asking}
			</p>
		{:else}
			{#if models.length}
				<div class="flex flex-wrap justify-center gap-2" role="radiogroup">
					{#each models as option (option.id)}
						<button
							type="button"
							role="radio"
							aria-checked={model === option.id}
							onclick={() => {
								if (model !== option.id) play('click');
								model = option.id;
							}}
							class={cn(
								'rounded-full border px-4 py-2 text-sm transition-colors outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/30',
								model === option.id &&
									'border-foreground bg-foreground text-background hover:bg-foreground/90'
							)}
						>
							{option.name ?? option.id}
						</button>
					{/each}
				</div>
			{/if}
			{#if listProblem}
				<p class="text-center text-sm text-muted-foreground">{listProblem}</p>
			{/if}
			<Input
				name="model"
				bind:value={model}
				required
				spellcheck="false"
				aria-label={m.welcome.model.modelId}
				placeholder={m.welcome.model.modelId}
				class="mx-auto h-10 max-w-sm rounded-full px-4 text-center font-mono text-sm"
			/>
			{#if problem}
				<p class="text-center text-sm text-destructive" role="alert">{problem}</p>
			{/if}
		{/if}
		<div class="flex justify-center gap-2">
			<Button
				type="button"
				variant="ghost"
				size="lg"
				class="h-11"
				onclick={() => (view = 'choose')}
			>
				{m.welcome.back}
			</Button>
			<Button type="submit" size="lg" class="h-11 min-w-44 px-8" disabled={busy || !model}>
				{busy ? m.admin.addModel.checkingModel : m.common.continue}
			</Button>
		</div>
	</form>
{/if}
