<script lang="ts">
	import { enhance } from '$app/forms';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import type { ModelChoice } from '@btw/core';
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import { Input } from '$lib/components/ui/input';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import ModelPicker from './ModelPicker.svelte';
	import { formatTokens, parseTokens } from '$lib/format';
	import { cn } from '$lib/utils';

	interface Props {
		providers: { id: string; label: string }[];
		/** Whether each API key provider has a key, and its last four characters. */
		keys: { provider: string; source: string | null; hint: string | null }[];
		claudeInstalled: boolean;
		/** Why the last add failed. */
		problem?: string | null;
		/** Open from the start: when there are no models yet. */
		startOpen: boolean;
	}

	let { providers, keys, claudeInstalled, problem, startOpen }: Props = $props();

	let open = $state(untrack(() => startOpen));
	let adding = $state(false);
	/** Hides the last add's problem after Cancel, until the next try. */
	let problemSeen = $state(false);

	let provider = $state('anthropic');
	let model = $state('');
	const label = $derived(providers.find((p) => p.id === provider)?.label ?? provider);
	const key = $derived(keys.find((k) => k.provider === provider));
	/** Whether the provider can be used: a key, or Claude Code for the plan. */
	const ready = $derived(provider === 'claude-plan' ? claudeInstalled : !!key?.source);

	type ModelList = { models: ModelChoice[]; problem: string | null };
	/** Each provider's models (null while they're asked for), asked for again when its key changes. */
	let lists = $state<Record<string, ModelList | null>>({});
	const listKey = $derived(`${provider}:${key?.hint ?? ''}:${ready}`);
	const list = $derived<ModelList | null>(
		ready ? (lists[listKey] ?? null) : { models: [], problem: null }
	);
	const picked = $derived(list?.models.find((m) => m.id === model));

	$effect(() => {
		if (!open || !ready || listKey in lists) return;
		const at = listKey;
		lists[at] = null;
		fetch(`/api/models?provider=${encodeURIComponent(provider)}`)
			.then(async (res): Promise<ModelList> =>
				res.ok
					? await res.json()
					: { models: [], problem: `btw couldn't get the models (${res.status}).` }
			)
			.catch((): ModelList => ({ models: [], problem: "btw couldn't be reached." }))
			.then((result) => (lists[at] = result));
	});

	const providerNote = $derived.by(() => {
		if (provider === 'claude-plan') {
			return claudeInstalled
				? { text: 'Runs on the Pro or Max plan Claude Code is signed in to.', warn: false }
				: { text: "Claude Code isn't installed yet: see Claude plan, above.", warn: true };
		}
		return ready
			? { text: `Runs on the ${label} API key.`, warn: false }
			: { text: `No ${label} API key yet: add one under API keys, above.`, warn: true };
	});

	const modelNote = $derived.by(() => {
		if (!ready) return null;
		if (!list) return { text: `Asking ${sourceName()} for its models…`, warn: false };
		if (list.problem) return { text: `${list.problem} You can still type an id.`, warn: true };
		if (picked?.description) return { text: picked.description, warn: false };
		return null;
	});

	function sourceName(): string {
		return provider === 'claude-plan' ? 'Claude Code' : label;
	}

	/** Offered as chips; any other size is typed under Custom. */
	const CONTEXT_WINDOWS = [128_000, 200_000, 1_000_000];
	/** What Auto (no override) gets with each provider. */
	const AUTO_CONTEXT: Record<string, string> = {
		anthropic: 'Auto uses the window Anthropic reports for the model.',
		openai: "OpenAI doesn't report it: Auto knows only its flagships' (1.05M since GPT-5.4).",
		openrouter: 'Auto uses the window OpenRouter lists for the model and its main provider.',
		'claude-plan': "Claude Code doesn't report it: Auto knows only its 1M-context models'."
	};
	let contextOpen = $state(false);
	/** "auto", a chip's token count, or "custom". */
	let contextChoice = $state('auto');
	let customContext = $state('');
	/** Set when Add was pressed with Custom and no count, so the hint turns red. */
	let contextTried = $state(false);
	const customTokens = $derived(parseTokens(customContext));
	const contextInvalid = $derived(
		contextChoice === 'custom' &&
			Number.isNaN(customTokens) &&
			(contextTried || customContext.trim() !== '')
	);
	const contextHint = $derived.by(() => {
		if (contextChoice === 'auto') return AUTO_CONTEXT[provider] ?? '';
		const tokens = contextChoice === 'custom' ? customTokens : Number(contextChoice);
		if (Number.isNaN(tokens)) return 'Type a token count, like 272k or 272000.';
		return `${tokens.toLocaleString('en')} tokens.`;
	});
	/** Next to "Context window" while it's folded away. */
	const contextSummary = $derived.by(() => {
		if (contextChoice === 'auto') {
			if (!picked) return 'Auto';
			return `Auto · ${picked.contextWindow ? formatTokens(picked.contextWindow) : 'unknown'}`;
		}
		const tokens = contextChoice === 'custom' ? customTokens : Number(contextChoice);
		return Number.isNaN(tokens) ? 'Custom' : formatTokens(tokens);
	});

	function close() {
		open = false;
		problemSeen = true;
		model = '';
		contextOpen = false;
		contextChoice = 'auto';
		customContext = '';
		contextTried = false;
	}
</script>

{#if !open}
	<Button variant="outline" class="h-10 rounded-full px-4" onclick={() => (open = true)}>
		<PlusIcon />
		Add a model
	</Button>
{:else}
	<form
		method="POST"
		action="?/add"
		class="space-y-5 rounded-2xl border p-4 text-sm sm:p-5"
		aria-labelledby="add-model-heading"
		use:enhance={({ cancel }) => {
			if (contextChoice === 'custom' && Number.isNaN(customTokens)) {
				cancel();
				contextOpen = true;
				contextTried = true;
				return;
			}
			adding = true;
			problemSeen = false;
			return async ({ result, update }) => {
				await update();
				adding = false;
				if (result.type === 'success') close();
			};
		}}
	>
		<h3 id="add-model-heading" class="text-base font-medium">Add a model</h3>

		<div class="space-y-2">
			<div id="provider-label" class="font-medium">Provider</div>
			<ToggleGroup.Root
				type="single"
				variant="outline"
				size="sm"
				value={provider}
				onValueChange={(value) => {
					if (!value || value === provider) return;
					provider = value;
					// Another provider's ids mean nothing here.
					model = '';
				}}
				aria-labelledby="provider-label"
			>
				{#each providers as p (p.id)}
					<ToggleGroup.Item value={p.id}>{p.label}</ToggleGroup.Item>
				{/each}
			</ToggleGroup.Root>
			<p class={providerNote.warn ? 'text-warning' : 'text-muted-foreground'}>
				{providerNote.text}
			</p>
			<input type="hidden" name="provider" value={provider} />
		</div>

		<div class="space-y-2">
			<label for="add-model-picker" class="block font-medium">Model</label>
			<ModelPicker
				id="add-model-picker"
				bind:value={model}
				models={list?.models ?? null}
				source={sourceName()}
				describedBy={modelNote ? 'add-model-note' : undefined}
			/>
			{#if modelNote}
				<p id="add-model-note" class={modelNote.warn ? 'text-warning' : 'text-muted-foreground'}>
					{modelNote.text}
				</p>
			{/if}
			<input type="hidden" name="model" value={model} />
		</div>

		<div class="space-y-2">
			<label for="add-model-name" class="block font-medium">
				Name <span class="font-normal text-muted-foreground">(optional)</span>
			</label>
			<Input
				id="add-model-name"
				name="name"
				placeholder={model ? `${model} (${provider})` : 'The model id and provider'}
				class="h-10 rounded-full px-4"
			/>
		</div>

		<Collapsible.Root bind:open={contextOpen} class="space-y-2">
			<Collapsible.Trigger
				class="-mx-2 flex w-[calc(100%+1rem)] items-center justify-between gap-3 rounded-xl px-2 py-1.5 hover:bg-muted"
			>
				<span class="font-medium">Context window</span>
				<span class="flex items-center gap-1 text-muted-foreground">
					{contextSummary}
					<ChevronDownIcon class={cn('size-4 transition-transform', contextOpen && 'rotate-180')} />
				</span>
			</Collapsible.Trigger>
			<Collapsible.Content class="space-y-2">
				<ToggleGroup.Root
					type="single"
					variant="outline"
					size="sm"
					spacing={1}
					class="flex-wrap"
					value={contextChoice}
					onValueChange={(value) => value && (contextChoice = value)}
					aria-label="Context window"
				>
					<ToggleGroup.Item value="auto">Auto</ToggleGroup.Item>
					{#each CONTEXT_WINDOWS as tokens (tokens)}
						<ToggleGroup.Item value={String(tokens)}>{formatTokens(tokens)}</ToggleGroup.Item>
					{/each}
					<ToggleGroup.Item value="custom">Custom</ToggleGroup.Item>
				</ToggleGroup.Root>
				{#if contextChoice === 'custom'}
					<Input
						bind:value={customContext}
						autocomplete="off"
						spellcheck="false"
						placeholder="Tokens, e.g. 272k"
						aria-label="Context window in tokens"
						aria-describedby="add-model-context-hint"
						aria-invalid={contextInvalid}
						class="h-10 rounded-full px-4 sm:w-60"
						{@attach (input) => input.focus()}
					/>
				{/if}
				<p
					id="add-model-context-hint"
					class={contextInvalid ? 'text-destructive' : 'text-muted-foreground'}
				>
					{contextHint}
				</p>
			</Collapsible.Content>
		</Collapsible.Root>
		{#if contextChoice !== 'auto'}
			<!-- Out here, so a folded-away choice is still sent. -->
			<input
				type="hidden"
				name="contextWindow"
				value={contextChoice === 'custom' ? customContext : contextChoice}
			/>
		{/if}

		{#if problem && !problemSeen}
			<p class="text-destructive" role="alert">{problem}</p>
		{/if}

		<div class="flex gap-2">
			<Button type="submit" disabled={adding || !model} class="h-10 px-5">
				{adding
					? provider === 'claude-plan'
						? 'Checking Claude Code…'
						: 'Checking the model…'
					: 'Add'}
			</Button>
			<Button type="button" variant="ghost" class="h-10" onclick={close}>Cancel</Button>
		</div>
	</form>
{/if}
