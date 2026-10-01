<script lang="ts">
	import './layout.css';
	import { untrack } from 'svelte';
	import { invalidate } from '$app/navigation';
	import { page } from '$app/state';
	import { ModeWatcher, mode } from 'mode-watcher';
	import { isAvatar } from '@nolune/core/avatars';
	import favicon from '$lib/assets/favicon.svg';
	import { avatarFavicon } from '$lib/avatars';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { setI18n } from '$lib/i18n';
	import { Preferences, setPreferences } from '$lib/preferences.svelte';
	import { avatarTint, tintStyle } from '$lib/tint';

	let { data, children } = $props();

	// The cookie is only read once; after that this object is the source of truth.
	setPreferences(new Preferences(untrack(() => data.prefs)));
	// Picking another language reloads the page, so this holds for the page's life.
	setI18n(untrack(() => data.locale));

	// What others change while the page is open: notifications, profile names and avatars, and
	// for admins, a new release.
	const signedIn = $derived(!!data.user);
	$effect(() => {
		if (!signedIn) return;
		const source = new EventSource('/api/events');
		source.onmessage = (event) => {
			const { type } = JSON.parse(event.data) as {
				type: 'notifications' | 'profiles' | 'update';
			};
			invalidate(`nolune:${type}`);
		};
		return () => source.close();
	});

	/** A profile's pages show its assistant's avatar in the tab, and take on its color. */
	const avatar = $derived.by(() => {
		const avatar = (page.data as { profile?: { avatar?: string } }).profile?.avatar;
		return isAvatar(avatar) ? avatar : undefined;
	});
	const icon = $derived(avatar ? avatarFavicon(avatar) : favicon);
	const tint = $derived(avatar && avatarTint(avatar));

	/**
	 * The browser's bar matches the page. Until the page knows the mode (and when it follows the
	 * system), each meta covers one system theme; a mode picked in Settings sets both.
	 */
	const themeColor = $derived({
		light: tint?.light.background ?? '#ffffff',
		dark: tint?.dark.background ?? '#212121'
	});
</script>

<svelte:head>
	<link rel="icon" href="/favicon.ico" sizes="32x32" />
	<link rel="icon" href={icon} type="image/svg+xml" />
	<link rel="apple-touch-icon" href="/apple-touch-icon.png" />
	<meta
		name="theme-color"
		media="(prefers-color-scheme: light)"
		content={themeColor[mode.current ?? 'light']}
	/>
	<meta
		name="theme-color"
		media="(prefers-color-scheme: dark)"
		content={themeColor[mode.current ?? 'dark']}
	/>
	{#if tint}
		<!-- Colors worked out from layout.css, not from anything people type. -->
		<!-- eslint-disable-next-line svelte/no-at-html-tags -->
		{@html tintStyle(tint)}
	{/if}
	<title>nolune</title>
</svelte:head>

<ModeWatcher />

<Tooltip.Provider delayDuration={300}>
	<div class="h-dvh">
		{@render children()}
	</div>
</Tooltip.Provider>
