<script lang="ts">
	import { page } from "$app/state";
	import { SETTINGS_SECTIONS, sectionForPath, sectionHref } from "$lib/settings/sections.js";
	import "$lib/settings/settings.css";

	let { children } = $props();

	const slug = $derived(page.params.slug!);
	const current = $derived(sectionForPath(page.url.pathname));
	const section = $derived(SETTINGS_SECTIONS.find((s) => s.id === current) ?? SETTINGS_SECTIONS[0]);
</script>

<div class="settings-page">
	<header class="settings-head">
		<h2 class="settings-title">Settings</h2>
		<nav class="settings-nav" aria-label="Settings sections">
			{#each SETTINGS_SECTIONS as s (s.id)}
				<a
					href={sectionHref(slug, s.id)}
					class="settings-nav-link"
					class:settings-nav-active={s.id === current}
					aria-current={s.id === current ? "page" : undefined}
				>
					{s.label}
				</a>
			{/each}
		</nav>
		<p class="settings-lead">{section.description}</p>
	</header>

	<div class="settings-panel">
		{@render children()}
	</div>
</div>
