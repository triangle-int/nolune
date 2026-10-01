<script lang="ts">
	import { resolve } from '$app/paths';
	import '../app.css';

	let { children } = $props();
</script>

<svelte:head>
	<title>nolune</title>
</svelte:head>

<main class="mx-auto flex min-h-dvh w-full max-w-sm flex-col justify-center gap-8 px-4 py-12">
	<a href={resolve('/')} class="text-xl font-semibold tracking-tight">nolune</a>
	{@render children()}
</main>
