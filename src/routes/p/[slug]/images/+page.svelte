<script lang="ts">
	import { onDestroy } from 'svelte';
	import { MediaQuery } from 'svelte/reactivity';
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import type { SubmitFunction } from '@sveltejs/kit';
	import { Select as SelectPrimitive } from 'bits-ui';
	import { articleBefore } from '@btw/core/articles';
	import CameraIcon from '@lucide/svelte/icons/camera';
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import ImageIcon from '@lucide/svelte/icons/image';
	import ImagePlusIcon from '@lucide/svelte/icons/image-plus';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import PenLineIcon from '@lucide/svelte/icons/pen-line';
	import PencilLineIcon from '@lucide/svelte/icons/pencil-line';
	import RectangleHorizontalIcon from '@lucide/svelte/icons/rectangle-horizontal';
	import RectangleVerticalIcon from '@lucide/svelte/icons/rectangle-vertical';
	import ScanIcon from '@lucide/svelte/icons/scan';
	import SquareIcon from '@lucide/svelte/icons/square';
	import XIcon from '@lucide/svelte/icons/x';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Select from '$lib/components/ui/select';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Rich from '$lib/components/Rich.svelte';
	import Composer from '$lib/components/chat/Composer.svelte';
	import ComposerDock from '$lib/components/chat/ComposerDock.svelte';
	import StepIcon from '$lib/components/chat/StepIcon.svelte';
	import DrawingCanvas from '$lib/components/images/DrawingCanvas.svelte';
	import EmojiChip from '$lib/components/images/EmojiChip.svelte';
	import { getI18n } from '$lib/i18n';
	import { Attachments } from '$lib/uploads.svelte';
	import { cn } from '$lib/utils';

	let { data } = $props();

	const { m } = getI18n();

	type Template = (typeof data.templates)[number];
	type Setting = Template['settings'][number];
	/** A chip's `tail` is the punctuation right after it, kept on its line. */
	type Part =
		| { kind: 'text'; text: string }
		| { kind: 'setting'; setting: Setting; tail: string }
		| { kind: 'image'; tail: string };

	const SHAPES = [
		{ value: 'square', label: m.images.shapes.square, icon: SquareIcon },
		{ value: 'portrait', label: m.images.shapes.portrait, icon: RectangleVerticalIcon },
		{ value: 'landscape', label: m.images.shapes.landscape, icon: RectangleHorizontalIcon },
		{ value: 'auto', label: m.images.shapes.auto, icon: ScanIcon }
	] as const;

	/** Phones get "Take a photo" next to "Choose a photo". */
	const touch = new MediaQuery('(pointer: coarse)');

	const categories = $derived([...new Set(data.templates.map((t) => t.category))]);
	let tab = $state<string | null>(null);
	const activeTab = $derived(tab && categories.includes(tab) ? tab : categories[0]);
	const shown = $derived(data.templates.filter((t) => t.category === activeTab));

	/*
	 * Opening a template shows its sheet. Most start right away: once the photo is picked (or
	 * drawn), or with Try it. Templates with settings go on to a sentence with a chip for each.
	 */
	let chosen = $state<Template | null>(null);
	let step = $state<'sheet' | 'compose' | 'draw'>('sheet');
	/** Where closing the drawing goes back to. */
	let drawnFrom: 'sheet' | 'compose' = 'sheet';
	/** Send as soon as the picture has uploaded. */
	let autoSend = $state(false);
	let starting = $state(false);
	let templateError = $state<string | null>(null);
	let templateForm = $state<HTMLFormElement>();
	let values = $state<Record<string, string>>({});
	/** What was typed for choices set to "Custom…", by setting; a key here means it's typed. */
	let custom = $state<Record<string, string>>({});
	const customInputs: Record<string, HTMLInputElement | undefined> = {};
	/** What each menu opens under: the whole chip, which stays put when it turns into a field. */
	const chipAnchors = $state<Record<string, HTMLElement | undefined>>({});
	let shape = $state('auto');

	/** The template's pictures upload as soon as they're picked, like files in the chat. */
	const photos = new Attachments(() => data.profile.slug, m);
	let photoInput = $state<HTMLInputElement>();
	let cameraInput = $state<HTMLInputElement>();
	let photoNote = $state<string | null>(null);
	const maxPhotos = $derived(Math.max(1, chosen?.maxImages ?? 1));
	const photoProblem = $derived(
		photoNote ?? photos.files.find((f) => f.status === 'failed')?.error ?? null
	);
	const missingPicture = $derived(chosen?.image === 'required' && photos.ids.length === 0);
	const sentence = $derived(chosen ? sentenceParts(chosen) : { parts: [], rest: [] });

	let text = $state('');
	let describing = $state(false);
	let describeError = $state<string | null>(null);
	let describeForm = $state<HTMLFormElement>();
	/** The grid scrolls under the composer, so it ends this far up. */
	let composerHeight = $state(160);
	const describeFiles = new Attachments(() => data.profile.slug, m);

	/** The template's sentence split into words and chips; settings it leaves out come after. */
	function sentenceParts(template: Template): { parts: Part[]; rest: Setting[] } {
		// Every section's chips show, so its markers go; `{{^key}}` sections are for the message.
		const source = (template.sentence ?? `${template.title}.`)
			.replace(/\{\{\^\s*([\w-]+)\s*\}\}[\s\S]*?\{\{\/\s*\1\s*\}\}/g, '')
			.replace(/\{\{[#/]\s*[\w-]+\s*\}\}/g, '');
		const parts: Part[] = [];
		const used: string[] = [];
		let last = 0;
		for (const match of source.matchAll(/\{\{\s*([\w-]+)\s*\}\}/g)) {
			if (match.index > last) parts.push({ kind: 'text', text: source.slice(last, match.index) });
			last = match.index + match[0].length;
			const key = match[1];
			const setting = template.settings.find((s) => s.id === key);
			const tail = source.slice(last).match(/^[.,;:!?)…]+/)?.[0] ?? '';
			last += tail.length;
			if (key === 'image' && template.image !== 'none') parts.push({ kind: 'image', tail });
			else if (setting) parts.push({ kind: 'setting', setting, tail });
			else parts.push({ kind: 'text', text: tail });
			used.push(key);
		}
		if (last < source.length) parts.push({ kind: 'text', text: source.slice(last) });
		if (template.image !== 'none' && !used.includes('image')) {
			parts.push({ kind: 'text', text: ' ' }, { kind: 'image', tail: '' });
		}
		return { parts, rest: template.settings.filter((s) => !used.includes(s.id)) };
	}

	function isPicture(file: File): boolean {
		// HEIC from a Mac often has no type; the server checks the content anyway.
		return file.type.startsWith('image/') || /\.(heic|heif)$/i.test(file.name);
	}

	/** Adds pictures; with room for one, a new one replaces the old. Returns whether any was. */
	function addPhotos(list: FileList | File[] | null | undefined): boolean {
		photoNote = null;
		const files = [...(list ?? [])];
		const pictures = files.filter(isPicture);
		if (pictures.length < files.length) photoNote = m.images.onlyPictures;
		if (!pictures.length) return false;
		if (maxPhotos === 1) {
			for (const file of photos.files) photos.remove(file.key);
			photos.add(pictures.slice(-1));
			return true;
		}
		const room = maxPhotos - photos.files.length;
		if (pictures.length > room) photoNote = m.images.atMostPictures(maxPhotos);
		photos.add(pictures.slice(0, Math.max(0, room)));
		return room > 0;
	}

	/** Takes the pictures off the server too: they were never sent. */
	function dropPhotos() {
		for (const file of photos.files) photos.remove(file.key);
		photoNote = null;
	}

	function open(template: Template) {
		dropPhotos();
		templateError = null;
		autoSend = false;
		step = 'sheet';
		chosen = template;
	}

	function close() {
		if (starting) return;
		dropPhotos();
		autoSend = false;
		chosen = null;
	}

	function compose() {
		if (!chosen) return;
		values = Object.fromEntries(chosen.settings.map((s) => [s.id, s.default]));
		custom = {};
		shape = chosen.size;
		step = 'compose';
	}

	/** After the picture from the sheet: settings to pick, or straight to making it. */
	function next() {
		if (!chosen) return;
		if (chosen.settings.length) compose();
		else if (chosen.image === 'required' && !photos.files.length) return;
		else if (photos.files.length) autoSend = true;
		else templateForm?.requestSubmit();
	}

	function onPicked(event: Event & { currentTarget: HTMLInputElement }) {
		const added = addPhotos(event.currentTarget.files);
		event.currentTarget.value = '';
		if (added && step === 'sheet') next();
	}

	function draw() {
		drawnFrom = step === 'compose' ? 'compose' : 'sheet';
		step = 'draw';
	}

	function onDrawn(file: File) {
		addPhotos([file]);
		step = drawnFrom;
		if (step === 'sheet') next();
	}

	// Sends once the picture is on the server, or gives up if it couldn't get there.
	$effect(() => {
		if (!autoSend || starting || photos.uploading) return;
		autoSend = false;
		if (photos.ids.length && !photos.files.some((f) => f.status === 'failed')) {
			templateForm?.requestSubmit();
		}
	});

	function coverUrl(template: Template): string | null {
		if (template.cover === null) return null;
		const slug = encodeURIComponent(data.profile.slug);
		return `/api/p/${slug}/templates/${encodeURIComponent(template.id)}/cover?v=${template.cover}`;
	}

	/** The menu's last item, which turns the chip into a field for the person's own choice. */
	const CUSTOM = '(custom)';

	function choose(setting: Setting, value: string) {
		if (value === CUSTOM) {
			custom[setting.id] ??= '';
			return;
		}
		delete custom[setting.id];
		values[setting.id] = value;
	}

	/** What a chip reads as now, for "a" or "an" in front of it. */
	function chipText(part: Part | undefined): string {
		if (part?.kind !== 'setting') return '';
		const { setting } = part;
		if (setting.id in custom) return custom[setting.id];
		if (setting.type === 'emoji') return '';
		return optionLabel(setting, values[setting.id] ?? setting.default);
	}

	function optionLabel(setting: Setting, value: string): string {
		if (setting.type !== 'select') return value;
		return setting.options.find((o) => o.value === value)?.label ?? value;
	}

	/** On success the server redirects to the new chat; on failure the form stays as it was. */
	function submit(
		setBusy: (busy: boolean) => void,
		setError: (message: string | null) => void,
		files: Attachments
	): SubmitFunction {
		return () => {
			setBusy(true);
			setError(null);
			return async ({ result, update }) => {
				if (result.type === 'redirect') {
					// The message has the files now; they only need forgetting here.
					files.clear();
					await update();
				} else {
					await update({ reset: false });
					if (result.type === 'failure') {
						setError((result.data?.message as string | undefined) ?? m.errors.thatDidntWork);
					} else if (result.type === 'error') {
						setError(result.error?.message ?? m.errors.somethingWentWrong);
					}
				}
				setBusy(false);
			};
		};
	}

	onDestroy(() => {
		photos.clear();
		describeFiles.clear();
	});

	const chip =
		'rounded-xl border border-foreground/15 bg-muted/70 px-2 align-baseline transition-colors hover:bg-muted focus-within:ring-2 focus-within:ring-ring';
	const bigButton =
		'flex h-12 w-full items-center justify-center gap-2 rounded-full text-base font-medium transition-opacity hover:opacity-85 disabled:opacity-50';
</script>

{#snippet tile(template: Template, className: string, iconClass: string)}
	{@const cover = coverUrl(template)}
	<span
		class={cn('relative block overflow-hidden', !template.color && 'bg-muted', className)}
		style:background-color={template.color}
	>
		{#if cover}
			<img
				src={cover}
				alt=""
				loading="lazy"
				class="absolute inset-0 size-full object-cover transition-transform duration-300 group-hover:scale-[1.03]"
			/>
		{:else}
			<span
				class="absolute inset-0 flex items-center justify-center text-black/40 transition-transform duration-300 group-hover:scale-[1.06]"
			>
				<StepIcon name={template.icon} fallback={ImageIcon} class={iconClass} />
			</span>
		{/if}
	</span>
{/snippet}

{#snippet imageChipFace(template: Template)}
	{@const picture = photos.files[0]}
	{#if picture}
		<span class="relative size-9 overflow-hidden rounded-lg bg-muted">
			{#if picture.preview}
				<img src={picture.preview} alt="" class="size-full object-cover" />
			{:else}
				<ImageIcon class="m-2 size-5 text-muted-foreground" />
			{/if}
			{#if picture.status === 'uploading'}
				<span class="absolute inset-0 flex items-center justify-center bg-black/40 text-white">
					<LoaderCircleIcon class="size-4 animate-spin" />
				</span>
			{/if}
		</span>
	{:else if template.imageSource === 'drawing'}
		<PenLineIcon class="m-1.5 size-6 text-muted-foreground" />
	{:else}
		<ImagePlusIcon class="m-1.5 size-6 text-muted-foreground" />
	{/if}
	<ChevronsUpDownIcon class="size-4 text-muted-foreground" />
{/snippet}

{#snippet imageChip(template: Template)}
	{@const picture = photos.files[0]}
	{@const chipClass = cn(
		chip,
		'inline-flex translate-y-1 items-center gap-1 py-1',
		picture?.status === 'failed' && 'border-destructive'
	)}
	{@const label = picture
		? m.images.changePicture(template.imageLabel)
		: (template.imageLabel ?? m.images.addPicture)}
	{#if template.imageSource === 'drawing'}
		<!-- A drawing can be made here, or be a photo of one on paper. -->
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<button {...props} type="button" class={chipClass} aria-label={label}>
						{@render imageChipFace(template)}
					</button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="start" class="w-56">
				<DropdownMenu.Item onSelect={draw}>
					<PenLineIcon />
					{m.images.draw}
				</DropdownMenu.Item>
				<DropdownMenu.Item onSelect={() => photoInput?.click()}>
					<ImagePlusIcon />
					{m.images.choosePhotoOfDrawing}
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{:else}
		<button type="button" onclick={() => photoInput?.click()} class={chipClass} aria-label={label}>
			{@render imageChipFace(template)}
		</button>
	{/if}
{/snippet}

{#snippet settingChip(setting: Setting)}
	{#if setting.type === 'select'}
		{@const typing = setting.id in custom}
		{@const own = m.images.yourOwn(setting.label)}
		<!-- The chip shows the choice and opens a menu of the others, and "Custom…" turns it into a
		     field for the person's own. Inline-flex, so the whitespace between its parts doesn't show. -->
		<span bind:this={chipAnchors[setting.id]} class="inline-flex">
			<input
				type="hidden"
				name={`setting:${setting.id}`}
				value={typing ? custom[setting.id] : (values[setting.id] ?? setting.default)}
			/>
			<Select.Root
				type="single"
				bind:value={() => (typing ? CUSTOM : values[setting.id]), (value) => choose(setting, value)}
				items={setting.options}
				onOpenChangeComplete={(open) => {
					// Straight into the field after "Custom…", once the menu has gone.
					if (!open && setting.id in custom) customInputs[setting.id]?.focus();
				}}
			>
				{#if typing}
					<span class={cn(chip, 'inline-flex items-center gap-1 pr-1')}>
						<input
							bind:this={customInputs[setting.id]}
							bind:value={custom[setting.id]}
							required
							maxlength={120}
							placeholder={own}
							aria-label={own}
							size={Math.max(6, (custom[setting.id] || own).length)}
							class="min-w-0 bg-transparent outline-none placeholder:text-muted-foreground/70"
						/>
						<SelectPrimitive.Trigger
							class="cursor-pointer rounded-lg p-1 text-muted-foreground hover:text-foreground"
							aria-label={m.images.pickFromList(setting.label)}
						>
							<ChevronsUpDownIcon class="size-4" />
						</SelectPrimitive.Trigger>
					</span>
				{:else}
					<SelectPrimitive.Trigger
						class={cn(chip, 'inline-flex cursor-pointer items-center gap-1 whitespace-nowrap')}
						aria-label={setting.label}
					>
						{optionLabel(setting, values[setting.id] ?? setting.default)}
						<ChevronsUpDownIcon class="size-4 shrink-0 text-muted-foreground" />
					</SelectPrimitive.Trigger>
				{/if}
				<Select.Content align="start" customAnchor={chipAnchors[setting.id]}>
					{#each setting.options as option (option.value)}
						<Select.Item value={option.value} label={option.label} class="py-2.5 text-base" />
					{/each}
					{#if setting.custom}
						<Select.Separator />
						<Select.Item value={CUSTOM} label={m.images.custom} class="py-2.5 text-base">
							<PencilLineIcon class="size-4" />
							{m.images.custom}
						</Select.Item>
					{/if}
				</Select.Content>
			</Select.Root>
		</span>
	{:else if setting.type === 'emoji'}
		<EmojiChip
			name={`setting:${setting.id}`}
			label={setting.label}
			max={setting.max}
			bind:value={values[setting.id]}
			class={chip}
		/>
	{:else}
		<input
			name={`setting:${setting.id}`}
			bind:value={values[setting.id]}
			placeholder={setting.placeholder ?? setting.label.toLowerCase()}
			aria-label={setting.label}
			required={setting.required}
			size={Math.max(4, (values[setting.id] || setting.placeholder || setting.label).length)}
			class={cn(chip, 'max-w-full py-0 outline-none placeholder:text-muted-foreground/70')}
		/>
	{/if}
{/snippet}

<PageHeader>
	<span class="truncate text-lg font-medium">{m.images.title}</span>
</PageHeader>

<div class="relative min-h-0 flex-1">
	<div class="h-full overflow-y-auto">
		<div
			class="mx-auto max-w-4xl space-y-5 px-4 pt-2"
			style:padding-bottom="{composerHeight + 16}px"
		>
			{#if !data.ready}
				<div class="flex items-start gap-3 rounded-2xl bg-muted px-4 py-3 text-sm">
					<CircleAlertIcon class="mt-0.5 size-4 shrink-0 text-warning" />
					<span class="min-w-0">
						<span class="block font-medium">{m.images.cantMakeYet}</span>
						<span class="block text-muted-foreground">
							{#if data.missingKey && data.user?.isAdmin}
								<Rich text={m.images.needsKeyAdmin} provider={data.missingKey.label}>
									{#snippet link()}<a
											href={resolve('/admin')}
											class="font-medium text-foreground underline">{m.images.addKeyLink}</a
										>{/snippet}
								</Rich>
							{:else if data.missingKey}
								{m.images.needsKey(data.missingKey.label)}
							{:else}
								{data.problem} {m.images.adminSetsUp}
							{/if}
						</span>
					</span>
				</div>
			{/if}

			{#if categories.length > 1}
				<div role="tablist" aria-label={m.images.groups} class="flex gap-1">
					{#each categories as category (category)}
						<button
							type="button"
							role="tab"
							aria-selected={category === activeTab}
							onclick={() => (tab = category)}
							class={cn(
								'rounded-full px-4 py-2 text-[15px] transition-colors',
								category === activeTab
									? 'bg-muted font-medium text-foreground'
									: 'text-muted-foreground hover:text-foreground'
							)}
						>
							{category}
						</button>
					{/each}
				</div>
			{/if}

			<div
				role={categories.length > 1 ? 'tabpanel' : undefined}
				class="grid grid-cols-2 gap-3 sm:grid-cols-3 sm:gap-4 lg:grid-cols-4"
			>
				{#each shown as template (template.id)}
					<button
						type="button"
						onclick={() => open(template)}
						class="group relative block overflow-hidden rounded-3xl text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
					>
						{@render tile(template, 'aspect-square', 'size-16 stroke-[1.5]')}
						<span
							class="absolute inset-x-0 bottom-0 bg-linear-to-t from-black/55 to-transparent px-3.5 pt-10 pb-3 text-sm font-semibold text-white"
						>
							{template.name}
						</span>
					</button>
				{:else}
					<p class="col-span-full py-10 text-center text-muted-foreground">
						{m.images.noTemplates}
					</p>
				{/each}
			</div>
		</div>
	</div>

	<ComposerDock bind:height={composerHeight}>
		<form
			bind:this={describeForm}
			method="POST"
			use:enhance={submit(
				(busy) => (describing = busy),
				(message) => (describeError = message),
				describeFiles
			)}
		>
			{#each describeFiles.ids as id (id)}
				<input type="hidden" name="upload" value={id} />
			{/each}
			<Composer
				bind:value={text}
				name="text"
				placeholder={m.images.describe}
				busy={describing || !data.ready}
				attachments={describeFiles}
				onsubmit={() => describeForm?.requestSubmit()}
			/>
			{#if describeError}
				<p class="mt-2 text-center text-sm text-destructive">{describeError}</p>
			{:else}
				<p class="mt-2 hidden text-center text-xs text-muted-foreground sm:block">
					{m.images.describeHint}
				</p>
			{/if}
		</form>
	</ComposerDock>
</div>

<!-- Outside the dialog, so the drawing screen can use them too. -->
<input
	bind:this={photoInput}
	data-picker="photo"
	type="file"
	accept="image/*"
	multiple={maxPhotos > 1}
	class="hidden"
	onchange={onPicked}
/>
<input
	bind:this={cameraInput}
	data-picker="camera"
	type="file"
	accept="image/*"
	capture="environment"
	class="hidden"
	onchange={onPicked}
/>

<Dialog.Root
	open={chosen !== null && step !== 'draw'}
	onOpenChange={(isOpen) => !isOpen && close()}
>
	<Dialog.Content
		showCloseButton={false}
		class={cn(
			'gap-0 overflow-hidden p-0 sm:max-w-md',
			// Phones: a sheet from the bottom, like the ChatGPT app.
			'max-sm:top-auto max-sm:bottom-0 max-sm:left-0 max-sm:max-w-full max-sm:translate-x-0 max-sm:translate-y-0 max-sm:rounded-b-none',
			step === 'compose' &&
				'flex max-h-[calc(100dvh-1rem)] flex-col max-sm:h-[calc(100dvh-0.5rem)] sm:max-w-xl'
		)}
	>
		{#if chosen}
			<form
				bind:this={templateForm}
				method="POST"
				use:enhance={submit(
					(busy) => (starting = busy),
					(message) => (templateError = message),
					photos
				)}
				class={cn(step === 'compose' && 'flex min-h-0 flex-1 flex-col')}
			>
				<input type="hidden" name="template" value={chosen.id} />
				{#each photos.ids as id (id)}
					<input type="hidden" name="upload" value={id} />
				{/each}

				{#if step === 'sheet'}
					{@const busy = starting || autoSend || (photos.uploading && !chosen.settings.length)}
					<div class="relative">
						{@render tile(chosen, 'aspect-square max-h-[50dvh] w-full', 'size-24 stroke-[1.25]')}
						<button
							type="button"
							onclick={close}
							class="absolute top-3 right-3 flex size-9 items-center justify-center rounded-full bg-black/45 text-white backdrop-blur hover:bg-black/60"
							aria-label={m.common.close}
						>
							<XIcon class="size-5" />
						</button>
					</div>
					<div
						class="grid gap-2 px-6 pt-6 pb-[max(1.5rem,env(safe-area-inset-bottom))] text-center"
					>
						<Dialog.Title class="text-2xl font-semibold">{chosen.title}</Dialog.Title>
						{#if chosen.description}
							<Dialog.Description class="line-clamp-3 text-base">
								{chosen.description}
							</Dialog.Description>
						{/if}
						{#if templateError || photoProblem}
							<p class="text-sm text-destructive">{templateError ?? photoProblem}</p>
						{/if}
						<div class="mt-4 grid gap-3">
							{#if busy}
								<div class={cn(bigButton, 'bg-muted text-muted-foreground')} role="status">
									<LoaderCircleIcon class="size-5 animate-spin" />
									{photos.uploading ? m.common.uploading : m.common.starting}
								</div>
							{:else if chosen.imageSource === 'drawing'}
								<button
									type="button"
									onclick={() => photoInput?.click()}
									disabled={!data.ready}
									class={cn(bigButton, 'bg-muted text-foreground')}
								>
									<ImagePlusIcon class="size-5" />
									{m.images.usePhotoOfDrawing}
								</button>
								<button
									type="button"
									onclick={draw}
									disabled={!data.ready}
									class={cn(bigButton, 'bg-primary text-primary-foreground')}
								>
									<PenLineIcon class="size-5" />
									{m.images.startDrawing}
								</button>
							{:else if chosen.image === 'required'}
								{#if touch.current}
									<button
										type="button"
										onclick={() => cameraInput?.click()}
										disabled={!data.ready}
										class={cn(bigButton, 'bg-muted text-foreground')}
									>
										<CameraIcon class="size-5" />
										{m.images.takePhoto}
									</button>
								{/if}
								<button
									type="button"
									onclick={() => photoInput?.click()}
									disabled={!data.ready}
									class={cn(bigButton, 'bg-primary text-primary-foreground')}
								>
									{m.images.choosePhoto}
								</button>
							{:else}
								<button
									type="button"
									onclick={next}
									disabled={!data.ready}
									class={cn(bigButton, 'bg-primary text-primary-foreground')}
								>
									{m.images.tryIt}
								</button>
							{/if}
						</div>
					</div>
				{:else}
					<div class="flex items-center justify-between px-4 pt-4">
						<button
							type="button"
							onclick={close}
							class="flex size-10 items-center justify-center rounded-full hover:bg-muted"
							aria-label={m.common.close}
						>
							<XIcon class="size-5" />
						</button>
						<Dialog.Title class="text-sm font-medium text-muted-foreground">
							{chosen.name}
						</Dialog.Title>
						<span class="size-10"></span>
					</div>

					<div class="min-h-0 flex-1 overflow-y-auto px-6 pt-4 pb-6">
						<p class="text-[26px] leading-[1.75] font-medium tracking-tight">
							{#each sentence.parts as part, i (i)}
								{#if part.kind === 'text'}{articleBefore(
										part.text,
										chipText(sentence.parts[i + 1])
									)}{:else}<span class="whitespace-nowrap"
										>{#if part.kind === 'image'}{@render imageChip(
												chosen
											)}{:else}{@render settingChip(part.setting)}{/if}{part.tail}</span
									>{/if}
							{/each}
						</p>
						{#each sentence.rest as setting (setting.id)}
							<p class="mt-3 text-lg leading-loose">
								<span class="text-muted-foreground">{setting.label}:</span>
								{@render settingChip(setting)}
							</p>
						{/each}
						<textarea
							name="extra"
							rows="2"
							placeholder={m.images.addAnything}
							class="mt-4 w-full resize-none bg-transparent text-xl leading-relaxed outline-none placeholder:text-muted-foreground/60"
						></textarea>
						{#if templateError || photoProblem}
							<p class="text-sm text-destructive">{templateError ?? photoProblem}</p>
						{/if}
					</div>

					<div
						class="flex items-center gap-3 border-t px-4 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]"
					>
						<Select.Root type="single" name="shape" bind:value={shape} items={[...SHAPES]}>
							{#each SHAPES.filter((s) => s.value === shape) as current (current.value)}
								<SelectPrimitive.Trigger
									class={cn(chip, 'inline-flex cursor-pointer items-center gap-1.5 py-1.5 text-sm')}
									aria-label={m.images.shape}
								>
									<current.icon class="size-4" />
									{current.label}
									<ChevronsUpDownIcon class="size-3.5 text-muted-foreground" />
								</SelectPrimitive.Trigger>
							{/each}
							<Select.Content align="start" side="top">
								{#each SHAPES as option (option.value)}
									<Select.Item value={option.value} label={option.label}>
										<option.icon class="size-4" />
										{option.label}
									</Select.Item>
								{/each}
							</Select.Content>
						</Select.Root>
						<span class="min-w-0 flex-1 truncate text-xs text-muted-foreground">
							{photos.uploading
								? m.images.uploadingPicture
								: missingPicture
									? chosen.imageSource === 'drawing'
										? m.images.addDrawingFirst
										: m.images.addPhotoFirst
									: ''}
						</span>
						<button
							type="submit"
							disabled={starting || missingPicture || photos.uploading || !data.ready}
							class="h-10 rounded-full bg-primary px-5 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-85 disabled:opacity-40"
						>
							{starting ? m.common.starting : m.images.generate}
						</button>
					</div>
				{/if}
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

{#if chosen && step === 'draw'}
	<DrawingCanvas ondone={onDrawn} onclose={() => (step = drawnFrom)} />
{/if}
