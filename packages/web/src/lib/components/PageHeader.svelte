<script lang="ts">
	import type { Snippet } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import MenuIcon from '@lucide/svelte/icons/menu';
	import SquarePenIcon from '@lucide/svelte/icons/square-pen';
	import * as Sidebar from '$lib/components/ui/sidebar';
	import { getI18n } from '$lib/i18n';
	import Notifications from './Notifications.svelte';

	interface Props {
		/** The title area, left of the actions. */
		children?: Snippet;
		/** Extra buttons before the bell. */
		actions?: Snippet;
	}

	let { children, actions }: Props = $props();

	const sidebar = Sidebar.useSidebar();
	const { m } = getI18n();
	const slug = $derived(page.params.slug);
	const notifications = $derived(page.data.notifications);
	/**
	 * Inside nolune for iOS's screens, the app's navigation bar has the title, the bell and the
	 * menu: only the page's own actions stay.
	 */
	const embedded = $derived(page.data.embedded === true);
</script>

{#if embedded}
	{#if actions}
		<header class="flex h-14 shrink-0 items-center justify-end gap-1.5 px-2 sm:px-3">
			{@render actions()}
		</header>
	{/if}
{:else}
	<header class="flex h-14 shrink-0 items-center gap-1.5 px-2 sm:px-3">
		<!-- Phones: round buttons like the ChatGPT app. Desktop has the sidebar (or its icon rail). -->
		<button
			onclick={sidebar.toggle}
			class="flex size-10 shrink-0 items-center justify-center rounded-full border bg-background shadow-xs md:hidden"
			aria-label={m.header.openMenu}
		>
			<MenuIcon class="size-5" />
		</button>

		<div class="flex min-w-0 flex-1 items-center gap-2 px-1">
			{@render children?.()}
		</div>

		{@render actions?.()}
		{#if notifications}
			<Notifications items={notifications.items} seenAt={notifications.seenAt} />
		{/if}
		{#if slug}
			<a
				href={resolve('/p/[slug]', { slug })}
				class="flex size-10 shrink-0 items-center justify-center rounded-full border bg-background shadow-xs md:hidden"
				aria-label={m.common.newChat}
			>
				<SquarePenIcon class="size-5" />
			</a>
		{/if}
	</header>
{/if}
