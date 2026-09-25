<script lang="ts">
	import { codeCopyButtons, renderMarkdown } from '$lib/markdown';
	import { cn } from '$lib/utils';

	interface Props {
		text: string;
		class?: string;
	}

	let { text, class: className }: Props = $props();

	const html = $derived(renderMarkdown(text));
</script>

<div class={cn('markdown', className)} {@attach codeCopyButtons}>
	<!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized by DOMPurify in renderMarkdown -->
	{@html html}
</div>
