<script lang="ts">
	import { untrack } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import { scale, slide } from 'svelte/transition';
	import { Portal } from 'bits-ui';
	import { enhance } from '$app/forms';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import SquarePenIcon from '@lucide/svelte/icons/square-pen';
	import SearchIcon from '@lucide/svelte/icons/search';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import ImagesIcon from '@lucide/svelte/icons/images';
	import BrainIcon from '@lucide/svelte/icons/brain';
	import PuzzleIcon from '@lucide/svelte/icons/puzzle';
	import UsersIcon from '@lucide/svelte/icons/users';
	import CheckIcon from '@lucide/svelte/icons/check';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import PanelLeftIcon from '@lucide/svelte/icons/panel-left';
	import MessageCircleIcon from '@lucide/svelte/icons/message-circle';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
	import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import type { Avatar } from '@btw/core/avatars';
	import * as Sidebar from '$lib/components/ui/sidebar';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Command from '$lib/components/ui/command';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { Kbd } from '$lib/components/ui/kbd';
	import { ChatDrag, SETTLE_MS } from '$lib/chat-drag.svelte';
	import { moveChat, type FolderItem } from '$lib/folders';
	import { cn } from '$lib/utils';
	import AssistantAvatar from './AssistantAvatar.svelte';
	import RenameChatDialog from './chat/RenameChatDialog.svelte';
	import DeleteFolderDialog from './folders/DeleteFolderDialog.svelte';
	import MoveToFolderMenu from './folders/MoveToFolderMenu.svelte';
	import NewFolderDialog from './folders/NewFolderDialog.svelte';
	import RenameFolderDialog from './folders/RenameFolderDialog.svelte';
	import TypedText from './TypedText.svelte';
	import UserMenu from './UserMenu.svelte';

	type ChatItem = { id: string; title: string; folderId: string | null };

	interface Props {
		profile: { slug: string; name: string; avatar: Avatar };
		profiles: { slug: string; name: string; avatar: Avatar }[];
		folders: FolderItem[];
		conversations: ChatItem[];
		user: { name: string; email: string; isAdmin: boolean };
	}

	let { profile, profiles, folders, conversations, user }: Props = $props();

	const sidebar = Sidebar.useSidebar();

	let searchOpen = $state(false);
	let renaming = $state<ChatItem | null>(null);
	let deleting = $state<{ id: string; title: string } | null>(null);
	let creatingFolder = $state(false);
	/** A chat to move into the folder being made ("New folder…" in its menu). */
	let movingToNew: string | null = null;
	let renamingFolder = $state<FolderItem | null>(null);
	let deletingFolder = $state<FolderItem | null>(null);
	let moveProblem = $state<string | null>(null);

	/** Folders whose chats are listed under them. */
	const expanded = new SvelteSet<string>();
	/** The sidebar's scrolling list, which a dragged chat stays inside. */
	let scroller = $state<HTMLElement | null>(null);
	/** A chat just dropped into another folder, shown there before the reload says so. */
	let landed = $state<{ id: string; folderId: string | null } | null>(null);
	let springTimer: ReturnType<typeof setTimeout> | undefined;

	const drag: ChatDrag<ChatItem> = new ChatDrag<ChatItem>({
		container: () => scroller,
		settleOn: (): { el: Element; vanish: boolean } | null => {
			const gap: Element | null | undefined =
				landing === null ? null : scroller?.querySelector(`[data-chat-gap="${landing}"]`);
			if (gap) return { el: gap, vanish: false };
			// A closed folder has no gap: the chat slides into its row.
			const row = landing && scroller?.querySelector(`[data-folder-row="${landing}"]`);
			return row ? { el: row, vanish: true } : null;
		},
		drop: (chat, target) => {
			clearTimeout(springTimer);
			if (target === null || target === (chat.folderId ?? '')) return;
			const folderId = target || null;
			landed = { id: chat.id, folderId };
			moveProblem = null;
			moveChat(chat.id, folderId)
				.catch((err) => (moveProblem = err instanceof Error ? err.message : String(err)))
				.finally(() => {
					if (landed?.id === chat.id) landed = null;
				});
		},
		hover: (target) => {
			clearTimeout(springTimer);
			// Holding a chat over a closed folder opens it, to show where the chat will land.
			if (!target || expanded.has(target)) return;
			springTimer = setTimeout(() => {
				if (drag.row?.target === target) expanded.add(target);
			}, 600);
		}
	});

	const shown = $derived.by(() => {
		const moved = landed;
		if (!moved) return conversations;
		return conversations.map((c) => (c.id === moved.id ? { ...c, folderId: moved.folderId } : c));
	});
	/** Each chat's place in the sidebar's order (most recent activity first). */
	const position = $derived(Object.fromEntries(shown.map((c, i) => [c.id, i])));
	/** The group a dragged chat would land in: the one under it, or back where it came from. */
	const landing: string | null = $derived(
		drag.row ? (drag.row.target ?? drag.row.item.folderId ?? '') : null
	);
	/** That group when it's another one than the chat's own, to light up. */
	const dropInto = $derived(
		drag.row && landing !== (drag.row.item.folderId ?? '') ? landing : null
	);

	type Row = { key: string; chat: ChatItem | null };

	/**
	 * A group's rows: '' for the chat list, or a folder's id. A dragged chat leaves its row, and the
	 * group it would land in opens a gap (`chat: null`) where it will be. Groups are in order of
	 * recent activity, so that's the chat's place in that order, not wherever the pointer is.
	 */
	function rowsOf(group: string): Row[] {
		const dragged = drag.row?.item;
		const chats = shown.filter((c) => (c.folderId ?? '') === group && c.id !== dragged?.id);
		const rows: Row[] = chats.map((chat) => ({ key: chat.id, chat }));
		if (dragged && landing === group) {
			const at = chats.filter((c) => position[c.id] < position[dragged.id]).length;
			rows.splice(at, 0, { key: 'gap', chat: null });
		}
		return rows;
	}
	/** Chats btw is working in right now, in this profile. */
	let running = $state<string[]>([]);

	// EventSource reconnects by itself; each (re)connect starts with the whole list.
	$effect(() => {
		const source = new EventSource(`/api/p/${profile.slug}/running`);
		source.onmessage = (event) =>
			(running = (JSON.parse(event.data) as { running: string[] }).running);
		return () => source.close();
	});

	/** The folder of the open page or chat. */
	const activeFolderId = $derived(
		page.params.folder ?? conversations.find((c) => c.id === page.params.id)?.folderId ?? null
	);

	// Opening a folder or one of its chats shows its chats in the sidebar.
	$effect(() => {
		const id = activeFolderId;
		if (id) untrack(() => expanded.add(id));
	});

	// On phones the sidebar is a drawer; close it once a link has been followed.
	afterNavigate(() => sidebar.setOpenMobile(false));

	function toggleFolder(id: string) {
		if (expanded.has(id)) expanded.delete(id);
		else expanded.add(id);
	}

	async function move(chatId: string, folderId: string | null) {
		moveProblem = null;
		try {
			await moveChat(chatId, folderId);
			if (folderId) expanded.add(folderId);
		} catch (err) {
			moveProblem = err instanceof Error ? err.message : String(err);
		}
	}

	function newFolder(forChat: string | null = null) {
		movingToNew = forChat;
		creatingFolder = true;
	}

	function openSearch() {
		sidebar.setOpenMobile(false);
		searchOpen = true;
	}

	function onWindowKeydown(event: KeyboardEvent) {
		if (event.key === 'k' && (event.metaKey || event.ctrlKey)) {
			event.preventDefault();
			searchOpen = !searchOpen;
		}
	}

	const newChatHref = $derived(resolve('/p/[slug]', { slug: profile.slug }));
	const chatHref = (id: string) => resolve('/p/[slug]/c/[id]', { slug: profile.slug, id });
	const folderHref = (folder: string) =>
		resolve('/p/[slug]/f/[folder]', { slug: profile.slug, folder });
</script>

{#snippet chatItem(conversation: ChatItem)}
	<Sidebar.MenuItem>
		<!-- Pressed and moved (or held, on touch screens), the row is dragged; see ChatDrag. -->
		<Sidebar.MenuButton
			isActive={page.params.id === conversation.id}
			class="select-none [-webkit-touch-callout:none]"
		>
			{#snippet child({ props })}
				{@const working = running.includes(conversation.id)}
				<a
					href={chatHref(conversation.id)}
					{...props}
					draggable="false"
					onpointerdown={(event) => drag.press(event, conversation)}
					ontouchstart={(event) => drag.touch(event, conversation)}
				>
					<!-- Shimmers like the "Thinking" label while btw works in the chat. -->
					<span class={cn(working && 'thinking-shimmer')}>
						<TypedText text={conversation.title} />
						{#if working}<span class="sr-only">, working</span>{/if}
					</span>
				</a>
			{/snippet}
		</Sidebar.MenuButton>
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<Sidebar.MenuAction
						showOnHover
						{...props}
						class="top-1/2! size-7 -translate-y-1/2 rounded-lg max-md:hidden"
					>
						<EllipsisIcon />
						<span class="sr-only">More</span>
					</Sidebar.MenuAction>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content side="right" align="start" class="w-48">
				<DropdownMenu.Item onSelect={() => (renaming = conversation)}>
					<PencilIcon />
					Rename
				</DropdownMenu.Item>
				<MoveToFolderMenu
					{folders}
					folderId={conversation.folderId}
					onmove={(folderId) => move(conversation.id, folderId)}
					onnew={() => newFolder(conversation.id)}
				/>
				<DropdownMenu.Separator />
				<DropdownMenu.Item variant="destructive" onSelect={() => (deleting = conversation)}>
					<Trash2Icon />
					Delete
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</Sidebar.MenuItem>
{/snippet}

<!-- Where a dragged chat will land. It slides open and shut while the chat moves between groups. -->
{#snippet gap(group: string)}
	<li
		data-chat-gap={group}
		aria-hidden="true"
		class="h-9 rounded-xl border border-dashed border-sidebar-border"
		transition:slide|global={{ duration: drag.row?.animateGap ? SETTLE_MS : 0 }}
	></li>
{/snippet}

<svelte:window onkeydown={onWindowKeydown} />

<Sidebar.Root collapsible="icon" class="border-none">
	<Sidebar.Header class="gap-1 px-2 pt-2">
		<div class="flex items-center gap-1">
			<DropdownMenu.Root>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							class="flex h-10 min-w-0 items-center gap-1 rounded-xl px-2.5 text-lg font-semibold group-data-[collapsible=icon]:hidden hover:bg-sidebar-accent"
						>
							<AssistantAvatar avatar={profile.avatar} size={22} class="mr-1" />
							<span class="truncate">{profile.name}</span>
							<ChevronDownIcon class="size-4 shrink-0 text-muted-foreground" />
						</button>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content class="w-64">
					<DropdownMenu.Label class="text-xs font-normal text-muted-foreground"
						>Profiles</DropdownMenu.Label
					>
					{#each profiles as p (p.slug)}
						<DropdownMenu.Item onSelect={() => goto(resolve('/p/[slug]', { slug: p.slug }))}>
							<AssistantAvatar avatar={p.avatar} size={16} />
							<span class="min-w-0 flex-1 truncate">{p.name}</span>
							{#if p.slug === profile.slug}<CheckIcon class="ml-auto" />{/if}
						</DropdownMenu.Item>
					{/each}
					<DropdownMenu.Separator />
					<DropdownMenu.Item onSelect={() => goto(resolve('/profiles'))}>
						<PlusIcon />
						New profile
					</DropdownMenu.Item>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
			<div class="flex-1"></div>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							onclick={sidebar.toggle}
							class="flex size-9 shrink-0 items-center justify-center rounded-xl text-muted-foreground group-data-[collapsible=icon]:size-8 hover:bg-sidebar-accent hover:text-foreground max-md:hidden"
							aria-label={sidebar.open ? 'Close sidebar' : 'Open sidebar'}
						>
							<PanelLeftIcon class="size-5" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content side="right">
					{sidebar.open ? 'Close sidebar' : 'Open sidebar'}
					<Kbd>Ctrl B</Kbd>
				</Tooltip.Content>
			</Tooltip.Root>
		</div>
	</Sidebar.Header>

	<Sidebar.Content bind:ref={scroller} {@attach drag.holdScroll} class="no-scrollbar">
		<Sidebar.Group class="px-2 py-1">
			<Sidebar.Menu>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton tooltipContent="New chat" isActive={page.route.id === '/p/[slug]'}>
						{#snippet child({ props })}
							<a href={newChatHref} {...props}>
								<SquarePenIcon />
								<span>New chat</span>
							</a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton tooltipContent="Search chats" onclick={openSearch}>
						<SearchIcon />
						<span>Search chats</span>
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton
						tooltipContent="Images"
						isActive={page.route.id === '/p/[slug]/images'}
					>
						{#snippet child({ props })}
							<a href={resolve('/p/[slug]/images', { slug: profile.slug })} {...props}>
								<ImagesIcon />
								<span>Images</span>
							</a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton
						tooltipContent="Automations"
						isActive={page.route.id === '/p/[slug]/automations'}
					>
						{#snippet child({ props })}
							<a href={resolve('/p/[slug]/automations', { slug: profile.slug })} {...props}>
								<ClockIcon />
								<span>Automations</span>
							</a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton
						tooltipContent="Memory"
						isActive={page.route.id === '/p/[slug]/memory'}
					>
						{#snippet child({ props })}
							<a href={resolve('/p/[slug]/memory', { slug: profile.slug })} {...props}>
								<BrainIcon />
								<span>Memory</span>
							</a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton
						tooltipContent="Skills"
						isActive={page.route.id === '/p/[slug]/skills'}
					>
						{#snippet child({ props })}
							<a href={resolve('/p/[slug]/skills', { slug: profile.slug })} {...props}>
								<PuzzleIcon />
								<span>Skills</span>
							</a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton
						tooltipContent="People & profile"
						isActive={page.route.id === '/p/[slug]/settings'}
					>
						{#snippet child({ props })}
							<a href={resolve('/p/[slug]/settings', { slug: profile.slug })} {...props}>
								<UsersIcon />
								<span>People & profile</span>
							</a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
			</Sidebar.Menu>
		</Sidebar.Group>

		<Sidebar.Group class="px-2 group-data-[collapsible=icon]:hidden">
			<Sidebar.GroupLabel class="text-sm font-medium text-muted-foreground"
				>Folders</Sidebar.GroupLabel
			>
			<Sidebar.Menu>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton onclick={() => newFolder()}>
						<FolderPlusIcon />
						<span>New folder</span>
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
				{#each folders as folder (folder.id)}
					{@const open = expanded.has(folder.id)}
					<Sidebar.MenuItem data-drop-target={folder.id} data-folder-row={folder.id}>
						<Sidebar.MenuButton
							isActive={page.params.folder === folder.id}
							class={cn(
								'pl-9',
								dropInto === folder.id && 'bg-sidebar-accent ring-2 ring-sidebar-ring'
							)}
						>
							{#snippet child({ props })}
								<a href={folderHref(folder.id)} {...props}>
									<span>{folder.name}</span>
								</a>
							{/snippet}
						</Sidebar.MenuButton>
						<!-- The folder's icon; pointing at the row turns it into the show/hide chevron. -->
						<button
							type="button"
							onclick={() => toggleFolder(folder.id)}
							class="absolute top-1.5 left-1.5 flex size-6 items-center justify-center rounded-lg text-sidebar-foreground hover:bg-sidebar-border/60 [&>svg]:size-4"
							aria-expanded={open}
							aria-label={open
								? `Hide the chats in ${folder.name}`
								: `Show the chats in ${folder.name}`}
						>
							{#if open}
								<FolderOpenIcon class="md:group-hover/menu-item:hidden" />
							{:else}
								<FolderIcon class="md:group-hover/menu-item:hidden" />
							{/if}
							<ChevronRightIcon
								class={cn(
									'hidden transition-transform md:group-hover/menu-item:block',
									open && 'rotate-90'
								)}
							/>
						</button>
						<DropdownMenu.Root>
							<DropdownMenu.Trigger>
								{#snippet child({ props })}
									<Sidebar.MenuAction
										showOnHover
										{...props}
										class="top-1! size-7 rounded-lg max-md:hidden"
									>
										<EllipsisIcon />
										<span class="sr-only">More</span>
									</Sidebar.MenuAction>
								{/snippet}
							</DropdownMenu.Trigger>
							<DropdownMenu.Content side="right" align="start" class="w-44">
								<DropdownMenu.Item onSelect={() => (renamingFolder = folder)}>
									<PencilIcon />
									Rename
								</DropdownMenu.Item>
								<DropdownMenu.Item variant="destructive" onSelect={() => (deletingFolder = folder)}>
									<Trash2Icon />
									Delete
								</DropdownMenu.Item>
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					</Sidebar.MenuItem>
					{#if open}
						<!--
							A sibling of the folder's item, so pointing at a chat doesn't highlight the folder.
							Chats dropped on the list go into the folder too.
						-->
						<li
							data-drop-target={folder.id}
							class={cn('rounded-xl', dropInto === folder.id && 'bg-sidebar-accent/60')}
							transition:slide={{ duration: SETTLE_MS }}
						>
							<Sidebar.MenuSub class="mr-0 pr-0">
								{#each rowsOf(folder.id) as row (row.key)}
									{#if row.chat}
										{@render chatItem(row.chat)}
									{:else}
										{@render gap(folder.id)}
									{/if}
								{:else}
									<li class="px-3 py-1.5 text-xs text-muted-foreground">
										Drag chats here, or start one on the folder's page.
									</li>
								{/each}
							</Sidebar.MenuSub>
						</li>
					{/if}
				{/each}
			</Sidebar.Menu>
			{#if moveProblem}
				<p class="px-3 py-1 text-xs text-destructive">{moveProblem}</p>
			{/if}
		</Sidebar.Group>

		<Sidebar.Group
			class={cn(
				'rounded-xl px-2 group-data-[collapsible=icon]:hidden',
				dropInto === '' && 'bg-sidebar-accent/60 ring-2 ring-sidebar-ring ring-inset'
			)}
			data-drop-target=""
		>
			<Sidebar.GroupLabel class="text-sm font-medium text-muted-foreground"
				>Chats</Sidebar.GroupLabel
			>
			<Sidebar.Menu>
				{#each rowsOf('') as row (row.key)}
					{#if row.chat}
						{@render chatItem(row.chat)}
					{:else}
						{@render gap('')}
					{/if}
				{:else}
					<p class="px-3 py-2 text-sm text-muted-foreground">
						{drag.row?.item.folderId
							? 'Drop here to take the chat out of its folder.'
							: 'Your chats will show up here.'}
					</p>
				{/each}
			</Sidebar.Menu>
		</Sidebar.Group>
	</Sidebar.Content>

	<Sidebar.Footer class="p-2">
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<UserMenu {user} />
			</Sidebar.MenuItem>
		</Sidebar.Menu>
	</Sidebar.Footer>
</Sidebar.Root>

<!-- Pinned near the top and kept inside the screen: a long list scrolls, the input never moves. -->
<Command.Dialog
	bind:open={searchOpen}
	title="Search chats"
	description="Find a chat by its title"
	class="top-[12dvh] flex max-h-[min(36rem,76dvh)] flex-col"
>
	<Command.Input placeholder="Search chats…" />
	<Command.List class="max-h-none">
		<Command.Empty>No chats found.</Command.Empty>
		<Command.Group>
			<Command.Item
				value="new chat"
				onSelect={() => {
					searchOpen = false;
					goto(newChatHref);
				}}
			>
				<SquarePenIcon />
				New chat
			</Command.Item>
		</Command.Group>
		{#if folders.length}
			<Command.Group heading="Folders">
				{#each folders as folder (folder.id)}
					<Command.Item
						value={`${folder.name} ${folder.id}`}
						onSelect={() => {
							searchOpen = false;
							goto(folderHref(folder.id));
						}}
					>
						<FolderIcon />
						<span class="truncate">{folder.name}</span>
					</Command.Item>
				{/each}
			</Command.Group>
		{/if}
		{#if conversations.length}
			<Command.Group heading="Chats">
				{#each conversations as conversation (conversation.id)}
					{@const folder = folders.find((f) => f.id === conversation.folderId)}
					<Command.Item
						value={`${conversation.title} ${folder?.name ?? ''} ${conversation.id}`}
						onSelect={() => {
							searchOpen = false;
							goto(chatHref(conversation.id));
						}}
					>
						<MessageCircleIcon />
						<span class="min-w-0 flex-1 truncate">{conversation.title}</span>
						{#if folder}
							<span class="max-w-32 shrink-0 truncate text-xs text-muted-foreground"
								>{folder.name}</span
							>
						{/if}
					</Command.Item>
				{/each}
			</Command.Group>
		{/if}
	</Command.List>
</Command.Dialog>

<AlertDialog.Root open={deleting !== null} onOpenChange={(open) => !open && (deleting = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete chat?</AlertDialog.Title>
			<AlertDialog.Description>
				This deletes <strong class="text-foreground">{deleting?.title}</strong> for everyone in
				{profile.name}.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action={deleting ? `${chatHref(deleting.id)}?/delete` : ''}
			use:enhance={() => {
				const id = deleting?.id;
				return async ({ result }) => {
					deleting = null;
					if (result.type !== 'redirect') return;
					// Stay where you are unless the open chat was the one deleted.
					if (page.params.id === id) await goto(newChatHref, { invalidateAll: true });
					else await invalidate('btw:conversations');
				};
			}}
		>
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">Cancel</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive">Delete</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>

<RenameChatDialog bind:chat={renaming} slug={profile.slug} />

<NewFolderDialog
	bind:open={creatingFolder}
	slug={profile.slug}
	oncreated={(folder) => {
		const chat = movingToNew;
		movingToNew = null;
		if (chat) move(chat, folder.id);
		else goto(folderHref(folder.id));
	}}
/>
<RenameFolderDialog bind:folder={renamingFolder} slug={profile.slug} />
<DeleteFolderDialog bind:folder={deletingFolder} slug={profile.slug} />

{#if drag.row}
	{@const row = drag.row}
	<!-- The dragged chat. It lets the pointer through, so ChatDrag can see what's under it. -->
	<Portal>
		<div
			aria-hidden="true"
			in:scale={{ start: 0.98, opacity: 1, duration: 120 }}
			class="pointer-events-none fixed top-0 z-100 flex items-center rounded-xl bg-sidebar-accent px-3 text-sm font-medium text-sidebar-accent-foreground shadow-lg ring-1 ring-sidebar-border"
			style:left="{row.left}px"
			style:width="{row.width}px"
			style:height="{row.height}px"
			style:translate="0 {row.settling?.top ?? row.top}px"
			style:scale={row.settling ? (row.settling.vanish ? 0.85 : 1) : 1.02}
			style:opacity={row.settling?.vanish ? 0 : 1}
			style:transition={row.settling
				? `translate ${SETTLE_MS}ms ease-out, scale ${SETTLE_MS}ms ease-out, opacity ${SETTLE_MS}ms ease-out`
				: 'none'}
		>
			<span class={cn('truncate', running.includes(row.item.id) && 'thinking-shimmer')}>
				{row.item.title}
			</span>
		</div>
	</Portal>
{/if}
