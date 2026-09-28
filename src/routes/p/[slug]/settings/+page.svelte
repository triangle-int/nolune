<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import type { MemberNote, PersonNote } from '@btw/core';
	import { isAvatar, type Avatar } from '@btw/core/avatars';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as Select from '$lib/components/ui/select';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import AvatarPicker from '$lib/components/AvatarPicker.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import UserAvatar from '$lib/components/UserAvatar.svelte';
	import PersonNoteChooser from '$lib/components/memory/PersonNoteChooser.svelte';
	import { getI18n } from '$lib/i18n';
	import { memoryAnchor } from '$lib/memory';
	import { getPreferences } from '$lib/preferences.svelte';

	let { data, form } = $props();

	const prefs = getPreferences();
	const { m } = getI18n();
	let who = $state('');
	let deleteOpen = $state(false);

	/** Whose note is being chosen, and how: see PersonNoteChooser. */
	let choosingFor = $state<string | null>(null);
	let chooser = $state<{
		notes: PersonNote[];
		action: string;
		fields: Record<string, string>;
		current: string | null;
		adding: boolean;
	}>({ notes: [], action: '', fields: {}, current: null, adding: false });

	/** The notes a member's could be: the ones that may be about them first, none of others'. */
	function notesFor(member: MemberNote): PersonNote[] {
		const others = new Set(
			data.members.flatMap((other) => (other.id !== member.id && other.note ? [other.note] : []))
		);
		const first = new Set(member.candidates.map((note) => note.path));
		return [
			...member.candidates,
			...data.people.filter((note) => !first.has(note.path) && !others.has(note.path))
		];
	}

	function chooseFor(member: MemberNote) {
		chooser = {
			notes: notesFor(member),
			action: '?/link',
			fields: { userId: member.id },
			current: member.note,
			adding: false
		};
		choosingFor = member.name;
	}
	/** The avatar just clicked, shown as picked until the page has saved it. */
	let picking = $state<Avatar | null>(null);
	const avatar = $derived(picking ?? data.profile.avatar);

	/** What's in the box; follows the saved soul until someone types. */
	let soul = $derived(data.soul);
	const edited = $derived(soul.trim() !== data.soul);
	let savingSoul = $state(false);
	let soulProblem = $state<string | null>(null);

	/**
	 * Grows with the text up to its max height, then scrolls. Not with `field-sizing: content`
	 * (the Textarea's default): Safari then lays the placeholder out wider than the box.
	 */
	function growWithText(node: HTMLTextAreaElement) {
		void soul;
		node.style.minHeight = '';
		const border = node.offsetHeight - node.clientHeight;
		const max = parseFloat(getComputedStyle(node).maxHeight) || Infinity;
		node.style.minHeight = `${Math.min(node.scrollHeight + border, max)}px`;
	}
</script>

<PageHeader>
	<span class="truncate text-lg font-medium">{m.profile.title}</span>
</PageHeader>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto max-w-xl space-y-10 px-4 py-6 sm:py-10">
		{#if form?.message}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm">{form.message}</p>
		{/if}

		<form method="POST" action="?/rename" use:enhance class="space-y-3">
			<h2 class="font-medium">{m.profile.name}</h2>
			<div class="flex gap-2">
				<Input
					name="name"
					value={data.profile.name}
					required
					aria-label={m.profile.profileName}
					class="h-10 flex-1 rounded-full px-4"
				/>
				<Button type="submit" variant="outline" class="h-10 px-5">{m.common.rename}</Button>
			</div>
			<p class="text-xs text-muted-foreground">
				{m.profile.folder(data.profile.slug)}
			</p>
		</form>

		<form
			method="POST"
			action="?/avatar"
			use:enhance={({ submitter }) => {
				const value = submitter?.getAttribute('value');
				picking = isAvatar(value) ? value : null;
				return async ({ update }) => {
					await update({ reset: false });
					picking = null;
				};
			}}
			class="space-y-3"
		>
			<div class="space-y-1">
				<h2 class="font-medium">{m.profile.avatar}</h2>
				<p class="text-sm text-muted-foreground">
					{m.profile.avatarHint}
				</p>
			</div>
			<AvatarPicker {avatar} />
		</form>

		<section class="space-y-3">
			<div class="space-y-1">
				<h2 class="font-medium">{m.profile.soul}</h2>
				<p class="text-sm text-muted-foreground">
					{m.profile.soulHint(data.profile.name)}
				</p>
			</div>
			<form
				method="POST"
				action="?/soul"
				use:enhance={() => {
					savingSoul = true;
					soulProblem = null;
					return async ({ result, update }) => {
						savingSoul = false;
						if (result.type === 'failure') {
							soulProblem = String(result.data?.message ?? m.errors.couldNotSave);
						} else await update({ reset: false });
					};
				}}
			>
				<Textarea
					name="soul"
					bind:value={soul}
					{@attach growWithText}
					maxlength={data.maxSoul}
					aria-label={m.profile.soul}
					placeholder={m.profile.soulPlaceholder}
					class="field-sizing-fixed! max-h-96 min-h-32"
				/>
				{#if soulProblem}
					<p class="mt-2 text-sm text-destructive">{soulProblem}</p>
				{/if}
				<div class="mt-2 flex min-h-8 items-center gap-2">
					<span class="text-xs text-muted-foreground tabular-nums">
						{m.common.characters(soul.length, data.maxSoul)}
					</span>
					{#if edited}
						<Button variant="ghost" size="sm" class="ml-auto" onclick={() => (soul = data.soul)}
							>{m.common.cancel}</Button
						>
						<Button type="submit" size="sm" disabled={savingSoul}>{m.common.save}</Button>
					{/if}
				</div>
			</form>
			<p class="text-xs text-muted-foreground">
				{prefs.technical ? m.profile.changesTechnical : m.profile.changes}
			</p>
		</section>

		<section class="space-y-3">
			<div class="space-y-1">
				<h2 class="font-medium">{m.profile.members}</h2>
				<p class="text-sm text-muted-foreground">
					{m.profile.membersHint}
				</p>
			</div>
			<ul class="overflow-hidden rounded-2xl border">
				{#each data.members as member (member.id)}
					<li class="flex items-center gap-3 border-b px-4 py-2.5 text-sm last:border-b-0">
						<UserAvatar name={member.name} />
						<div class="min-w-0 flex-1">
							<p class="truncate">{member.name}</p>
							<p class="truncate text-xs text-muted-foreground">
								{#if member.note && member.exists}
									<a
										href="{resolve('/p/[slug]/memory', {
											slug: data.profile.slug
										})}#{memoryAnchor(member.note)}"
										class="underline-offset-2 hover:text-foreground hover:underline"
										>{m.profile.memberNote(member.note)}</a
									>
								{:else if member.note}
									{m.profile.noteStarts(member.note)}
								{:else}
									{m.profile.mayHaveNote}
								{/if}
							</p>
						</div>
						{#if notesFor(member).some((note) => note.path !== member.note)}
							<Button
								variant="ghost"
								size="sm"
								class="text-muted-foreground"
								onclick={() => chooseFor(member)}
								>{member.note ? m.profile.changeNote : m.profile.chooseNote}</Button
							>
						{/if}
						<form method="POST" action="?/remove" use:enhance>
							<input type="hidden" name="userId" value={member.id} />
							<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground"
								>{m.common.remove}</Button
							>
						</form>
					</li>
				{/each}
			</ul>
			{#if data.others.length}
				<form
					method="POST"
					action="?/add"
					use:enhance={() =>
						async ({ result, update }) => {
							const choose =
								result.type === 'success'
									? (result.data?.choose as { who: string; candidates: PersonNote[] } | undefined)
									: undefined;
							if (!choose) return update();
							// Memory may know them already: which note is theirs?
							chooser = {
								notes: choose.candidates,
								action: '?/add',
								fields: { who },
								current: null,
								adding: true
							};
							choosingFor = choose.who;
						}}
					class="flex gap-2"
				>
					<input type="hidden" name="who" value={who} />
					<Select.Root type="single" bind:value={who}>
						<Select.Trigger
							class="h-10 flex-1 rounded-full px-4"
							aria-label={m.profile.personToAdd}
						>
							{who || m.profile.chooseSomeone}
						</Select.Trigger>
						<Select.Content>
							{#each data.others as name (name)}
								<Select.Item value={name}>{name}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
					<Button type="submit" variant="outline" class="h-10 px-5" disabled={!who}
						>{m.common.add}</Button
					>
				</form>
			{:else}
				<p class="text-sm text-muted-foreground">{m.profile.everyoneIsMember}</p>
			{/if}
		</section>

		<section class="space-y-3">
			<h2 class="font-medium text-destructive">{m.profile.deleteTitle}</h2>
			<p class="text-sm text-muted-foreground">
				{m.profile.deleteHint}
			</p>
			<Button variant="destructive" onclick={() => (deleteOpen = true)}
				>{m.profile.deleteButton}</Button
			>
		</section>
	</div>
</div>

<PersonNoteChooser bind:person={choosingFor} {...chooser} />

<AlertDialog.Root bind:open={deleteOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.profile.deleteConfirm(data.profile.name)}</AlertDialog.Title>
			<AlertDialog.Description>
				{m.profile.deleteBody}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form method="POST" action="?/delete">
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive"
					>{m.common.delete}</AlertDialog.Action
				>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
