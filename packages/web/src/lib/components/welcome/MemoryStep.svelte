<script lang="ts">
	import { enhance } from '$app/forms';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import type { Avatar } from '@nolune/core/avatars';
	import {
		EXPORT_SECTIONS,
		countSections,
		parseMemoryExport,
		type ExportedFact
	} from '@nolune/core/memory-export';
	import type { ImportedNote } from '@nolune/core';
	import AssistantAvatar from '$lib/components/AssistantAvatar.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { copyText } from '$lib/clipboard';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	interface Props {
		prompt: string;
		avatar: Avatar;
		/** Saved: what was added, and the lines it came from, for the dots to fly out of. */
		onimported: (imported: { notes: ImportedNote[]; added: number }, lines: ExportedFact[]) => void;
		/** Nothing to bring over. */
		onskip: () => void;
	}

	let { prompt, avatar, onimported, onskip }: Props = $props();
	const { m } = getI18n();

	/** Where people keep what they'd paste. */
	const ASSISTANTS = [
		{ name: 'ChatGPT', url: 'https://chatgpt.com/' },
		{ name: 'Claude', url: 'https://claude.ai/new' },
		{ name: 'Gemini', url: 'https://gemini.google.com/app' }
	];

	let view = $state<'ask' | 'paste'>('ask');
	let copied = $state(false);
	let text = $state('');
	let busy = $state(false);
	let problem = $state<string | null>(null);

	const facts = $derived(parseMemoryExport(text));
	const counts = $derived(countSections(facts));

	async function copy() {
		await copyText(prompt);
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}
</script>

{#if view === 'ask'}
	<div class="space-y-7">
		<div class="space-y-3 text-center">
			<h2
				class="flex items-center justify-center gap-3 text-3xl font-semibold tracking-tight text-balance sm:text-4xl"
			>
				<AssistantAvatar {avatar} mood="idle" size={40} class="max-sm:hidden" />
				{m.welcome.memory.title}
			</h2>
			<p class="mx-auto max-w-lg text-muted-foreground">{m.welcome.memory.subtitle}</p>
		</div>

		<div class="overflow-hidden rounded-3xl border bg-card">
			<div class="flex items-center justify-between gap-3 border-b py-2 pr-2 pl-5">
				<span class="text-sm text-muted-foreground">{m.welcome.memory.prompt}</span>
				<Button variant="outline" size="sm" class="bg-card dark:bg-card" onclick={copy}>
					{#if copied}<CheckIcon />{m.common.copied}{:else}<CopyIcon />{m.welcome.memory
							.copyPrompt}{/if}
				</Button>
			</div>
			<pre
				class="max-h-40 overflow-hidden [mask-image:linear-gradient(to_bottom,black_55%,transparent)] px-5 pt-4 pb-6 font-mono text-[13px] leading-relaxed whitespace-pre-wrap text-foreground/80">{prompt}</pre>
		</div>

		<div class="flex flex-wrap justify-center gap-2">
			{#each ASSISTANTS as assistant (assistant.name)}
				<Button
					variant="outline"
					href={assistant.url}
					target="_blank"
					rel="noreferrer"
					class="rounded-full px-4"
				>
					{m.welcome.memory.open(assistant.name)}
					<ArrowUpRightIcon />
				</Button>
			{/each}
		</div>

		<div class="flex flex-wrap items-center justify-center gap-2">
			<Button size="lg" class="h-11 min-w-44 px-8" onclick={() => (view = 'paste')}>
				{m.welcome.memory.haveIt}
			</Button>
			<Button variant="ghost" size="lg" class="h-11 text-muted-foreground" onclick={onskip}>
				{m.welcome.memory.startFresh}
			</Button>
		</div>
	</div>
{:else}
	<form
		method="POST"
		action="?/remember"
		class="space-y-5"
		use:enhance={() => {
			busy = true;
			problem = null;
			const lines = facts;
			return async ({ result }) => {
				busy = false;
				if (result.type === 'success' && result.data?.imported) {
					onimported(
						result.data.imported as { notes: ImportedNote[]; added: number },
						// Rewritten by the model when it didn't parse: then the dots start from the box.
						lines
					);
				} else if (result.type === 'failure') {
					problem = String(result.data?.rememberError ?? '');
				} else if (result.type === 'error') {
					problem = result.error?.message ?? m.errors.requestFailed(500);
				}
			};
		}}
	>
		<h2 class="text-center text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
			{m.welcome.memory.pasteTitle}
		</h2>
		<div class="relative">
			<Textarea
				name="text"
				bind:value={text}
				required
				rows={11}
				spellcheck="false"
				autofocus
				aria-label={m.welcome.memory.pasteTitle}
				placeholder={m.welcome.memory.pastePlaceholder}
				class={cn(
					'max-h-[45dvh] min-h-56 rounded-3xl px-5 py-4 font-mono text-[13px] leading-relaxed transition-shadow',
					text &&
						'border-(--tone)/60 shadow-[0_0_0_4px_color-mix(in_oklab,var(--tone)_18%,transparent)]'
				)}
				style="--tone: var(--avatar-{avatar})"
			/>
			<!-- Reading along. -->
			<AssistantAvatar
				{avatar}
				mood={busy ? 'working' : text ? 'thinking' : 'idle'}
				size={36}
				class="pointer-events-none absolute -top-5 right-5"
			/>
		</div>

		<div class="flex flex-wrap justify-center gap-2" aria-live="polite">
			{#each EXPORT_SECTIONS as section (section)}
				{#if section !== 'other' || counts.other}
					<span
						class={cn(
							'flex items-center gap-2 rounded-full border py-1 pr-1.5 pl-3 text-sm transition-colors',
							counts[section] ? 'text-foreground' : 'text-muted-foreground'
						)}
					>
						{m.welcome.memory.sections[section]}
						<span
							class={cn(
								'min-w-6 rounded-full px-1.5 text-center text-xs leading-5 tabular-nums',
								counts[section] ? 'bg-foreground text-background' : 'bg-muted'
							)}>{counts[section]}</span
						>
					</span>
				{/if}
			{/each}
		</div>

		{#if problem}
			<p class="text-center text-sm text-destructive" role="alert">{problem}</p>
		{/if}

		<div class="flex flex-wrap items-center justify-center gap-2">
			<Button
				type="button"
				variant="ghost"
				size="lg"
				class="h-11"
				onclick={() => {
					view = 'ask';
					problem = null;
				}}
			>
				{m.welcome.back}
			</Button>
			<Button type="submit" size="lg" class="h-11 min-w-44 px-8" disabled={busy || !text.trim()}>
				{busy
					? facts.length
						? m.welcome.memory.saving
						: m.welcome.memory.reading
					: facts.length
						? m.welcome.memory.remember(facts.length)
						: m.welcome.memory.rememberThis}
			</Button>
		</div>
	</form>
{/if}
