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
	 * to change it. `defaults`: each provider's model when none is typed. `servers`: the family's,
	 * whose models are `<server>/<model>`.
	 */
	let {
		setting,
		defaults,
		keys,
		servers,
		result
	}: {
		setting: EmbeddingState;
		defaults: { openai: string; openrouter: string };
		keys: Pick<ApiKeyStatus, 'provider' | 'label' | 'source'>[];
		servers: { name: string; url: string }[];
		result:
			| { embeddingsMessage?: string; embeddingsWarning?: string; embeddingsError?: string }
			| null
			| undefined;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.embeddings);
	const uid = $props.id();
	const MODES: Mode[] = ['auto', 'openai', 'openrouter', 'custom-openai', 'off'];

	let open = $state(false);
	let saving = $state(false);
	let mode = $state<Mode>('auto');
	let model = $state('');
	let server = $state('');
	const serverUrl = $derived(servers.find((s) => s.name === server)?.url);

	const key = (provider: string) => keys.find((k) => k.provider === provider);
	const hasKey = (provider: string) => !!key(provider)?.source;
	const label = (provider: string) => key(provider)?.label ?? provider;

	/** What the row says is in use, or why search goes by words only. */
	const status = $derived.by(() => {
		if (setting.using) return { warn: false, text: t.using(setting.using) };
		if (setting.mode === 'off') return { warn: false, text: t.off };
		if (setting.mode === 'custom-openai') return { warn: true, text: t.noServer };
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
		if (mode === 'custom-openai') {
			return serverUrl
				? { warn: false, text: t.customNote(server, serverUrl) }
				: { warn: true, text: t.noServer };
		}
		return { warn: false, text: t.offNote };
	});

	/** The form starts from what's saved; a server's model is `<server>/<model>`. */
	function start() {
		mode = setting.mode;
		model = setting.model ?? '';
		server = servers[0]?.name ?? '';
		if (mode === 'custom-openai') {
			const slash = model.indexOf('/');
			server = model.slice(0, Math.max(0, slash)) || server;
			model = model.slice(slash + 1);
		}
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

				{#if mode === 'custom-openai' && servers.length > 1}
					<div class="space-y-2">
						<div id="{uid}-server" class="font-medium">{t.server}</div>
						<ToggleGroup.Root
							type="single"
							variant="outline"
							size="sm"
							value={server}
							onValueChange={(value) => {
								if (value) server = value;
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

				{#if mode === 'openai' || mode === 'openrouter' || mode === 'custom-openai'}
					<div class="space-y-2">
						<label for="{uid}-model" class="block font-medium">{t.model}</label>
						<Input
							id="{uid}-model"
							name={mode === 'custom-openai' ? undefined : 'model'}
							bind:value={model}
							required={mode === 'custom-openai'}
							autocomplete="off"
							spellcheck="false"
							placeholder={mode === 'custom-openai'
								? t.customModel
								: mode === 'openai'
									? defaults.openai
									: defaults.openrouter}
							class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
						/>
						{#if mode === 'custom-openai'}
							<!-- A server's model, as a Custom OpenAI preset keeps it. -->
							<input type="hidden" name="model" value="{server}/{model.trim()}" />
						{/if}
					</div>
				{/if}

				{#if result?.embeddingsError}
					<p class="text-destructive" role="alert">{result.embeddingsError}</p>
				{/if}

				<div class="flex gap-2">
					<Button
						type="submit"
						disabled={saving || (mode === 'custom-openai' && !serverUrl)}
						class="h-10 px-5 max-sm:flex-1"
					>
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
