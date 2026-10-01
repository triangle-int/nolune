<script lang="ts">
	import type { DisplayMemoryChange } from '@nolune/core';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import type { Snippet } from 'svelte';
	import { page } from '$app/state';
	import { Button } from '$lib/components/ui/button';
	import { errorMessage } from '$lib/http';
	import { getI18n } from '$lib/i18n';
	import { memoryTitle } from '$lib/memory';
	import { cn } from '$lib/utils';

	/**
	 * One thing the note-taker saved: the fact as it reads now (and before, for a change), the note
	 * it's in, and Undo, which puts the note back as it was. On a card only its owner undoes it,
	 * or keeps it only in the profile it came from (`profile`, its name; the page's own profile when
	 * not given). `note`: the note's link,
	 * as each place wants it. `after`: more on the same line as the note, like the chat it came
	 * from. `onundone`: after an undo, for places that don't get it live.
	 */
	let {
		change,
		slug,
		profile,
		note,
		after,
		onundone
	}: {
		change: DisplayMemoryChange;
		slug: string;
		profile?: string;
		note: Snippet<[string]>;
		after?: Snippet;
		onundone?: () => void;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.memory.changes);
	const mine = $derived(!!change.card && change.card.ownerId === page.data.user?.id);
	const from = $derived(profile ?? (page.data.profile as { name: string } | undefined)?.name);
	const title = $derived(
		change.card
			? mine
				? m.memory.cards.yours
				: m.memory.cards.title(change.card.owner)
			: memoryTitle(m, change.note)
	);

	let undoing = $state(false);
	let problem = $state<string | null>(null);

	/** What went wrong with a request, in the reader's language. */
	async function failed(res: Response): Promise<string> {
		const text = await res.text();
		const reason =
			res.status === 409 ? (JSON.parse(text) as { reason?: string }).reason : undefined;
		if (reason === 'changed') return t.changedSince;
		if (reason === 'undone') return t.alreadyUndone;
		if (reason === 'owner' && change.card) return t.ownerOnly(change.card.owner);
		if (reason) return reason;
		return (
			errorMessage(text, res.headers.get('content-type')) ?? m.errors.requestFailed(res.status)
		);
	}

	async function send(url: string) {
		undoing = true;
		problem = null;
		try {
			const res = await fetch(url, { method: 'POST' });
			if (res.ok) {
				onundone?.();
				return;
			}
			problem = await failed(res);
		} catch {
			problem = m.errors.requestFailed(0);
		} finally {
			undoing = false;
		}
	}
</script>

<li class="flex items-start gap-2">
	<!-- Relative, so the label in it stays here instead of stretching the page that scrolls it. -->
	<span
		class="relative mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground"
		title={change.op === 'add' ? t.added : t.changed}
	>
		{#if change.op === 'add'}<PlusIcon class="size-3" />{:else}<PencilIcon class="size-3" />{/if}
		<span class="sr-only">{change.op === 'add' ? t.added : t.changed}</span>
	</span>
	<div class="min-w-0 flex-1 space-y-0.5">
		<p class={cn('break-words', change.undone && 'text-muted-foreground line-through')}>
			{change.fact}
		</p>
		{#if change.before !== null}
			<p class="text-xs break-words text-muted-foreground">{t.before(change.before)}</p>
		{/if}
		<p class="flex flex-wrap items-center gap-x-1.5 text-xs text-muted-foreground">
			{@render note(title)}
			{#if after}<span aria-hidden="true">·</span>{@render after()}{/if}
		</p>
		{#if problem}
			<p class="text-xs text-destructive" role="alert">{problem}</p>
		{/if}
	</div>
	{#if change.undone}
		<span class="shrink-0 py-1 text-xs text-muted-foreground">
			{change.undone.by ? t.undoneBy(change.undone.by) : t.undone}
		</span>
	{:else if !change.card || mine}
		<div class="-my-1 flex shrink-0 items-center">
			{#if mine && from}
				<Button
					variant="ghost"
					size="sm"
					class="h-7 px-2 text-xs text-muted-foreground"
					title={t.keepHereHint(from)}
					disabled={undoing}
					onclick={() => send(`/api/card/changes/${change.id}/keep`)}
				>
					{t.keepHere}
				</Button>
			{/if}
			<Button
				variant="ghost"
				size="sm"
				class="h-7 px-2 text-xs text-muted-foreground"
				disabled={undoing}
				onclick={() => send(`/api/p/${slug}/memory/${change.id}/undo`)}
			>
				{t.undo}
			</Button>
		</div>
	{/if}
</li>
