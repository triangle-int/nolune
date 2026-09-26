<script lang="ts">
	import { onMount } from 'svelte';
	import { cubicOut } from 'svelte/easing';
	import { Tween, prefersReducedMotion } from 'svelte/motion';
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import type { SubmitFunction } from '@sveltejs/kit';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import EraserIcon from '@lucide/svelte/icons/eraser';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Markdown from '$lib/components/chat/Markdown.svelte';
	import DotGrid, { orderTopics, type Topic } from '$lib/components/memory/DotGrid.svelte';
	import { formatAgo } from '$lib/format';
	import { memoryAnchor, memoryTopic } from '$lib/memory';
	import { cn } from '$lib/utils';

	let { data, form } = $props();

	const WEEK = 7 * 24 * 60 * 60 * 1000;

	const topics: Topic[] = $derived(
		data.files.map((file) => ({
			path: file.path,
			title: memoryTopic(file.path),
			group: file.path.includes('/') ? memoryTopic(file.path.split('/')[0]) : null,
			updatedAt: file.updatedAt,
			facts: file.facts
		}))
	);
	/** The notes below follow the grid's order. */
	const files = $derived.by(() => {
		const byPath = new Map(data.files.map((file) => [file.path, file]));
		return orderTopics(topics).map((topic) => ({ topic, file: byPath.get(topic.path)! }));
	});
	const total = $derived(topics.reduce((sum, topic) => sum + topic.facts.length, 0));
	const thisWeek = $derived(
		topics
			.flatMap((topic) => topic.facts)
			.filter((fact) => fact.learnedAt !== null && Date.now() - fact.learnedAt < WEEK).length
	);
	const updatedAt = $derived(Math.max(0, ...data.files.map((file) => file.updatedAt)));

	const count = new Tween(0, { duration: 1200, easing: cubicOut });
	$effect(() => {
		count.set(total, { duration: prefersReducedMotion.current ? 0 : 1200 });
	});

	const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

	/** The topic pointed at, here or in the grid. */
	let focus = $state<string | null>(null);
	/** A card that was just jumped to. */
	let flash = $state<string | null>(null);
	let editing = $state<string | null>(null);
	let draft = $state('');
	let saving = $state(false);
	let forgetting = $state<string | null>(null);

	function pick(path: string) {
		document.getElementById(memoryAnchor(path))?.scrollIntoView({
			behavior: prefersReducedMotion.current ? 'auto' : 'smooth',
			block: 'center'
		});
		flash = path;
		setTimeout(() => {
			if (flash === path) flash = null;
		}, 1600);
	}

	/** Markdown renders in the browser only, and {@html} isn't redone when the page hydrates. */
	let mounted = $state(false);

	// Chat steps link to a note with #memory-…; point it out.
	onMount(() => {
		mounted = true;
		const file = data.files.find((f) => `#${memoryAnchor(f.path)}` === location.hash);
		if (file) pick(file.path);
	});

	const save: SubmitFunction = () => {
		saving = true;
		return async ({ result, update }) => {
			saving = false;
			if (result.type === 'success') editing = null;
			// btw changed the note meanwhile: load its version, so saving again replaces it knowingly.
			if (result.type === 'failure' && result.status === 409) await invalidateAll();
			await update({ reset: false });
		};
	};
</script>

<PageHeader>
	<span class="truncate text-lg font-medium">Memory</span>
</PageHeader>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto max-w-3xl space-y-6 px-4 py-6 sm:py-10">
		<p class="text-muted-foreground">
			What btw remembers for {data.profile.name}, shared by everyone in it. It looks here when a
			chat starts and saves what it learns along the way. To add something, just tell it in a chat,
			like "Remember that Anna is allergic to nuts".
		</p>

		<div class="space-y-3">
			<div class="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 px-1">
				<p class="flex items-baseline gap-2">
					<span class="text-3xl font-semibold tabular-nums">{Math.round(count.current)}</span>
					<span class="text-muted-foreground">
						{total === 1 ? 'memory' : 'memories'} in {plural(topics.length, 'topic', 'topics')}
					</span>
				</p>
				{#if updatedAt}
					<p class="text-sm text-muted-foreground">
						{#if thisWeek}{thisWeek} new this week ·{/if}
						updated {formatAgo(updatedAt)}
					</p>
				{/if}
			</div>
			<DotGrid {topics} bind:focus onpick={pick} />
		</div>

		{#if form?.message && !form.path}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm">{form.message}</p>
		{/if}

		{#each files as { topic, file } (file.path)}
			{@const lit = focus === file.path || flash === file.path}
			<section
				id={memoryAnchor(file.path)}
				aria-label={topic.title}
				class={cn(
					'scroll-mt-6 rounded-3xl border p-4 transition-[border-color,box-shadow] duration-300 sm:p-5',
					lit && 'border-foreground/30 shadow-[0_0_0_4px_var(--muted)]'
				)}
				onpointerenter={() => (focus = file.path)}
				onpointerleave={() => (focus = null)}
			>
				<div class="flex items-start gap-3">
					<div class="min-w-0 flex-1">
						<h2 class="font-medium">{topic.title}</h2>
						<p class="truncate text-xs text-muted-foreground">
							{file.path} · {plural(topic.facts.length, 'memory', 'memories')} · updated {formatAgo(
								file.updatedAt
							)}
						</p>
					</div>
					{#if editing !== file.path}
						<Button
							variant="ghost"
							size="icon-sm"
							class="-mt-1 text-muted-foreground"
							aria-label="Edit {topic.title}"
							onclick={() => {
								editing = file.path;
								draft = file.text;
							}}
						>
							<PencilIcon />
						</Button>
						<Button
							variant="ghost"
							size="icon-sm"
							class="-mt-1 -mr-1 text-muted-foreground hover:text-destructive"
							aria-label="Forget {topic.title}"
							onclick={() => (forgetting = file.path)}
						>
							<EraserIcon />
						</Button>
					{/if}
				</div>

				{#if editing === file.path}
					<form method="POST" action="?/save" use:enhance={save} class="mt-3 space-y-3">
						<input type="hidden" name="path" value={file.path} />
						<input type="hidden" name="basedOn" value={file.updatedAt} />
						<Textarea
							name="text"
							bind:value={draft}
							rows={Math.min(18, Math.max(5, draft.split('\n').length + 1))}
							class="rounded-2xl font-mono text-xs"
							aria-label="{topic.title} note"
						/>
						{#if form?.path === file.path && form.message}
							<p class="text-sm {'conflict' in form ? 'text-warning' : 'text-destructive'}">
								{form.message}
							</p>
						{/if}
						<div class="flex gap-2">
							<Button type="submit" size="sm" disabled={saving}>Save</Button>
							<Button type="button" variant="ghost" size="sm" onclick={() => (editing = null)}
								>Cancel</Button
							>
						</div>
					</form>
				{:else}
					{#if form?.path === file.path && form.message}
						<p class="mt-3 rounded-2xl bg-muted px-4 py-2 text-sm">{form.message}</p>
					{/if}
					{#if !mounted}
						<p class="mt-3 text-sm whitespace-pre-line">{file.text}</p>
					{:else if /\.(md|markdown|txt)$/i.test(file.path) || !/\.[^/]+$/.test(file.path)}
						<Markdown text={file.text} class="mt-3 text-sm" />
					{:else}
						<pre
							class="mt-3 max-h-96 overflow-auto rounded-2xl bg-muted/50 p-3 font-mono text-xs whitespace-pre-wrap">{file.text}</pre>
					{/if}
				{/if}
			</section>
		{/each}
	</div>
</div>

<AlertDialog.Root open={forgetting !== null} onOpenChange={(open) => !open && (forgetting = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Forget "{forgetting ? memoryTopic(forgetting) : ''}"?</AlertDialog.Title>
			<AlertDialog.Description>
				btw forgets everything in <strong class="text-foreground">{forgetting}</strong> for everyone
				in {data.profile.name}. Chats that already read it keep what they read.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/forget"
			use:enhance={() =>
				async ({ update }) => {
					forgetting = null;
					await update();
				}}
		>
			<input type="hidden" name="path" value={forgetting ?? ''} />
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">Cancel</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive">Forget</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
