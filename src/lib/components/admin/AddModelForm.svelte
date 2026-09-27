<script lang="ts">
	import PlusIcon from '@lucide/svelte/icons/plus';
	import type { ComponentProps } from 'svelte';
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import PresetForm from './PresetForm.svelte';
	import { getI18n } from '$lib/i18n';

	type Props = Pick<
		ComponentProps<typeof PresetForm>,
		'providers' | 'keys' | 'claudeInstalled' | 'problem'
	> & {
		/** Open from the start: when there are no models yet. */
		startOpen: boolean;
	};

	let { startOpen, ...form }: Props = $props();

	const { m } = getI18n();
	let open = $state(untrack(() => startOpen));
</script>

{#if open}
	<PresetForm {...form} class="rounded-2xl border p-4 sm:p-5" onclose={() => (open = false)} />
{:else}
	<Button variant="outline" class="h-10 rounded-full px-4" onclick={() => (open = true)}>
		<PlusIcon />
		{m.admin.addModel.title}
	</Button>
{/if}
