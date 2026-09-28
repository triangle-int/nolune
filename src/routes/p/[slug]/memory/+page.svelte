<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { cubicOut } from 'svelte/easing';
	import { Tween, prefersReducedMotion } from 'svelte/motion';
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import type { SubmitFunction } from '@sveltejs/kit';
	import { MEMORY_CATEGORIES, categoryOf } from '@btw/core/memory-categories';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import EraserIcon from '@lucide/svelte/icons/eraser';
	import FolderInputIcon from '@lucide/svelte/icons/folder-input';
	import PinIcon from '@lucide/svelte/icons/pin';
	import { Button } from '$lib/components/ui/button';
	import { Switch } from '$lib/components/ui/switch';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Rich from '$lib/components/Rich.svelte';
	import Markdown from '$lib/components/chat/Markdown.svelte';
	import DotGrid, { orderTopics, type Topic } from '$lib/components/memory/DotGrid.svelte';
	import MemoryChangeItem from '$lib/components/memory/MemoryChangeItem.svelte';
	import MoveNoteDialog from '$lib/components/memory/MoveNoteDialog.svelte';
	import { formatAgo } from '$lib/format';
	import { getI18n } from '$lib/i18n';
	import { memoryAnchor, memoryTitle } from '$lib/memory';
	import { cn } from '$lib/utils';

	let { data, form } = $props();

	const i18n = getI18n();
	const { m } = i18n;

	const WEEK = 7 * 24 * 60 * 60 * 1000;
	/** Of what the note-taker saved lately, how many show before "Show all". */
	const RECENT_SHOWN = 4;
	const slug = $derived(page.params.slug ?? '');
	let allRecent = $state(false);
	const recent = $derived(allRecent ? data.recent : data.recent.slice(0, RECENT_SHOWN));

	const whose = $derived(new Map(data.members.map((member) => [member.note, member.name])));
	/**
	 * By category: a note each, then a folder's notes under its name. Notes from before the
	 * categories come last, as unsorted.
	 */
	const topics: Topic[] = $derived(
		data.files.map((file) => {
			const category = categoryOf(file.path);
			const folder = !!category && file.path.includes('/');
			return {
				path: file.path,
				title: memoryTitle(m, file.path, file.text),
				group: !category
					? m.memory.categories.unsorted
					: folder
						? m.memory.categories[category]
						: null,
				rank: category
					? MEMORY_CATEGORIES.indexOf(category) + (folder ? 0.5 : 0)
					: MEMORY_CATEGORIES.length,
				member: whose.get(file.path) ?? null,
				updatedAt: file.updatedAt,
				facts: file.facts
			};
		})
	);
	/**
	 * The notes below follow the grid's order, after the pinned core note. Core is there even
	 * before it exists, so people can start it here.
	 */
	const files = $derived.by(() => {
		const byPath = new Map(data.files.map((file) => [file.path, file]));
		const ordered = orderTopics(topics).map((topic) => ({ topic, file: byPath.get(topic.path)! }));
		const at = ordered.findIndex(({ file }) => file.path === data.core.path);
		const core =
			at === -1
				? {
						topic: {
							path: data.core.path,
							title: memoryTitle(m, data.core.path),
							group: null,
							rank: 0,
							member: null,
							updatedAt: 0,
							facts: []
						},
						file: { path: data.core.path, text: '', facts: [], size: 0, updatedAt: 0 }
					}
				: ordered.splice(at, 1)[0];
		return [core, ...ordered];
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

	/** The topic pointed at, here or in the grid. */
	let focus = $state<string | null>(null);
	/** A card that was just jumped to. */
	let flash = $state<string | null>(null);
	let editing = $state<string | null>(null);
	let draft = $state('');
	let saving = $state(false);
	let forgetting = $state<string | null>(null);
	let moving = $state<string | null>(null);
	const titles = $derived(topics.map(({ path, title }) => ({ path, title })));

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

	let learnForm: HTMLFormElement | undefined = $state();
	let learnOn = $state(true);

	async function setLearning(on: boolean) {
		learnOn = on;
		await tick();
		learnForm?.requestSubmit();
	}

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
	<span class="truncate text-lg font-medium">{m.memory.title}</span>
</PageHeader>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto max-w-3xl space-y-6 px-4 py-6 sm:py-10">
		<p class="text-muted-foreground">
			{m.memory.intro(data.profile.name)}
		</p>

		<form method="POST" action="?/learn" use:enhance bind:this={learnForm}>
			<input type="hidden" name="on" value={learnOn ? 'on' : 'off'} />
			<label class="flex cursor-pointer items-start gap-4 rounded-2xl border px-4 py-3">
				<span class="min-w-0 flex-1 space-y-0.5">
					<span class="block font-medium">{m.memory.learn}</span>
					<span class="block text-sm text-muted-foreground">{m.memory.learnHint}</span>
				</span>
				<Switch
					class="mt-0.5"
					checked={data.learnFromChats}
					onCheckedChange={setLearning}
					aria-label={m.memory.learn}
				/>
			</label>
		</form>

		<div class="space-y-3">
			<div class="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 px-1">
				<p class="flex items-baseline gap-2">
					<span class="text-3xl font-semibold tabular-nums">{Math.round(count.current)}</span>
					<span class="text-muted-foreground">
						{m.memory.total(total, topics.length)}
					</span>
				</p>
				{#if updatedAt}
					<p class="text-sm text-muted-foreground">
						{#if thisWeek}{m.memory.newThisWeek(thisWeek)} ·{/if}
						{m.memory.updated(formatAgo(updatedAt, i18n))}
					</p>
				{/if}
			</div>
			<DotGrid {topics} bind:focus onpick={pick} />
		</div>

		{#if data.recent.length}
			<section class="space-y-3" aria-labelledby="recent-heading">
				<div class="space-y-0.5 px-1">
					<h2 id="recent-heading" class="font-medium">{m.memory.changes.recent}</h2>
					<p class="text-sm text-muted-foreground">{m.memory.changes.recentHint}</p>
				</div>
				<ul class="space-y-3 rounded-3xl border p-4 text-sm sm:p-5">
					{#each recent as change (change.id)}
						<MemoryChangeItem {change} {slug} onundone={invalidateAll}>
							{#snippet note(topic)}
								<button
									type="button"
									class="underline-offset-2 hover:text-foreground hover:underline"
									onclick={() => pick(change.note)}>{topic}</button
								>
							{/snippet}
							{#snippet after()}
								<span>
									{#if change.conversation}
										<Rich text={m.memory.changes.fromChat}>
											{#snippet chat()}<a
													href={resolve('/p/[slug]/c/[id]', { slug, id: change.conversation!.id })}
													class="underline-offset-2 hover:text-foreground hover:underline"
													>{change.conversation!.title || m.memory.changes.untitled}</a
												>{/snippet}
										</Rich>
									{:else}
										{m.memory.changes.deletedChat}
									{/if}
								</span>
								<span aria-hidden="true">·</span>
								<span>{formatAgo(change.createdAt, i18n)}</span>
							{/snippet}
						</MemoryChangeItem>
					{/each}
				</ul>
				{#if data.recent.length > RECENT_SHOWN}
					<Button
						variant="ghost"
						size="sm"
						class="text-muted-foreground"
						onclick={() => (allRecent = !allRecent)}
					>
						{allRecent ? m.memory.showFewer : m.memory.changes.showAll(data.recent.length)}
					</Button>
				{/if}
			</section>
		{/if}

		{#if form?.message && !form.path}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm">{form.message}</p>
		{/if}

		{#each files as { topic, file } (file.path)}
			{@const lit = focus === file.path || flash === file.path}
			{@const pinned = file.path === data.core.path}
			{@const missing = pinned && !file.updatedAt}
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
						<h2 class="flex items-center gap-1.5 font-medium">
							{#if pinned}<PinIcon class="size-3.5 text-muted-foreground" />{/if}
							{topic.title}
						</h2>
						<p class="truncate text-xs text-muted-foreground">
							{[
								file.path,
								topic.member && m.memory.memberNote(topic.member),
								pinned && m.memory.pinned,
								!missing && m.memory.memories(topic.facts.length),
								!missing && m.memory.updated(formatAgo(file.updatedAt, i18n))
							]
								.filter(Boolean)
								.join(' · ')}
						</p>
						{#if !categoryOf(file.path)}
							<p class="mt-1 text-xs text-warning">{m.memory.unsortedHint}</p>
						{/if}
					</div>
					{#if editing !== file.path}
						<Button
							variant="ghost"
							size="icon-sm"
							class="-mt-1 text-muted-foreground"
							aria-label={m.memory.edit(topic.title)}
							onclick={() => {
								editing = file.path;
								draft = file.text;
							}}
						>
							<PencilIcon />
						</Button>
						{#if !pinned}
							<Button
								variant="ghost"
								size="icon-sm"
								class="-mt-1 text-muted-foreground"
								aria-label={m.memory.move.button(topic.title)}
								title={m.memory.move.button(topic.title)}
								onclick={() => (moving = file.path)}
							>
								<FolderInputIcon />
							</Button>
						{/if}
						{#if !missing}
							<Button
								variant="ghost"
								size="icon-sm"
								class="-mt-1 -mr-1 text-muted-foreground hover:text-destructive"
								aria-label={m.memory.forget(topic.title)}
								onclick={() => (forgetting = file.path)}
							>
								<EraserIcon />
							</Button>
						{/if}
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
							aria-label={m.memory.note(topic.title)}
							placeholder={pinned ? m.memory.corePlaceholder : undefined}
						/>
						{#if form?.path === file.path && form.message}
							<p class="text-sm {'conflict' in form ? 'text-warning' : 'text-destructive'}">
								{form.message}
							</p>
						{/if}
						<div class="flex items-center gap-2">
							<Button type="submit" size="sm" disabled={saving}>{m.common.save}</Button>
							<Button type="button" variant="ghost" size="sm" onclick={() => (editing = null)}
								>{m.common.cancel}</Button
							>
							{#if pinned}
								<span
									class={cn(
										'ml-auto text-xs text-muted-foreground tabular-nums',
										draft.length > data.core.maxChars && 'text-destructive'
									)}
								>
									{m.common.characters(draft.length, data.core.maxChars)}
								</span>
							{/if}
						</div>
					</form>
				{:else}
					{#if form?.path === file.path && form.message}
						<p class="mt-3 rounded-2xl bg-muted px-4 py-2 text-sm">{form.message}</p>
					{/if}
					{#if missing}
						<p class="mt-3 text-sm text-muted-foreground">
							{m.memory.coreEmpty}
						</p>
					{:else if !mounted}
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

<MoveNoteDialog bind:path={moving} notes={titles} />

<AlertDialog.Root open={forgetting !== null} onOpenChange={(open) => !open && (forgetting = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title
				>{m.memory.forgetTitle(
					titles.find((note) => note.path === forgetting)?.title ?? forgetting ?? ''
				)}</AlertDialog.Title
			>
			<AlertDialog.Description>
				<Rich text={m.memory.forgetBody} profile={data.profile.name}>
					{#snippet path()}<strong class="text-foreground">{forgetting}</strong>{/snippet}
				</Rich>
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
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive"
					>{m.memory.forgetButton}</AlertDialog.Action
				>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
