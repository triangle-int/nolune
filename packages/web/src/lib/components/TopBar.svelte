<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import Notifications from './Notifications.svelte';
	import UserMenu from './UserMenu.svelte';

	/** Header for the pages outside a profile (profile list, models). */
	const user = $derived(page.data.user);
	const notifications = $derived(page.data.notifications);
	/** Inside nolune for iOS's screens, under the app's own navigation. */
	const embedded = $derived(page.data.embedded === true);
</script>

{#if !embedded}
	<header class="flex h-14 shrink-0 items-center gap-1.5 px-3 sm:px-4">
		<a
			href={resolve('/profiles')}
			class="rounded-lg px-2 py-1 text-xl font-semibold tracking-tight"
		>
			nolune
		</a>
		<div class="flex-1"></div>
		{#if notifications}
			<Notifications items={notifications.items} seenAt={notifications.seenAt} />
		{/if}
		{#if user}
			<UserMenu {user} variant="compact" />
		{/if}
	</header>
{/if}
