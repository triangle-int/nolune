<script lang="ts">
	import './layout.css';
	import { untrack } from 'svelte';
	import { ModeWatcher } from 'mode-watcher';
	import favicon from '$lib/assets/favicon.svg';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { Preferences, setPreferences } from '$lib/preferences.svelte';

	let { data, children } = $props();

	// The cookie is only read once; after that this object is the source of truth.
	setPreferences(new Preferences(untrack(() => data.prefs)));
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<title>btw</title>
</svelte:head>

<ModeWatcher themeColors={{ light: '#ffffff', dark: '#212121' }} />

<Tooltip.Provider delayDuration={300}>
	<div class="h-dvh">
		{@render children()}
	</div>
</Tooltip.Provider>
