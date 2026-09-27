<script lang="ts">
	import type { CustomProviderStatus } from '@btw/core';
	import type { Snippet } from 'svelte';
	import { Input } from '$lib/components/ui/input';
	import { getI18n } from '$lib/i18n';

	/**
	 * A custom provider's name, address and key, to add one (`provider` null) or change it; its
	 * API, when it's added, goes in `children`, after the name. `url` is bound, so the key's
	 * placeholder says when the saved key stays: only for the same address.
	 */
	let {
		provider,
		url = $bindable(),
		children
	}: { provider: CustomProviderStatus | null; url: string; children?: Snippet } = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.customProviders);
	const uid = $props.id();
</script>

<div class="space-y-2">
	<label for="{uid}-name" class="block font-medium">{t.name}</label>
	<Input
		id="{uid}-name"
		name="name"
		value={provider?.name ?? ''}
		required
		maxlength={40}
		autocomplete="off"
		placeholder={t.namePlaceholder}
		class="h-10 rounded-full px-4 sm:w-60"
	/>
</div>

{@render children?.()}

<div class="space-y-2">
	<label for="{uid}-url" class="block font-medium">{t.address}</label>
	<Input
		id="{uid}-url"
		name="url"
		bind:value={url}
		required
		autocomplete="off"
		spellcheck="false"
		placeholder="http://localhost:11434"
		aria-describedby="{uid}-url-hint"
		class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
	/>
	<p id="{uid}-url-hint" class="text-muted-foreground">{t.addressHint}</p>
</div>

<div class="space-y-2">
	<label for="{uid}-key" class="block font-medium">
		{t.key} <span class="font-normal text-muted-foreground">{t.keyOptional}</span>
	</label>
	<Input
		id="{uid}-key"
		name="key"
		type="password"
		autocomplete="off"
		spellcheck="false"
		placeholder={provider?.hasKey && url.trim().replace(/\/+$/, '') === provider.url
			? t.keyKept
			: ''}
		class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
	/>
</div>
