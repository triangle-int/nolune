<script lang="ts">
	import { untrack } from 'svelte';
	import { deserialize, enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
	import FileIcon from '@lucide/svelte/icons/file';
	import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
	import MessageSquareIcon from '@lucide/svelte/icons/message-square';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import TypedText from '$lib/components/TypedText.svelte';
	import NewChatForm from '$lib/components/chat/NewChatForm.svelte';
	import DeleteFolderDialog from '$lib/components/folders/DeleteFolderDialog.svelte';
	import RenameFolderDialog from '$lib/components/folders/RenameFolderDialog.svelte';
	import type { FolderItem } from '$lib/folders';
	import { formatAgo, formatBytes } from '$lib/format';
	import { getI18n } from '$lib/i18n';
	import { getPreferences } from '$lib/preferences.svelte';
	import { Attachments } from '$lib/uploads.svelte';
	import { cn } from '$lib/utils';

	let { data } = $props();

	const prefs = getPreferences();
	const i18n = getI18n();
	const { m } = i18n;
	const slug = $derived(data.profile.slug);
	const chats = $derived(data.conversations.filter((c) => c.folderId === data.folder.id));

	let renaming = $state<FolderItem | null>(null);
	let deleting = $state<FolderItem | null>(null);

	// --- instructions ---

	/** What's in the box; follows the saved text until someone types. */
	let instructions = $derived(data.folder.instructions);
	const edited = $derived(instructions.trim() !== data.folder.instructions);
	let savingInstructions = $state(false);
	let instructionsProblem = $state<string | null>(null);

	/**
	 * Grows with the text up to its max height, then scrolls. Not with `field-sizing: content`
	 * (the Textarea's default): Safari then lays the placeholder out wider than the box, cut off
	 * and scrolling sideways.
	 */
	function growWithText(node: HTMLTextAreaElement) {
		void instructions;
		node.style.minHeight = '';
		const border = node.offsetHeight - node.clientHeight;
		const max = parseFloat(getComputedStyle(node).maxHeight) || Infinity;
		node.style.minHeight = `${Math.min(node.scrollHeight + border, max)}px`;
	}

	// --- files ---

	const uploads = new Attachments(() => slug, m);
	let fileInput = $state<HTMLInputElement>();
	let dragging = $state(false);
	let adding = $state(false);
	let filesProblem = $state<string | null>(null);
	const pending = $derived(uploads.files.filter((f) => f.status !== 'ready'));

	/** Once every picked file has uploaded, they're added to the folder in one go. */
	$effect(() => {
		if (adding || uploads.uploading || !uploads.ids.length) return;
		const ids = [...uploads.ids];
		untrack(() => addFiles(ids));
	});

	async function addFiles(ids: string[]) {
		adding = true;
		filesProblem = null;
		const body = new FormData();
		for (const id of ids) body.append('upload', id);
		try {
			const res = await fetch(`${page.url.pathname}?/addFiles`, {
				method: 'POST',
				body,
				headers: { 'x-sveltekit-action': 'true' }
			});
			const result = deserialize(await res.text());
			if (result.type === 'success') {
				await invalidateAll();
				uploads.forget(ids);
			} else {
				filesProblem =
					result.type === 'failure'
						? String(result.data?.message ?? m.folders.couldNotAdd)
						: m.folders.couldNotAdd;
				for (const f of uploads.files) if (f.id && ids.includes(f.id)) uploads.remove(f.key);
			}
		} catch {
			filesProblem = m.folders.couldNotAddOffline;
			for (const f of uploads.files) if (f.id && ids.includes(f.id)) uploads.remove(f.key);
		} finally {
			adding = false;
		}
	}

	function hasFiles(event: DragEvent): boolean {
		return !!event.dataTransfer?.types.includes('Files');
	}

	const fileHref = (id: string, download = false) =>
		`/api/p/${encodeURIComponent(slug)}/folders/${data.folder.id}/files/${id}${download ? '?download' : ''}`;
</script>

<PageHeader>
	<FolderOpenIcon class="size-5 shrink-0 text-muted-foreground" />
	<span class="truncate text-lg font-medium">{data.folder.name}</span>

	{#snippet actions()}
		<DropdownMenu.Root>
			<DropdownMenu.Trigger
				class="flex size-10 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground aria-expanded:bg-muted"
				aria-label={m.folders.options}
			>
				<EllipsisIcon class="size-5" />
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="end" class="w-48">
				<DropdownMenu.Item onSelect={() => (renaming = data.folder)}>
					<PencilIcon />
					{m.common.rename}
				</DropdownMenu.Item>
				<DropdownMenu.Item variant="destructive" onSelect={() => (deleting = data.folder)}>
					<Trash2Icon />
					{m.common.delete}
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{/snippet}
</PageHeader>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div
		class="mx-auto flex max-w-3xl flex-col gap-6 px-3 pt-4 pb-[max(2.5rem,env(safe-area-inset-bottom))] sm:px-4 sm:pt-10"
	>
		<h1 class="flex min-w-0 items-center gap-3 px-1 text-[28px] leading-tight tracking-tight">
			<FolderOpenIcon class="size-7 shrink-0" />
			<span class="truncate">{data.folder.name}</span>
		</h1>

		{#if data.presets.length}
			{#key data.folder.id}
				<NewChatForm
					{slug}
					presets={data.presets}
					defaultPresetId={data.defaultPresetId}
					efforts={data.efforts}
					folders={data.folders}
					folderId={data.folder.id}
					avatar={data.profile.avatar}
					placeholder={m.folders.newChatIn(data.folder.name)}
					autofocus
				/>
			{/key}
		{/if}

		<div class="grid gap-3 sm:grid-cols-2">
			<section class="flex flex-col rounded-3xl border p-4">
				<h2 class="font-medium">{m.folders.instructions}</h2>
				<p class="mt-0.5 text-sm text-muted-foreground">
					{m.folders.instructionsHint}
				</p>
				<form
					method="POST"
					action="?/instructions"
					class="mt-3 flex flex-1 flex-col"
					use:enhance={() => {
						savingInstructions = true;
						instructionsProblem = null;
						return async ({ result, update }) => {
							savingInstructions = false;
							if (result.type === 'failure') {
								instructionsProblem = String(result.data?.message ?? m.errors.couldNotSave);
							} else await update({ reset: false });
						};
					}}
				>
					<Textarea
						name="instructions"
						bind:value={instructions}
						{@attach growWithText}
						maxlength={data.maxInstructions}
						placeholder={m.folders.instructionsPlaceholder}
						class="field-sizing-fixed! max-h-80 min-h-28 flex-1"
					/>
					{#if instructionsProblem}
						<p class="mt-2 text-sm text-destructive">{instructionsProblem}</p>
					{/if}
					{#if edited}
						<div class="mt-2 flex justify-end gap-2">
							<Button
								variant="ghost"
								size="sm"
								onclick={() => (instructions = data.folder.instructions)}>{m.common.cancel}</Button
							>
							<Button type="submit" size="sm" disabled={savingInstructions}>{m.common.save}</Button>
						</div>
					{/if}
				</form>
				<p class="mt-2 text-xs text-muted-foreground">
					{prefs.technical ? m.folders.changesTechnical : m.folders.changes}
				</p>
			</section>

			<section
				class={cn(
					'flex flex-col rounded-3xl border p-4 transition-shadow',
					dragging && 'ring-2 ring-ring'
				)}
				aria-label={m.folders.files}
				ondragover={(event) => {
					if (!hasFiles(event)) return;
					event.preventDefault();
					dragging = true;
				}}
				ondragleave={(event) => {
					if (!event.currentTarget.contains(event.relatedTarget as Node | null)) dragging = false;
				}}
				ondrop={(event) => {
					if (!hasFiles(event)) return;
					event.preventDefault();
					dragging = false;
					if (event.dataTransfer?.files.length) uploads.add(event.dataTransfer.files);
				}}
			>
				<div class="flex items-center justify-between gap-2">
					<h2 class="font-medium">{m.folders.files}</h2>
					<Button
						variant="outline"
						size="sm"
						onclick={() => fileInput?.click()}
						disabled={data.files.length >= data.maxFiles}
					>
						<PlusIcon />
						{m.folders.addFiles}
					</Button>
					<input
						bind:this={fileInput}
						type="file"
						multiple
						hidden
						onchange={(event) => {
							const input = event.currentTarget;
							if (input.files?.length) uploads.add(input.files);
							input.value = '';
						}}
					/>
				</div>
				<p class="mt-0.5 text-sm text-muted-foreground">
					{m.folders.filesHint}
				</p>

				{#if data.files.length || pending.length}
					<ul class="mt-3 flex flex-col gap-1">
						{#each data.files as file (file.id)}
							<li class="group/file flex items-center gap-3 rounded-xl px-1 py-1 hover:bg-muted">
								<!-- eslint-disable svelte/no-navigation-without-resolve -- a file from an API route, not a page -->
								<a
									href={fileHref(file.id, !file.viewable)}
									target={file.viewable ? '_blank' : undefined}
									rel="noopener"
									class="flex min-w-0 flex-1 items-center gap-3"
								>
									{#if file.viewable}
										<img
											src={fileHref(file.id)}
											alt=""
											loading="lazy"
											class="size-10 shrink-0 rounded-lg border object-cover"
										/>
									{:else}
										<span
											class="flex size-10 shrink-0 items-center justify-center rounded-lg border bg-background"
										>
											<FileIcon class="size-5 text-muted-foreground" />
										</span>
									{/if}
									<span class="min-w-0 text-sm leading-tight">
										<span class="block truncate font-medium">{file.name}</span>
										<span class="block text-xs text-muted-foreground"
											>{formatBytes(file.bytes, i18n)}</span
										>
									</span>
								</a>
								<!-- eslint-enable svelte/no-navigation-without-resolve -->
								<form method="POST" action="?/removeFile" use:enhance>
									<input type="hidden" name="file" value={file.id} />
									<button
										type="submit"
										class="flex size-8 items-center justify-center rounded-full text-muted-foreground hover:bg-background hover:text-foreground md:opacity-0 md:group-hover/file:opacity-100 md:focus-visible:opacity-100"
										aria-label={m.common.removeFile(file.name)}
									>
										<XIcon class="size-4" />
									</button>
								</form>
							</li>
						{/each}
						{#each pending as file (file.key)}
							<li class="flex items-center gap-3 px-1 py-1">
								<span
									class="relative flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-lg border bg-background"
								>
									<FileIcon class="size-5 text-muted-foreground" />
									{#if file.status === 'uploading'}
										<span class="absolute inset-x-0 bottom-0 h-1 bg-muted">
											<span
												class="block h-full bg-primary transition-[width]"
												style:width="{Math.round(file.progress * 100)}%"
											></span>
										</span>
									{/if}
								</span>
								<span class="min-w-0 flex-1 text-sm leading-tight">
									<span class="block truncate font-medium">{file.name}</span>
									<span
										class={cn(
											'block truncate text-xs',
											file.status === 'failed' ? 'text-destructive' : 'text-muted-foreground'
										)}
									>
										{file.status === 'failed' ? file.error : m.common.uploading}
									</span>
								</span>
								<button
									type="button"
									onclick={() => uploads.remove(file.key)}
									class="flex size-8 items-center justify-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground"
									aria-label={m.common.removeFile(file.name)}
								>
									<XIcon class="size-4" />
								</button>
							</li>
						{/each}
					</ul>
				{:else}
					<p
						class="mt-3 flex flex-1 items-center justify-center rounded-2xl border border-dashed px-4 py-6 text-center text-sm text-muted-foreground"
					>
						{m.folders.noFiles}
					</p>
				{/if}
				{#if filesProblem}
					<p class="mt-2 text-sm text-destructive">{filesProblem}</p>
				{/if}
				{#if prefs.technical}
					<p class="mt-2 text-xs break-all text-muted-foreground">
						{m.folders.savedIn(data.folder.dir)}
					</p>
				{/if}
			</section>
		</div>

		<section>
			<h2 class="px-1 pb-2 font-medium">{m.folders.chats}</h2>
			{#if chats.length}
				<ul class="overflow-hidden rounded-3xl border">
					{#each chats as chat (chat.id)}
						<li class="border-b last:border-b-0">
							<a
								href={resolve('/p/[slug]/c/[id]', { slug, id: chat.id })}
								class="flex items-center gap-3 px-4 py-3 text-sm hover:bg-muted"
							>
								<MessageSquareIcon class="size-4 shrink-0 text-muted-foreground" />
								<span class="min-w-0 flex-1 truncate"><TypedText text={chat.title} /></span>
								<span class="shrink-0 text-xs text-muted-foreground">
									{formatAgo(chat.updatedAt, i18n)}
								</span>
							</a>
						</li>
					{/each}
				</ul>
			{:else}
				<p
					class="rounded-3xl border border-dashed px-4 py-6 text-center text-sm text-muted-foreground"
				>
					{m.folders.noChats}
				</p>
			{/if}
		</section>
	</div>
</div>

<RenameFolderDialog bind:folder={renaming} {slug} />
<DeleteFolderDialog bind:folder={deleting} {slug} />
