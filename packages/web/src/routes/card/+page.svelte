<script lang="ts">
	import { onMount } from 'svelte';
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import type { SubmitFunction } from '@sveltejs/kit';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import EraserIcon from '@lucide/svelte/icons/eraser';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import Rich from '$lib/components/Rich.svelte';
	import TopBar from '$lib/components/TopBar.svelte';
	import UserAvatar from '$lib/components/UserAvatar.svelte';
	import Markdown from '$lib/components/chat/Markdown.svelte';
	import MemoryChangeItem from '$lib/components/memory/MemoryChangeItem.svelte';
	import { formatAgo } from '$lib/format';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	let { data, form } = $props();

	const i18n = getI18n();
	const { m } = i18n;

	const facts = $derived(data.file?.facts.length ?? 0);
	let editing = $state(false);
	let draft = $state('');
	let saving = $state(false);
	let clearing = $state(false);

	/** Checked at first: what several notes say, and rules pinned in a profile. */
	let picked = $derived(data.candidates.filter((c) => c.suggested).map((c) => c.id));
	/** The candidates by the profile they're in. */
	const groups = $derived(
		[...new Set(data.candidates.map((c) => c.profile.name))].map(
			(profile) => [profile, data.candidates.filter((c) => c.profile.name === profile)] as const
		)
	);

	/** Markdown renders in the browser only, and {@html} isn't redone when the page hydrates. */
	let mounted = $state(false);
	onMount(() => {
		mounted = true;
		if (form && 'editing' in form && form.editing) editing = true;
	});

	function edit() {
		editing = true;
		draft = data.file?.text ?? `# ${data.card.owner}\n\n`;
	}

	const save: SubmitFunction = () => {
		saving = true;
		return async ({ result, update }) => {
			saving = false;
			if (result.type === 'success') editing = false;
			// nolune changed the card meanwhile: load its version, so saving again replaces it knowingly.
			if (result.type === 'failure' && result.status === 409) await invalidateAll();
			await update({ reset: false });
		};
	};
</script>

<div class="flex h-full flex-col">
	<TopBar />
	<main class="min-h-0 flex-1 overflow-y-auto">
		<div class="mx-auto max-w-2xl space-y-8 px-4 py-8 sm:py-12">
			<div class="space-y-1">
				<h1 class="text-2xl font-semibold">{m.card.title}</h1>
				<p class="text-muted-foreground">{m.card.intro}</p>
			</div>

			{#if form?.message && !('editing' in form)}
				<p class="rounded-2xl bg-muted px-4 py-3 text-sm" role="status">{form.message}</p>
			{/if}

			<section class="rounded-3xl border p-4 sm:p-5" aria-label={m.card.title}>
				<div class="flex items-start gap-3">
					<UserAvatar name={data.card.owner} picture={data.user?.picture} class="size-8" />
					<div class="min-w-0 flex-1">
						<h2 class="font-medium">{data.card.owner}</h2>
						<p class="truncate text-xs text-muted-foreground">
							{[
								data.card.path,
								facts && m.memory.memories(facts),
								facts && data.file && m.memory.updated(formatAgo(data.file.updatedAt, i18n))
							]
								.filter(Boolean)
								.join(' · ')}
						</p>
					</div>
					{#if !editing}
						<Button
							variant="ghost"
							size="icon-sm"
							class="-mt-1 text-muted-foreground"
							aria-label={m.card.edit}
							title={m.card.edit}
							onclick={edit}
						>
							<PencilIcon />
						</Button>
						{#if facts}
							<Button
								variant="ghost"
								size="icon-sm"
								class="-mt-1 -mr-1 text-muted-foreground hover:text-destructive"
								aria-label={m.card.clear}
								title={m.card.clear}
								onclick={() => (clearing = true)}
							>
								<EraserIcon />
							</Button>
						{/if}
					{/if}
				</div>

				{#if editing}
					<form method="POST" action="?/save" use:enhance={save} class="mt-3 space-y-3">
						<input type="hidden" name="basedOn" value={data.file?.updatedAt ?? 0} />
						<Textarea
							name="text"
							bind:value={draft}
							rows={Math.min(18, Math.max(6, draft.split('\n').length + 1))}
							class="rounded-2xl font-mono text-xs"
							aria-label={m.card.title}
							placeholder={m.card.placeholder}
						/>
						{#if form && 'editing' in form && form.message}
							<p class="text-sm {'conflict' in form ? 'text-warning' : 'text-destructive'}">
								{form.message}
							</p>
						{/if}
						<div class="flex items-center gap-2">
							<Button type="submit" size="sm" disabled={saving}>{m.common.save}</Button>
							<Button type="button" variant="ghost" size="sm" onclick={() => (editing = false)}
								>{m.common.cancel}</Button
							>
							<span
								class={cn(
									'ml-auto text-xs text-muted-foreground tabular-nums',
									draft.length > data.maxChars && 'text-destructive'
								)}
							>
								{m.common.characters(draft.length, data.maxChars)}
							</span>
						</div>
					</form>
				{:else if !facts || !data.file}
					<p class="mt-3 text-sm text-muted-foreground">{m.card.empty}</p>
				{:else if !mounted}
					<p class="mt-3 text-sm whitespace-pre-line">{data.file.text}</p>
				{:else}
					<Markdown text={data.file.text} class="mt-3 text-sm" />
				{/if}

				<p class="mt-3 text-xs text-muted-foreground">
					{data.profiles.length
						? m.card.readBy(data.profiles.map((p) => p.name))
						: m.card.inNoProfile}
				</p>
			</section>

			{#if data.candidates.length}
				<section id="bring" class="scroll-mt-6 space-y-3" aria-labelledby="bring-heading">
					<div class="space-y-0.5 px-1">
						<h2 id="bring-heading" class="font-medium">{m.card.bring.title}</h2>
						<p class="text-sm text-muted-foreground">{m.card.bring.intro}</p>
					</div>
					<form method="POST" action="?/bring" use:enhance class="space-y-3">
						<div class="space-y-4 rounded-3xl border p-4 text-sm sm:p-5">
							{#each groups as [profile, candidates] (profile)}
								<fieldset class="space-y-2">
									<legend
										class="pb-1 text-xs font-medium tracking-wider text-muted-foreground uppercase"
									>
										{profile}
									</legend>
									{#each candidates as candidate (candidate.id)}
										<label class="flex cursor-pointer items-start gap-3">
											<input
												type="checkbox"
												name="fact"
												value={candidate.id}
												bind:group={picked}
												class="mt-0.5 size-4 shrink-0 accent-primary"
											/>
											<span class="min-w-0 flex-1 break-words">
												{candidate.text}
												{#if candidate.note === 'core.md'}
													<span class="text-xs text-muted-foreground">· {m.card.bring.rule}</span>
												{/if}
											</span>
										</label>
									{/each}
								</fieldset>
							{/each}
						</div>
						<Button type="submit" size="sm" disabled={!picked.length}>
							{m.card.bring.add(picked.length)}
						</Button>
					</form>
				</section>
			{/if}

			{#if data.changes.length}
				<section class="space-y-3" aria-labelledby="changes-heading">
					<div class="space-y-0.5 px-1">
						<h2 id="changes-heading" class="font-medium">{m.card.changes.title}</h2>
						<p class="text-sm text-muted-foreground">{m.card.changes.intro}</p>
					</div>
					<ul class="space-y-3 rounded-3xl border p-4 text-sm sm:p-5">
						{#each data.changes as change (change.id)}
							<MemoryChangeItem
								change={{
									...change,
									note: data.card.path,
									card: { ownerId: data.user?.id ?? '', owner: data.card.owner }
								}}
								slug={change.profile.slug}
								profile={change.profile.name}
								onundone={invalidateAll}
							>
								{#snippet note()}
									{#if change.conversation}
										<Rich text={m.card.changes.fromChat} profile={change.profile.name}>
											{#snippet chat()}<a
													href={resolve('/p/[slug]/c/[id]', {
														slug: change.profile.slug,
														id: change.conversation!.id
													})}
													class="underline-offset-2 hover:text-foreground hover:underline"
													>{change.conversation!.title || m.memory.changes.untitled}</a
												>{/snippet}
										</Rich>
									{:else}
										<Rich text={m.card.changes.inProfile} profile={change.profile.name} />
									{/if}
								{/snippet}
								{#snippet after()}
									<span>{formatAgo(change.createdAt, i18n)}</span>
								{/snippet}
							</MemoryChangeItem>
						{/each}
					</ul>
				</section>
			{/if}
		</div>
	</main>
</div>

<AlertDialog.Root open={clearing} onOpenChange={(open) => !open && (clearing = false)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.card.clearTitle}</AlertDialog.Title>
			<AlertDialog.Description>{m.card.clearBody}</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/clear"
			use:enhance={() =>
				async ({ update }) => {
					clearing = false;
					await update();
				}}
		>
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive">{m.card.clear}</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
