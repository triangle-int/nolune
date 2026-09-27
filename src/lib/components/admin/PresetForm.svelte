<script lang="ts">
	import { enhance } from '$app/forms';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import type { ModelChoice } from '@btw/core';
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import { Input } from '$lib/components/ui/input';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import ModelPicker from './ModelPicker.svelte';
	import { formatTokens, parseTokens } from '$lib/format';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	interface Props {
		providers: { id: string; label: string }[];
		/** Whether each API key provider has a key, and its last four characters. */
		keys: { provider: string; source: string | null; hint: string | null }[];
		/** The family's servers, for Custom OpenAI and Custom Anthropic. */
		servers: { name: string; url: string }[];
		claudeInstalled: boolean;
		codexInstalled: boolean;
		/** The preset being changed, with its own context window if it has one; a new one without. */
		preset?: { id: string; name: string; provider: string; model: string; override: number | null };
		/** Why the last save failed. */
		problem?: string | null;
		class?: string;
		/** After Cancel, and after a save. */
		onclose: () => void;
	}

	let {
		providers,
		keys,
		servers,
		claudeInstalled,
		codexInstalled,
		preset,
		problem,
		class: className,
		onclose
	}: Props = $props();

	const { m: messages } = getI18n();
	const t = messages.admin.addModel;
	const uid = $props.id();

	/** Offered as chips; any other size is typed under Custom. */
	const CONTEXT_WINDOWS = [128_000, 200_000, 1_000_000];
	/** What the form starts with: the preset's, or nothing yet. */
	const start = untrack(() => preset);
	/** Custom OpenAI and Custom Anthropic run a server's models, kept as `<server>/<model>`. */
	const isCustom = (provider: string) => provider.startsWith('custom-');
	const serverOf = (model: string) => model.slice(0, Math.max(0, model.indexOf('/')));
	/** As presets.ts names it: a server's model by its server's name. */
	const defaultName = (model: string, provider: string) =>
		isCustom(provider) && serverOf(model)
			? `${model.slice(serverOf(model).length + 1)} (${serverOf(model)})`
			: `${model} (${provider})`;

	let saving = $state(false);
	/** The last save's problem is this form's only once it has been sent. */
	let sent = $state(false);

	let provider = $state(start?.provider ?? 'anthropic');
	let model = $state(start?.model ?? '');
	let server = $state(
		(start && isCustom(start.provider) && serverOf(start.model)) ||
			untrack(() => servers[0]?.name) ||
			''
	);
	const serverUrl = $derived(servers.find((s) => s.name === server)?.url);
	/** The model as the preset keeps it: a server's with its server's name before it. */
	const fullModel = $derived(
		isCustom(provider) && model && !model.startsWith(`${server}/`) ? `${server}/${model}` : model
	);
	/** Empty while it's the default, so the default goes on following the model. */
	let name = $state(
		start && start.name !== defaultName(start.model, start.provider) ? start.name : ''
	);
	const label = $derived(providers.find((p) => p.id === provider)?.label ?? provider);
	const key = $derived(keys.find((k) => k.provider === provider));
	/** Whether the provider can be used: a key, the plan's agent (Claude Code, Codex), or a server. */
	const ready = $derived(
		provider === 'claude-plan'
			? claudeInstalled
			: provider === 'chatgpt-plan'
				? codexInstalled
				: isCustom(provider)
					? !!serverUrl
					: !!key?.source
	);
	/** A new provider or model is checked with the provider when it's saved. */
	const checks = $derived(!start || provider !== start.provider || fullModel !== start.model);

	type ModelList = { models: ModelChoice[]; problem: string | null };
	/** Each provider's models (null while they're asked for), asked for again when its key changes. */
	let lists = $state<Record<string, ModelList | null>>({});
	const listKey = $derived(
		`${provider}:${isCustom(provider) ? `${server} ${serverUrl}` : (key?.hint ?? '')}:${ready}`
	);
	const list = $derived<ModelList | null>(
		ready ? (lists[listKey] ?? null) : { models: [], problem: null }
	);
	const picked = $derived(list?.models.find((m) => m.id === fullModel));

	$effect(() => {
		if (!ready || listKey in lists) return;
		const at = listKey;
		lists[at] = null;
		const on = isCustom(provider) ? `&server=${encodeURIComponent(server)}` : '';
		fetch(`/api/models?provider=${encodeURIComponent(provider)}${on}`)
			.then(async (res): Promise<ModelList> =>
				res.ok ? await res.json() : { models: [], problem: t.couldNotList(res.status) }
			)
			.catch((): ModelList => ({ models: [], problem: t.unreachable }))
			.then((result) => (lists[at] = result));
	});

	const providerNote = $derived.by(() => {
		if (provider === 'claude-plan') {
			return claudeInstalled
				? { text: t.onPlan, warn: false }
				: { text: t.noClaudeCode, warn: true };
		}
		if (provider === 'chatgpt-plan') {
			return codexInstalled
				? { text: t.onChatGptPlan, warn: false }
				: { text: t.noCodex, warn: true };
		}
		if (isCustom(provider)) {
			const api = provider === 'custom-anthropic' ? 'Anthropic' : 'OpenAI';
			return serverUrl
				? { text: t.onServer(api, server, serverUrl), warn: false }
				: { text: t.noServers, warn: true };
		}
		return ready ? { text: t.onKey(label), warn: false } : { text: t.noKey(label), warn: true };
	});

	const modelNote = $derived.by(() => {
		if (!ready) return null;
		if (!list) return { text: t.asking(sourceName()), warn: false };
		if (list.problem) return { text: t.listProblem(list.problem), warn: true };
		if (picked?.description) return { text: picked.description, warn: false };
		return null;
	});

	function sourceName(): string {
		return provider === 'claude-plan'
			? 'Claude Code'
			: provider === 'chatgpt-plan'
				? 'Codex'
				: isCustom(provider)
					? server
					: label;
	}

	/** What Auto (no override) gets with each provider. */
	const AUTO_CONTEXT: Record<string, string | undefined> = t.autoContext;
	let contextOpen = $state(false);
	/** "auto", a chip's token count, or "custom". */
	let contextChoice = $state(
		start?.override == null
			? 'auto'
			: CONTEXT_WINDOWS.includes(start.override)
				? String(start.override)
				: 'custom'
	);
	let customContext = $state(
		start?.override != null && !CONTEXT_WINDOWS.includes(start.override)
			? String(start.override)
			: ''
	);
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
		if (Number.isNaN(tokens)) return t.typeTokens;
		return t.tokens(tokens);
	});
	/** Next to "Context window" while it's folded away. */
	const contextSummary = $derived.by(() => {
		if (contextChoice === 'auto') {
			if (!picked) return t.auto;
			return t.autoWith(picked.contextWindow ? formatTokens(picked.contextWindow) : t.unknown);
		}
		const tokens = contextChoice === 'custom' ? customTokens : Number(contextChoice);
		return Number.isNaN(tokens) ? t.custom : formatTokens(tokens);
	});
</script>

<form
	method="POST"
	action={start ? '?/edit' : '?/add'}
	class={cn('space-y-5 text-sm', className)}
	aria-labelledby={start ? undefined : `${uid}-heading`}
	aria-label={start ? messages.admin.editPreset(start.name) : undefined}
	use:enhance={({ cancel }) => {
		if (contextChoice === 'custom' && Number.isNaN(customTokens)) {
			cancel();
			contextOpen = true;
			contextTried = true;
			return;
		}
		saving = true;
		sent = true;
		return async ({ result, update }) => {
			await update();
			saving = false;
			if (result.type === 'success') onclose();
		};
	}}
>
	{#if start}
		<input type="hidden" name="id" value={start.id} />
	{:else}
		<h3 id="{uid}-heading" class="text-base font-medium">{t.title}</h3>
	{/if}

	<div class="space-y-2">
		<div id="{uid}-provider" class="font-medium">{t.provider}</div>
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
			aria-labelledby="{uid}-provider"
			class="flex-wrap"
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

	{#if isCustom(provider) && servers.length > 1}
		<div class="space-y-2">
			<div id="{uid}-server" class="font-medium">{t.server}</div>
			<ToggleGroup.Root
				type="single"
				variant="outline"
				size="sm"
				value={server}
				onValueChange={(value) => {
					if (!value || value === server) return;
					server = value;
					// Another server's models may not be there.
					model = '';
				}}
				aria-labelledby="{uid}-server"
				class="flex-wrap"
			>
				{#each servers as s (s.name)}
					<ToggleGroup.Item value={s.name}>{s.name}</ToggleGroup.Item>
				{/each}
			</ToggleGroup.Root>
		</div>
	{/if}

	<div class="space-y-2">
		<label for="{uid}-model" class="block font-medium">{t.model}</label>
		<ModelPicker
			id="{uid}-model"
			bind:value={model}
			models={list?.models ?? null}
			source={sourceName()}
			describedBy={modelNote ? `${uid}-model-note` : undefined}
		/>
		{#if modelNote}
			<p id="{uid}-model-note" class={modelNote.warn ? 'text-warning' : 'text-muted-foreground'}>
				{modelNote.text}
			</p>
		{/if}
		<input type="hidden" name="model" value={fullModel} />
	</div>

	<div class="space-y-2">
		<label for="{uid}-name" class="block font-medium">
			{t.name} <span class="font-normal text-muted-foreground">{t.optional}</span>
		</label>
		<Input
			id="{uid}-name"
			name="name"
			bind:value={name}
			placeholder={model ? defaultName(fullModel, provider) : t.namePlaceholder}
			class="h-10 rounded-full px-4"
		/>
	</div>

	<Collapsible.Root bind:open={contextOpen} class="space-y-2">
		<Collapsible.Trigger
			class="-mx-2 flex w-[calc(100%+1rem)] items-center justify-between gap-3 rounded-xl px-2 py-1.5 hover:bg-muted"
		>
			<span class="font-medium">{t.contextWindow}</span>
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
				aria-label={t.contextWindow}
			>
				<ToggleGroup.Item value="auto">{t.auto}</ToggleGroup.Item>
				{#each CONTEXT_WINDOWS as tokens (tokens)}
					<ToggleGroup.Item value={String(tokens)}>{formatTokens(tokens)}</ToggleGroup.Item>
				{/each}
				<ToggleGroup.Item value="custom">{t.custom}</ToggleGroup.Item>
			</ToggleGroup.Root>
			{#if contextChoice === 'custom'}
				<Input
					bind:value={customContext}
					autocomplete="off"
					spellcheck="false"
					placeholder={t.tokensPlaceholder}
					aria-label={t.contextTokens}
					aria-describedby="{uid}-context-hint"
					aria-invalid={contextInvalid}
					class="h-10 rounded-full px-4 sm:w-60"
					{@attach (input) => input.focus()}
				/>
			{/if}
			<p
				id="{uid}-context-hint"
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

	{#if problem && sent}
		<p class="text-destructive" role="alert">{problem}</p>
	{/if}

	<div class="flex gap-2">
		<Button type="submit" disabled={saving || !model} class="h-10 px-5">
			{#if !saving}
				{start ? messages.common.save : messages.common.add}
			{:else if !checks}
				{t.saving}
			{:else}
				{provider === 'claude-plan'
					? t.checkingClaude
					: provider === 'chatgpt-plan'
						? t.checkingCodex
						: t.checkingModel}
			{/if}
		</Button>
		<Button type="button" variant="ghost" class="h-10" onclick={onclose}>
			{messages.common.cancel}
		</Button>
	</div>
</form>
