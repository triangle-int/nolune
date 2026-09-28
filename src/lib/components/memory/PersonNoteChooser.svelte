<script lang="ts">
	import { enhance } from '$app/forms';
	import type { PersonNote } from '@btw/core';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { getI18n } from '$lib/i18n';
	import { memoryTopic } from '$lib/memory';
	import { cn } from '$lib/utils';

	/**
	 * Which note in memory is about someone: when they're added and memory may know them already,
	 * or to change it later. Each note shows what it says, and choosing one says they'll read it,
	 * since it may have been written without them. Open while `person` is set. `fields`: the
	 * form's other inputs, like who to add. `current`: their note now, if any.
	 */
	let {
		person = $bindable(),
		notes,
		action,
		fields,
		current = null,
		adding = false
	}: {
		person: string | null;
		notes: PersonNote[];
		action: string;
		fields: Record<string, string>;
		current?: string | null;
		adding?: boolean;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.profile.chooser);

	/** Facts shown of each note. */
	const SHOWN = 3;

	let choice = $state<string | null>(null);
	let saving = $state(false);
	let problem = $state<string | null>(null);

	function close() {
		person = null;
		choice = null;
		problem = null;
	}
</script>

<Dialog.Root open={person !== null} onOpenChange={(isOpen) => !isOpen && close()}>
	<Dialog.Content class="sm:max-w-lg">
		{#if person}
			<form
				method="POST"
				{action}
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
				{#each Object.entries(fields) as [name, value] (name)}
					<input type="hidden" {name} {value} />
				{/each}
				<Dialog.Header>
					<Dialog.Title>{adding ? t.addTitle(person) : t.linkTitle(person)}</Dialog.Title>
					<Dialog.Description>{adding ? t.body(person) : t.linkBody(person)}</Dialog.Description>
				</Dialog.Header>
				<fieldset class="-mx-1 max-h-[50vh] min-w-0 space-y-2 overflow-y-auto px-1">
					<legend class="sr-only">{t.linkTitle(person)}</legend>
					{#each notes as note (note.path)}
						<label
							class={cn(
								'flex cursor-pointer gap-3 rounded-2xl border p-3 text-sm transition-colors',
								choice === note.path ? 'border-foreground/40 bg-muted/60' : 'hover:bg-muted/40'
							)}
						>
							<input
								type="radio"
								name="note"
								value={note.path}
								bind:group={choice}
								class="mt-1 accent-foreground"
							/>
							<span class="min-w-0 flex-1 space-y-1">
								<span class="flex flex-wrap items-baseline gap-x-2">
									<span class="font-medium">{note.title ?? memoryTopic(note.path)}</span>
									<span class="text-xs text-muted-foreground">
										{note.path}{note.path === current ? ` · ${t.current}` : ''}
									</span>
								</span>
								{#if note.who}<span class="block text-muted-foreground">{note.who}</span>{/if}
								{#if note.aliases.length}
									<span class="block text-xs text-muted-foreground">
										{t.alsoCalled(note.aliases.join(', '))}
									</span>
								{/if}
								{#if note.facts.length}
									<ul class="space-y-0.5 text-muted-foreground">
										{#each note.facts.slice(0, SHOWN) as fact, i (i)}
											<li class="truncate">· {fact}</li>
										{/each}
									</ul>
									{#if note.facts.length > SHOWN}
										<span class="block text-xs text-muted-foreground">
											{t.more(note.facts.length - SHOWN)}
										</span>
									{/if}
								{/if}
							</span>
						</label>
					{/each}
					<label
						class={cn(
							'flex cursor-pointer items-center gap-3 rounded-2xl border p-3 text-sm transition-colors',
							choice === 'new' ? 'border-foreground/40 bg-muted/60' : 'hover:bg-muted/40'
						)}
					>
						<input
							type="radio"
							name="note"
							value="new"
							bind:group={choice}
							class="accent-foreground"
						/>
						<span>{t.newNote}</span>
					</label>
				</fieldset>
				{#if choice && choice !== 'new' && choice !== current}
					<p class="rounded-2xl bg-muted px-4 py-3 text-sm" role="note">{t.privacy(person)}</p>
				{/if}
				{#if problem}
					<p class="text-sm text-destructive" role="alert">{problem}</p>
				{/if}
				<Dialog.Footer>
					<Button type="button" variant="ghost" onclick={close}>{m.common.cancel}</Button>
					<Button type="submit" disabled={saving || !choice || choice === current}>
						{adding ? t.add(person) : t.link}
					</Button>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>
