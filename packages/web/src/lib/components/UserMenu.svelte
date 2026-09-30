<script lang="ts">
	import { resolve } from '$app/paths';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import LogOutIcon from '@lucide/svelte/icons/log-out';
	import BoxIcon from '@lucide/svelte/icons/box';
	import UsersIcon from '@lucide/svelte/icons/users';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Sidebar from '$lib/components/ui/sidebar';
	import { getI18n } from '$lib/i18n';
	import SettingsDialog from './SettingsDialog.svelte';
	import UserAvatar from './UserAvatar.svelte';

	interface Props {
		user: { name: string; email: string; isAdmin: boolean; picture: string | null };
		/** `sidebar` shows the name next to the avatar; `compact` is just the avatar. */
		variant?: 'sidebar' | 'compact';
	}

	let { user, variant = 'sidebar' }: Props = $props();

	const { m } = getI18n();
	let settingsOpen = $state(false);
	let logoutForm = $state<HTMLFormElement>();
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			{#if variant === 'sidebar'}
				<Sidebar.MenuButton {...props} size="lg" class="h-12 gap-2.5 px-2">
					<UserAvatar name={user.name} picture={user.picture} class="size-8" />
					<span class="min-w-0 flex-1 truncate text-left font-medium">{user.name}</span>
				</Sidebar.MenuButton>
			{:else}
				<button
					{...props}
					class="flex size-9 items-center justify-center rounded-full hover:bg-muted"
					aria-label={m.userMenu.account}
				>
					<UserAvatar name={user.name} picture={user.picture} />
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
		<DropdownMenu.Item onSelect={() => (settingsOpen = true)}>
			<SettingsIcon />
			{m.userMenu.settings}
		</DropdownMenu.Item>
		<DropdownMenu.Item>
			{#snippet child({ props })}
				<a {...props} href={resolve('/profiles')}><UsersIcon />{m.userMenu.allProfiles}</a>
			{/snippet}
		</DropdownMenu.Item>
		{#if user.isAdmin}
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<a {...props} href={resolve('/admin')}><BoxIcon />{m.userMenu.modelsAndKeys}</a>
				{/snippet}
			</DropdownMenu.Item>
		{/if}
		<DropdownMenu.Separator />
		<DropdownMenu.Item onSelect={() => logoutForm?.requestSubmit()}>
			<LogOutIcon />
			{m.userMenu.logOut}
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>

<form bind:this={logoutForm} method="POST" action="/logout" class="hidden"></form>

<SettingsDialog bind:open={settingsOpen} {user} />
