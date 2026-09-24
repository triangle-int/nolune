<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';

	let { data, children } = $props();
	let menuOpen = $state(false);
</script>

<div class="flex h-full">
	<aside
		class={[
			'w-64 shrink-0 flex-col border-r border-stone-200 bg-white md:flex',
			menuOpen ? 'fixed inset-y-12 left-0 z-20 flex shadow-lg' : 'hidden'
		]}
	>
		<div class="flex items-center justify-between px-4 pt-4 pb-2">
			<a href={resolve('/p/[slug]', { slug: data.profile.slug })} class="truncate font-semibold"
				>{data.profile.name}</a
			>
			<a
				href={resolve('/p/[slug]/settings', { slug: data.profile.slug })}
				class="text-sm text-stone-500 hover:text-stone-900"
				onclick={() => (menuOpen = false)}>Settings</a
			>
		</div>
		<a
			href={resolve('/p/[slug]', { slug: data.profile.slug })}
			onclick={() => (menuOpen = false)}
			class="mx-3 my-2 rounded-md border border-stone-300 px-3 py-1.5 text-center text-sm hover:bg-stone-50"
		>
			New conversation
		</a>
		<nav class="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
			{#each data.conversations as conversation (conversation.id)}
				<a
					href={resolve('/p/[slug]/c/[id]', { slug: data.profile.slug, id: conversation.id })}
					onclick={() => (menuOpen = false)}
					class={[
						'block truncate rounded-md px-2 py-1.5 text-sm',
						page.params.id === conversation.id ? 'bg-stone-100 font-medium' : 'hover:bg-stone-50'
					]}
				>
					{conversation.title}
				</a>
			{:else}
				<p class="px-2 text-sm text-stone-500">No conversations yet.</p>
			{/each}
		</nav>
	</aside>
	<div class="flex min-w-0 flex-1 flex-col">
		<button
			class="border-b border-stone-200 bg-white px-4 py-2 text-left text-sm text-stone-600 md:hidden"
			onclick={() => (menuOpen = !menuOpen)}
		>
			☰ {data.profile.name}
		</button>
		<div class="min-h-0 flex-1">
			{@render children()}
		</div>
	</div>
</div>
