<script lang="ts">
	import { enhance } from '$app/forms';
	import { MEMORY_CATEGORIES, categoryOf, noteName } from '@btw/core/memory-categories';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import * as Select from '$lib/components/ui/select';
	import { getI18n } from '$lib/i18n';

	/**
	 * Moves a note into a category, or a note of its own for a person or project. When that note is
	 * there already, the two are merged: two notes about one person, say. Open while `path` is set.
	 * `notes`: every note, with the title the page shows.
	 */
	let {
		path = $bindable(),
		notes
	}: {
		path: string | null;
		notes: { path: string; title: string }[];
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.memory.move);

	const NEW_PERSON = '+people';
	const NEW_PROJECT = '+projects';

	let choice = $state('');
	let name = $state('');
	let saving = $state(false);
	let problem = $state<string | null>(null);

	const titles = $derived(new Map(notes.map((note) => [note.path, note.title])));
	const title = $derived(path ? (titles.get(path) ?? path) : '');
	const categories = $derived(
		MEMORY_CATEGORIES.filter((c) => c !== 'core' && c !== 'people').map((c) => ({
			path: `${c}.md`,
			title: m.memory.categories[c]
		}))
	);
	const inFolder = (folder: string) =>
		notes.filter((note) => note.path.startsWith(`${folder}/`) && categoryOf(note.path));
	const people = $derived(inFolder('people'));
	const projects = $derived(inFolder('projects'));

	/** Where it goes. */
	const target = $derived(
		choice === NEW_PERSON || choice === NEW_PROJECT
			? name.trim() && `${choice.slice(1)}/${noteName(name)}.md`
			: choice
	);
	const merging = $derived(!!target && titles.has(target));

	function close() {
		path = null;
		choice = '';
		name = '';
		problem = null;
	}
</script>

<Dialog.Root open={path !== null} onOpenChange={(isOpen) => !isOpen && close()}>
	<Dialog.Content>
		{#if path}
			<form
				method="POST"
				action="?/move"
				class="grid min-w-0 gap-4"
				use:enhance={() => {
					saving = true;
					problem = null;
					return async ({ result, update }) => {
						saving = false;
						if (result.type === 'failure') {
							problem = String(result.data?.message ?? m.errors.requestFailed(result.status));
							return;
						}
						close();
						await update();
					};
				}}
			>
				<input type="hidden" name="from" value={path} />
				<input type="hidden" name="to" value={target} />
				<Dialog.Header>
					<Dialog.Title>{t.title(title)}</Dialog.Title>
					<Dialog.Description>{t.body}</Dialog.Description>
				</Dialog.Header>
				<Select.Root type="single" bind:value={choice}>
					<Select.Trigger class="h-10 w-full rounded-full px-4" aria-label={t.to}>
						{choice === NEW_PERSON
							? t.newPerson
							: choice === NEW_PROJECT
								? t.newProject
								: choice
									? (titles.get(choice) ?? categories.find((c) => c.path === choice)?.title)
									: t.choose}
					</Select.Trigger>
					<Select.Content>
						<Select.Group>
							<Select.GroupHeading>{t.categories}</Select.GroupHeading>
							{#each categories.filter((c) => c.path !== path) as option (option.path)}
								<Select.Item value={option.path}>{option.title}</Select.Item>
							{/each}
						</Select.Group>
						<Select.Group>
							<Select.GroupHeading>{m.memory.categories.people}</Select.GroupHeading>
							{#each people.filter((note) => note.path !== path) as note (note.path)}
								<Select.Item value={note.path}>{note.title}</Select.Item>
							{/each}
							<Select.Item value={NEW_PERSON}>{t.newPerson}</Select.Item>
						</Select.Group>
						<Select.Group>
							<Select.GroupHeading>{m.memory.categories.projects}</Select.GroupHeading>
							{#each projects.filter((note) => note.path !== path) as note (note.path)}
								<Select.Item value={note.path}>{note.title}</Select.Item>
							{/each}
							<Select.Item value={NEW_PROJECT}>{t.newProject}</Select.Item>
						</Select.Group>
					</Select.Content>
				</Select.Root>
				{#if choice === NEW_PERSON || choice === NEW_PROJECT}
					<Input
						bind:value={name}
						required
						maxlength={60}
						placeholder={choice === NEW_PERSON ? t.personName : t.projectName}
						aria-label={choice === NEW_PERSON ? t.personName : t.projectName}
						class="h-10 rounded-full px-4"
					/>
				{/if}
				{#if target}
					<p class="text-sm text-muted-foreground">
						{merging ? t.mergeHint(title, titles.get(target) ?? target) : t.moveHint(target)}
					</p>
				{/if}
				{#if problem}
					<p class="text-sm text-destructive" role="alert">{problem}</p>
				{/if}
				<Dialog.Footer>
					<Button type="button" variant="ghost" onclick={close}>{m.common.cancel}</Button>
					<Button type="submit" disabled={saving || !target}>
						{merging ? t.merge : t.move}
					</Button>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>
