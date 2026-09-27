<script lang="ts">
	import { enhance } from '$app/forms';
	import SearchIcon from '@lucide/svelte/icons/search';
	import type { ApiKeyStatus, EmbeddingState } from '@btw/core';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	type Mode = EmbeddingState['mode'];

	/**
	 * Where memory search gets its embeddings, to find facts by meaning: what's in use, and a form
	 * to change it. `defaults`: each provider's model when none is typed.
	 */
	let {
		setting,
		defaults,
		keys,
		result
	}: {
		setting: EmbeddingState;
		defaults: { openai: string; openrouter: string };
		keys: Pick<ApiKeyStatus, 'provider' | 'label' | 'source'>[];
		result:
			| { embeddingsMessage?: string; embeddingsWarning?: string; embeddingsError?: string }
			| null
			| undefined;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.embeddings);
	const uid = $props.id();
	const MODES: Mode[] = ['auto', 'openai', 'openrouter', 'server', 'off'];

	let open = $state(false);
	let saving = $state(false);
	let mode = $state<Mode>('auto');
	let model = $state('');
	let url = $state('');

	const key = (provider: string) => keys.find((k) => k.provider === provider);
	const hasKey = (provider: string) => !!key(provider)?.source;
	const label = (provider: string) => key(provider)?.label ?? provider;

	/** What the row says is in use, or why search goes by words only. */
	const status = $derived.by(() => {
		if (setting.using) {
			return {
				warn: false,
				text:
					setting.mode === 'server' && setting.model && setting.url
						? t.usingServer(setting.model, setting.url)
						: t.using(setting.using)
			};
		}
		if (setting.mode === 'off') return { warn: false, text: t.off };
		if (setting.mode === 'openai' || setting.mode === 'openrouter') {
			return { warn: true, text: t.noKey(label(setting.mode)) };
		}
		return { warn: true, text: t.noKeys };
	});

	/** Under the choice: what it does, or what it still needs. */
	const note = $derived.by(() => {
		if (mode === 'auto') {
			return hasKey('openai') || hasKey('openrouter')
				? { warn: false, text: t.autoNote }
				: { warn: true, text: t.noKeys };
		}
		if (mode === 'openai' || mode === 'openrouter') {
			return hasKey(mode)
				? { warn: false, text: t.withKey(label(mode)) }
				: { warn: true, text: t.noKey(label(mode)) };
		}
		return { warn: false, text: mode === 'server' ? t.serverNote : t.offNote };
	});

	/** The form starts from what's saved. */
	function start() {
		mode = setting.mode;
		model = setting.model ?? '';
		url = setting.url ?? '';
		open = true;
	}
</script>

<section class="space-y-3" aria-labelledby="{uid}-heading">
	<div class="space-y-1">
		<h2 id="{uid}-heading" class="text-lg font-medium">{t.title}</h2>
		<p class="text-muted-foreground">{t.hint}</p>
	</div>
	<div class="space-y-3 rounded-2xl border px-4 py-3 text-sm">
		<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
			<span
				class={cn(
					'flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted max-sm:self-start',
					setting.using ? 'text-foreground' : 'text-muted-foreground'
				)}
			>
				<SearchIcon class="size-4" />
			</span>
			<div class="min-w-0 flex-1">
				<div class="font-medium">{t.name}</div>
				<div class={cn('break-words', status.warn ? 'text-warning' : 'text-muted-foreground')}>
					{status.text}
				</div>
			</div>
			{#if !open}
				<div class="flex gap-1 max-sm:basis-full max-sm:pl-9">
					<Button variant="ghost" size="sm" class="text-muted-foreground" onclick={start}>
						{t.change}
					</Button>
				</div>
			{/if}
		</div>

		{#if open}
			<form
				method="POST"
				action="?/embeddings"
				class="space-y-4 sm:pl-12"
				use:enhance={() => {
					saving = true;
					return async ({ result: sent, update }) => {
						await update({ reset: false });
						saving = false;
						if (sent.type === 'success') open = false;
					};
				}}
			>
				<div class="space-y-2">
					<span id="{uid}-source" class="block font-medium">{t.source}</span>
					<ToggleGroup.Root
						type="single"
						variant="outline"
						size="sm"
						value={mode}
						onValueChange={(value) => {
							if (value) mode = value as Mode;
						}}
						aria-labelledby="{uid}-source"
						class="flex-wrap"
					>
						{#each MODES as id (id)}
							<ToggleGroup.Item value={id}>{t.modes[id]}</ToggleGroup.Item>
						{/each}
					</ToggleGroup.Root>
					<p class={note.warn ? 'text-warning' : 'text-muted-foreground'}>{note.text}</p>
					<input type="hidden" name="mode" value={mode} />
				</div>

				{#if mode === 'server'}
					<div class="space-y-2">
						<label for="{uid}-url" class="block font-medium">{t.address}</label>
						<Input
							id="{uid}-url"
							name="url"
							bind:value={url}
							required
							autocomplete="off"
							spellcheck="false"
							placeholder="http://localhost:11434/v1"
							class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
						/>
					</div>
				{/if}

				{#if mode === 'openai' || mode === 'openrouter' || mode === 'server'}
					<div class="space-y-2">
						<label for="{uid}-model" class="block font-medium">{t.model}</label>
						<Input
							id="{uid}-model"
							name="model"
							bind:value={model}
							required={mode === 'server'}
							autocomplete="off"
							spellcheck="false"
							placeholder={mode === 'server'
								? t.serverModel
								: mode === 'openai'
									? defaults.openai
									: defaults.openrouter}
							class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
						/>
					</div>
				{/if}

				{#if mode === 'server'}
					<div class="space-y-2">
						<label for="{uid}-key" class="block font-medium">
							{t.key} <span class="font-normal text-muted-foreground">{t.keyOptional}</span>
						</label>
						<Input
							id="{uid}-key"
							name="key"
							type="password"
							autocomplete="off"
							spellcheck="false"
							placeholder={setting.hasKey && url === setting.url ? t.keyKept : ''}
							class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
						/>
					</div>
				{/if}

				{#if result?.embeddingsError}
					<p class="text-destructive" role="alert">{result.embeddingsError}</p>
				{/if}

				<div class="flex gap-2">
					<Button type="submit" disabled={saving} class="h-10 px-5 max-sm:flex-1">
						{saving ? t.checking : m.common.save}
					</Button>
					<Button type="button" variant="ghost" class="h-10" onclick={() => (open = false)}>
						{m.common.cancel}
					</Button>
				</div>
			</form>
		{:else if result?.embeddingsWarning}
			<p class="text-warning sm:pl-12" role="status">{result.embeddingsWarning}</p>
		{:else if result?.embeddingsMessage}
			<p class="text-muted-foreground sm:pl-12" role="status">{result.embeddingsMessage}</p>
		{/if}
	</div>
</section>
