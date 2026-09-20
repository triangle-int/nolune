<script lang="ts">
	import { page } from "$app/state";
	import MemoryLibraryView from "$lib/components/memory/MemoryLibraryView.svelte";

	const slug = $derived(page.params.slug!);
	/** `?open=<path>` opens one memory (receipt panels and the recall orbit link here). */
	const initialPath = $derived(page.url.searchParams.get("open") ?? "");
</script>

{#key slug}
	<MemoryLibraryView {slug} {initialPath} />
{/key}
