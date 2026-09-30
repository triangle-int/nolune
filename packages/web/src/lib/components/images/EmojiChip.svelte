<script lang="ts">
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import DeleteIcon from '@lucide/svelte/icons/delete';
	import SmilePlusIcon from '@lucide/svelte/icons/smile-plus';
	import { mode } from 'mode-watcher';
	import type { I18n as PickerLabels } from 'emoji-picker-element/shared';
	// Served by nolune itself, so the picker works without reaching a CDN. It's fetched the first
	// time the picker opens and kept in the browser's IndexedDB after that.
	import deData from 'emoji-picker-element-data/de/cldr/data.json?url';
	import enData from 'emoji-picker-element-data/en/emojibase/data.json?url';
	import esData from 'emoji-picker-element-data/es/cldr/data.json?url';
	import frData from 'emoji-picker-element-data/fr/emojibase/data.json?url';
	import ruData from 'emoji-picker-element-data/ru/emojibase/data.json?url';
	import * as Popover from '$lib/components/ui/popover';
	import { getI18n, type Locale } from '$lib/i18n';
	import { cn } from '$lib/utils';

	/** The emoji's names and search words, and the picker's own labels, in each language. */
	const PICKER: Record<
		Locale,
		{ data: string; labels?: () => Promise<{ default: PickerLabels }> }
	> = {
		en: { data: enData },
		ru: { data: ruData, labels: () => import('emoji-picker-element/i18n/ru_RU.js') },
		de: { data: deData, labels: () => import('emoji-picker-element/i18n/de.js') },
		es: { data: esData, labels: () => import('emoji-picker-element/i18n/es.js') },
		fr: { data: frData, labels: () => import('emoji-picker-element/i18n/fr.js') }
	};

	interface Props {
		/** The form field it fills. */
		name: string;
		label: string;
		/** How many emoji it holds; picking one more drops the oldest. */
		max: number;
		value?: string;
		class?: string;
	}

	let { name, label, max, value = $bindable(''), class: className }: Props = $props();

	const { m, locale } = getI18n();

	let open = $state(false);
	let picker = $state<HTMLElement>();

	const graphemes = new Intl.Segmenter('en', { granularity: 'grapheme' });
	const emoji = $derived(
		[...graphemes.segment(value)].map((s) => s.segment).filter((s) => s.trim())
	);

	function add(unicode: string) {
		value = [...emoji, unicode].slice(-max).join('');
	}

	function removeLast() {
		value = emoji.slice(0, -1).join('');
	}

	/** emoji-picker-element is a web component that needs the browser: loaded on first open. */
	function mountPicker(node: HTMLElement) {
		let element: HTMLElement | undefined;
		let gone = false;
		const { data, labels } = PICKER[locale];
		void Promise.all([import('emoji-picker-element'), labels?.()]).then(([{ Picker }, i18n]) => {
			if (gone) return;
			const created = new Picker({ dataSource: data, locale, i18n: i18n?.default });
			created.addEventListener('emoji-click', (event) => {
				if (event.detail.unicode) add(event.detail.unicode);
			});
			node.append(created);
			element = picker = created;
		});
		return () => {
			gone = true;
			element?.remove();
		};
	}

	// Follows nolune's own light or dark setting, not only the system's.
	$effect(() => {
		if (picker) picker.className = mode.current === 'dark' ? 'dark' : 'light';
	});
</script>

<!-- Inline-flex, so no stray space shows between the chip and the text after it. -->
<span class="inline-flex">
	<input type="hidden" {name} {value} />
	<Popover.Root bind:open>
		<Popover.Trigger
			class={cn('inline-flex items-center gap-1 whitespace-nowrap', className)}
			aria-label={m.emoji.value(label, value)}
		>
			{#if emoji.length}
				{value}
			{:else}
				<SmilePlusIcon class="m-1.5 size-6 text-muted-foreground" />
			{/if}
			<ChevronsUpDownIcon class="size-4 shrink-0 text-muted-foreground" />
		</Popover.Trigger>
		<Popover.Content
			collisionPadding={8}
			class="w-[min(22rem,calc(100vw-1rem))] gap-0 overflow-hidden p-0"
		>
			<div class="flex items-center gap-2 border-b py-2 pr-2 pl-4">
				<span class="min-w-0 flex-1 truncate text-2xl leading-10">
					{#if emoji.length}
						{value}
					{:else}
						<span class="text-sm text-muted-foreground">{m.emoji.pickUpTo(max)}</span>
					{/if}
				</span>
				<button
					type="button"
					onclick={removeLast}
					disabled={!emoji.length}
					class="flex size-10 items-center justify-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-40"
					aria-label={m.emoji.removeLast}
				>
					<DeleteIcon class="size-5" />
				</button>
				<button
					type="button"
					onclick={() => (open = false)}
					class="h-10 rounded-full bg-primary px-4 text-sm font-medium text-primary-foreground hover:opacity-85"
				>
					{m.common.done}
				</button>
			</div>
			<div class="emoji-picker" {@attach mountPicker}></div>
		</Popover.Content>
	</Popover.Root>
</span>

<style>
	/* The picker's own variables, set from nolune's colors so it matches both themes. */
	.emoji-picker :global(emoji-picker) {
		display: block;
		width: 100%;
		height: 20rem;
		--background: var(--popover);
		--border-size: 0;
		--border-color: var(--border);
		--button-active-background: var(--accent);
		--button-hover-background: var(--muted);
		--indicator-color: var(--foreground);
		--input-border-color: var(--input);
		--input-border-radius: 9999px;
		--input-font-color: var(--foreground);
		--input-padding: 0.375rem 0.75rem;
		--input-placeholder-color: var(--muted-foreground);
		--outline-color: var(--ring);
		--num-columns: 8;
		--emoji-size: 1.5rem;
		/* All nine category tabs fit a phone's width. */
		--category-emoji-size: 1.25rem;
		--category-emoji-padding: 0.4rem;
	}
</style>
