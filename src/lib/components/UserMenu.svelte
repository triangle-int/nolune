<script lang="ts">
	import { resolve } from '$app/paths';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import LogOutIcon from '@lucide/svelte/icons/log-out';
	import BoxIcon from '@lucide/svelte/icons/box';
	import UsersIcon from '@lucide/svelte/icons/users';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Sidebar from '$lib/components/ui/sidebar';
	import SettingsDialog from './SettingsDialog.svelte';
	import UserAvatar from './UserAvatar.svelte';

	interface Props {
		user: { name: string; email: string; isAdmin: boolean };
		/** `sidebar` shows the name next to the avatar; `compact` is just the avatar. */
		variant?: 'sidebar' | 'compact';
	}

	let { user, variant = 'sidebar' }: Props = $props();

	let settingsOpen = $state(false);
	let logoutForm = $state<HTMLFormElement>();
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			{#if variant === 'sidebar'}
				<Sidebar.MenuButton {...props} size="lg" class="h-12 gap-2.5 px-2">
					<UserAvatar name={user.name} class="size-8" />
					<span class="min-w-0 flex-1 truncate text-left font-medium">{user.name}</span>
				</Sidebar.MenuButton>
			{:else}
				<button
					{...props}
					class="flex size-9 items-center justify-center rounded-full hover:bg-muted"
					aria-label="Account"
				>
					<UserAvatar name={user.name} />
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
			<UserAvatar name={user.name} class="size-8" />
			<span class="min-w-0">
				<span class="block truncate text-sm font-medium text-foreground">{user.name}</span>
				<span class="block truncate text-xs text-muted-foreground">{user.email}</span>
			</span>
		</DropdownMenu.Label>
		<DropdownMenu.Separator />
		<DropdownMenu.Item onSelect={() => (settingsOpen = true)}>
			<SettingsIcon />
			Settings
		</DropdownMenu.Item>
		<DropdownMenu.Item>
			{#snippet child({ props })}
				<a {...props} href={resolve('/profiles')}><UsersIcon />All profiles</a>
			{/snippet}
		</DropdownMenu.Item>
		{#if user.isAdmin}
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<a {...props} href={resolve('/admin')}><BoxIcon />Models & keys</a>
				{/snippet}
			</DropdownMenu.Item>
		{/if}
		<DropdownMenu.Separator />
		<DropdownMenu.Item onSelect={() => logoutForm?.requestSubmit()}>
			<LogOutIcon />
			Log out
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>

<form bind:this={logoutForm} method="POST" action="/logout" class="hidden"></form>

<SettingsDialog bind:open={settingsOpen} {user} />
