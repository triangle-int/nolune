<script lang="ts">
	import { enhance } from '$app/forms';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import SquarePenIcon from '@lucide/svelte/icons/square-pen';
	import SearchIcon from '@lucide/svelte/icons/search';
	import ClockIcon from '@lucide/svelte/icons/clock';
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
	import * as Sidebar from '$lib/components/ui/sidebar';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Command from '$lib/components/ui/command';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { Kbd } from '$lib/components/ui/kbd';
	import TypedText from './TypedText.svelte';
	import UserMenu from './UserMenu.svelte';

	interface Props {
		profile: { slug: string; name: string };
		profiles: { slug: string; name: string }[];
		conversations: { id: string; title: string }[];
		user: { name: string; email: string; isAdmin: boolean };
	}

	let { profile, profiles, conversations, user }: Props = $props();

	const sidebar = Sidebar.useSidebar();

	let searchOpen = $state(false);
	let deleting = $state<{ id: string; title: string } | null>(null);

	// On phones the sidebar is a drawer; close it once a link has been followed.
	afterNavigate(() => sidebar.setOpenMobile(false));

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
</script>

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

	<Sidebar.Content class="no-scrollbar">
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
				>Chats</Sidebar.GroupLabel
			>
			<Sidebar.Menu>
				{#each conversations as conversation (conversation.id)}
					<Sidebar.MenuItem>
						<Sidebar.MenuButton isActive={page.params.id === conversation.id}>
							{#snippet child({ props })}
								<a href={chatHref(conversation.id)} {...props}>
									<span><TypedText text={conversation.title} /></span>
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
							<DropdownMenu.Content side="right" align="start" class="w-44">
								<DropdownMenu.Item variant="destructive" onSelect={() => (deleting = conversation)}>
									<Trash2Icon />
									Delete
								</DropdownMenu.Item>
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					</Sidebar.MenuItem>
				{:else}
					<p class="px-3 py-2 text-sm text-muted-foreground">Your chats will show up here.</p>
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

<Command.Dialog bind:open={searchOpen} title="Search chats" description="Find a chat by its title">
	<Command.Input placeholder="Search chats…" />
	<Command.List class="max-h-[60vh]">
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
		{#if conversations.length}
			<Command.Group heading="Chats">
				{#each conversations as conversation (conversation.id)}
					<Command.Item
						value={`${conversation.title} ${conversation.id}`}
						onSelect={() => {
							searchOpen = false;
							goto(resolve('/p/[slug]/c/[id]', { slug: profile.slug, id: conversation.id }));
						}}
					>
						<MessageCircleIcon />
						<span class="truncate">{conversation.title}</span>
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
