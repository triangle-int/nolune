<script lang="ts">
	import CopyButton from '$lib/components/chat/CopyButton.svelte';

	/** Text to hand on, like a password or a link, with a button that copies it. */
	let { text, label }: { text: string; label: string } = $props();
</script>

<div class="flex items-center gap-1 rounded-xl bg-muted py-1 pr-1 pl-3">
	<code class="min-w-0 flex-1 font-mono break-all text-foreground">{text}</code>
	<CopyButton {text} {label} />
</div>
