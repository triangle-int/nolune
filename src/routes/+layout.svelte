<script lang="ts">
	import './layout.css';
	import { untrack } from 'svelte';
	import { invalidate } from '$app/navigation';
	import { page } from '$app/state';
	import { ModeWatcher } from 'mode-watcher';
	import { isAvatar } from '@btw/core/avatars';
	import favicon from '$lib/assets/favicon.svg';
	import { avatarFavicon } from '$lib/avatars';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { Preferences, setPreferences } from '$lib/preferences.svelte';

	let { data, children } = $props();

	// The cookie is only read once; after that this object is the source of truth.
	setPreferences(new Preferences(untrack(() => data.prefs)));

	// What others change while the page is open: notifications, and profile names and avatars.
	const signedIn = $derived(!!data.user);
	$effect(() => {
		if (!signedIn) return;
		const source = new EventSource('/api/events');
		source.onmessage = (event) => {
			const { type } = JSON.parse(event.data) as { type: 'notifications' | 'profiles' };
			invalidate(`btw:${type}`);
		};
		return () => source.close();
	});

	/** A profile's pages show its assistant's avatar in the tab. */
	const icon = $derived.by(() => {
		const avatar = (page.data as { profile?: { avatar?: string } }).profile?.avatar;
		return isAvatar(avatar) ? avatarFavicon(avatar) : favicon;
	});
</script>

<svelte:head>
	<link rel="icon" href="/favicon.ico" sizes="32x32" />
	<link rel="icon" href={icon} type="image/svg+xml" />
	<link rel="apple-touch-icon" href="/apple-touch-icon.png" />
	<title>btw</title>
</svelte:head>

<ModeWatcher themeColors={{ light: '#ffffff', dark: '#212121' }} />

<Tooltip.Provider delayDuration={300}>
	<div class="h-dvh">
		{@render children()}
	</div>
</Tooltip.Provider>
