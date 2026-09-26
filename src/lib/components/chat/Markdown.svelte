<script lang="ts">
	import { codeCopyButtons, renderMarkdown, type MediaContext } from '$lib/markdown';
	import { cn } from '$lib/utils';

	interface Props {
		text: string;
		/** For replies: where their pictures and files come from. */
		media?: MediaContext;
		class?: string;
	}

	let { text, media, class: className }: Props = $props();

	const html = $derived(renderMarkdown(text, media));
</script>

<div class={cn('markdown', className)} {@attach codeCopyButtons}>
	<!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized by DOMPurify in renderMarkdown -->
	{@html html}
</div>
