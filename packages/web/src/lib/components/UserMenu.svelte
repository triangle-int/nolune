<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import CircleArrowUpIcon from '@lucide/svelte/icons/circle-arrow-up';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import LogOutIcon from '@lucide/svelte/icons/log-out';
	import ArrowLeftRightIcon from '@lucide/svelte/icons/arrow-left-right';
	import BoxIcon from '@lucide/svelte/icons/box';
	import ContactIcon from '@lucide/svelte/icons/contact';
	import IdCardIcon from '@lucide/svelte/icons/id-card';
	import PlugIcon from '@lucide/svelte/icons/plug';
	import UsersIcon from '@lucide/svelte/icons/users';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Sidebar from '$lib/components/ui/sidebar';
	import type { Update } from '@nolune/core';
	import { getI18n } from '$lib/i18n';
	import { connectElsewhere, inIosApp } from '$lib/ios';
	import { cn } from '$lib/utils';
	import SettingsDialog from './SettingsDialog.svelte';
	import UpdateDialog from './UpdateDialog.svelte';
	import UserAvatar from './UserAvatar.svelte';

	interface Props {
		user: { name: string; email: string; isAdmin: boolean; picture: string | null };
		/** `sidebar` shows the name next to the avatar; `compact` is just the avatar. */
		variant?: 'sidebar' | 'compact';
	}

	let { user, variant = 'sidebar' }: Props = $props();

	const { m } = getI18n();
	let settingsOpen = $state(false);
	let updateOpen = $state(false);
	let logoutForm = $state<HTMLFormElement>();
	/** In nolune for iOS: it can connect to another family's nolune. */
	let inApp = $state(false);

	/** A newer nolune, which the root layout only gives admins. */
	const update = $derived<Update | null>(page.data.update ?? null);

	/**
	 * The release this browser last opened the update dialog for: the avatar has a dot until it's
	 * this one. Unknown (undefined) until the page has loaded, so a dot never flashes and goes.
	 */
	const SEEN_KEY = 'nolune-update-seen';
	let seen = $state<string | null>();
	const unseen = $derived(!!update && seen !== undefined && seen !== update.version);

	onMount(() => {
		inApp = inIosApp();
		try {
			seen = localStorage.getItem(SEEN_KEY);
		} catch {
			// Storage is blocked: the dot shows until the dialog is opened.
			seen = null;
		}
	});

	function openUpdate() {
		updateOpen = true;
		if (!update) return;
		seen = update.version;
		try {
			localStorage.setItem(SEEN_KEY, update.version);
		} catch {
			// Storage is blocked; it's seen for as long as the page is open.
		}
	}
</script>

{#snippet avatar(className: string, ring: string)}
	<span class="relative shrink-0">
		<UserAvatar name={user.name} picture={user.picture} class={className} />
		{#if unseen}
			<span class={cn('absolute -top-0.5 -right-0.5 size-2.5 rounded-full bg-sky-500 ring-2', ring)}
			></span>
		{/if}
	</span>
{/snippet}

<DropdownMenu.Root>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			{#if variant === 'sidebar'}
				<Sidebar.MenuButton {...props} size="lg" class="h-12 gap-2.5 px-2">
					{@render avatar('size-8', 'ring-sidebar')}
					<span class="min-w-0 flex-1 truncate text-left font-medium">{user.name}</span>
					{#if unseen}<span class="sr-only">{m.update.available(update?.version ?? '')}</span>{/if}
				</Sidebar.MenuButton>
			{:else}
				<button
					{...props}
					class="flex size-9 items-center justify-center rounded-full hover:bg-muted"
					aria-label={unseen
						? `${m.userMenu.account}: ${m.update.available(update?.version ?? '')}`
						: m.userMenu.account}
				>
					{@render avatar('', 'ring-background')}
				</button>
			{/if}
		{/snippet}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content
		class="w-64"
		side={variant === 'sidebar' ? 'top' : 'bottom'}
		align={variant === 'sidebar' ? 'start' : 'end'}
	>
		<DropdownMenu.Label class="flex items-center gap-2.5 px-3 py-2 font-normal">
			<UserAvatar name={user.name} picture={user.picture} class="size-8" />
			<span class="min-w-0">
				<span class="block truncate text-sm font-medium text-foreground">{user.name}</span>
				<span class="block truncate text-xs text-muted-foreground">{user.email}</span>
			</span>
		</DropdownMenu.Label>
		<DropdownMenu.Separator />
		{#if update}
			<DropdownMenu.Item onSelect={openUpdate}>
				<CircleArrowUpIcon class="text-sky-600 dark:text-sky-400" />
				{m.update.available(update.version)}
			</DropdownMenu.Item>
			<DropdownMenu.Separator />
		{/if}
		<DropdownMenu.Item onSelect={() => (settingsOpen = true)}>
			<SettingsIcon />
			{m.userMenu.settings}
		</DropdownMenu.Item>
		<DropdownMenu.Item>
			{#snippet child({ props })}
				<a {...props} href={resolve('/profiles')}><UsersIcon />{m.userMenu.allProfiles}</a>
			{/snippet}
		</DropdownMenu.Item>
		<DropdownMenu.Item>
			{#snippet child({ props })}
				<a {...props} href={resolve('/card')}><IdCardIcon />{m.userMenu.card}</a>
			{/snippet}
		</DropdownMenu.Item>
		{#if user.isAdmin}
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<a {...props} href={resolve('/admin')}><BoxIcon />{m.userMenu.modelsAndKeys}</a>
				{/snippet}
			</DropdownMenu.Item>
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<a {...props} href={resolve('/admin/services')}><PlugIcon />{m.userMenu.services}</a>
				{/snippet}
			</DropdownMenu.Item>
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<a {...props} href={resolve('/admin/people')}><ContactIcon />{m.userMenu.people}</a>
				{/snippet}
			</DropdownMenu.Item>
		{/if}
		<DropdownMenu.Separator />
		{#if inApp}
			<DropdownMenu.Item onSelect={connectElsewhere}>
				<ArrowLeftRightIcon />
				{m.app.connectElsewhere}
			</DropdownMenu.Item>
		{/if}
		<DropdownMenu.Item onSelect={() => logoutForm?.requestSubmit()}>
			<LogOutIcon />
			{m.userMenu.logOut}
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>

<form bind:this={logoutForm} method="POST" action="/logout" class="hidden"></form>

<SettingsDialog bind:open={settingsOpen} {user} />
{#if update}
	<UpdateDialog bind:open={updateOpen} {update} />
{/if}
