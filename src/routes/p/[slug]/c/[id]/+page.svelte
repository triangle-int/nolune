<script lang="ts">
	import Chat from '$lib/components/Chat.svelte';

	let { data } = $props();
</script>

{#key data.conversation.id}
	<Chat
		conversation={data.conversation}
		efforts={data.efforts}
		presets={data.presets}
		defaultPresetId={data.defaultPresetId}
		me={data.user?.name ?? ''}
		folders={data.folders}
		folderId={data.conversations.find((c) => c.id === data.conversation.id)?.folderId ?? null}
		avatar={data.profile.avatar}
	/>
{/key}
