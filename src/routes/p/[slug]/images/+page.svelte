<script lang="ts">
	import { onDestroy } from 'svelte';
	import { MediaQuery } from 'svelte/reactivity';
	import { enhance } from '$app/forms';
	import type { SubmitFunction } from '@sveltejs/kit';
	import CameraIcon from '@lucide/svelte/icons/camera';
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import ImageIcon from '@lucide/svelte/icons/image';
	import ImagePlusIcon from '@lucide/svelte/icons/image-plus';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import PenLineIcon from '@lucide/svelte/icons/pen-line';
	import RectangleHorizontalIcon from '@lucide/svelte/icons/rectangle-horizontal';
	import RectangleVerticalIcon from '@lucide/svelte/icons/rectangle-vertical';
	import ScanIcon from '@lucide/svelte/icons/scan';
	import SquareIcon from '@lucide/svelte/icons/square';
	import XIcon from '@lucide/svelte/icons/x';
	import * as Dialog from '$lib/components/ui/dialog';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Composer from '$lib/components/chat/Composer.svelte';
	import StepIcon from '$lib/components/chat/StepIcon.svelte';
	import DrawingCanvas from '$lib/components/images/DrawingCanvas.svelte';
	import { Attachments } from '$lib/uploads.svelte';
	import { cn } from '$lib/utils';

	let { data } = $props();

	type Template = (typeof data.templates)[number];
	type Setting = Template['settings'][number];
	type Part =
		{ kind: 'text'; text: string } | { kind: 'setting'; setting: Setting } | { kind: 'image' };

	const SHAPES = [
		{ value: 'square', label: 'Square', icon: SquareIcon },
		{ value: 'portrait', label: 'Portrait', icon: RectangleVerticalIcon },
		{ value: 'landscape', label: 'Landscape', icon: RectangleHorizontalIcon },
		{ value: 'auto', label: 'Auto', icon: ScanIcon }
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
	let shape = $state('auto');

	/** The template's pictures upload as soon as they're picked, like files in the chat. */
	const photos = new Attachments(() => data.profile.slug);
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
	const describeFiles = new Attachments(() => data.profile.slug);

	/** The template's sentence split into words and chips; settings it leaves out come after. */
	function sentenceParts(template: Template): { parts: Part[]; rest: Setting[] } {
		const source = template.sentence ?? `${template.title}.`;
		const parts: Part[] = [];
		const used: string[] = [];
		let last = 0;
		for (const match of source.matchAll(/\{\{\s*([\w-]+)\s*\}\}/g)) {
			if (match.index > last) parts.push({ kind: 'text', text: source.slice(last, match.index) });
			last = match.index + match[0].length;
			const key = match[1];
			const setting = template.settings.find((s) => s.id === key);
			if (key === 'image' && template.image !== 'none') parts.push({ kind: 'image' });
			else if (setting) parts.push({ kind: 'setting', setting });
			used.push(key);
		}
		if (last < source.length) parts.push({ kind: 'text', text: source.slice(last) });
		if (template.image !== 'none' && !used.includes('image')) {
			parts.push({ kind: 'text', text: ' ' }, { kind: 'image' });
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
		if (pictures.length < files.length) photoNote = 'Only pictures can be used here.';
		if (!pictures.length) return false;
		if (maxPhotos === 1) {
			for (const file of photos.files) photos.remove(file.key);
			photos.add(pictures.slice(-1));
			return true;
		}
		const room = maxPhotos - photos.files.length;
		if (pictures.length > room) photoNote = `At most ${maxPhotos} pictures.`;
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
						setError((result.data?.message as string | undefined) ?? 'That didn’t work.');
					} else if (result.type === 'error') {
						setError(result.error?.message ?? 'Something went wrong.');
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

{#snippet imageChip(template: Template)}
	{@const picture = photos.files[0]}
	<button
		type="button"
		onclick={() => (template.imageSource === 'drawing' ? draw() : photoInput?.click())}
		class={cn(
			chip,
			'inline-flex translate-y-1 items-center gap-1 py-1',
			picture?.status === 'failed' && 'border-destructive'
		)}
		aria-label={picture
			? `Change ${template.imageLabel ?? 'the picture'}`
			: (template.imageLabel ?? 'Add a picture')}
	>
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
	</button>
{/snippet}

{#snippet settingChip(setting: Setting)}
	{#if setting.type === 'select'}
		<!-- The chip shows the choice; the invisible select over it opens the system picker. -->
		<span class={cn(chip, 'relative inline-flex items-center gap-1 whitespace-nowrap')}>
			{optionLabel(setting, values[setting.id] ?? setting.default)}
			<ChevronsUpDownIcon class="size-4 shrink-0 text-muted-foreground" />
			<select
				name={`setting:${setting.id}`}
				bind:value={values[setting.id]}
				aria-label={setting.label}
				class="absolute inset-0 cursor-pointer opacity-0"
			>
				{#each setting.options as option (option.value)}
					<option value={option.value}>{option.label}</option>
				{/each}
			</select>
		</span>
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
	<span class="truncate text-lg font-medium">Images</span>
</PageHeader>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto max-w-4xl space-y-5 px-4 pt-2 pb-8">
		{#if !data.ready}
			<div class="flex items-start gap-3 rounded-2xl bg-muted px-4 py-3 text-sm">
				<CircleAlertIcon class="mt-0.5 size-4 shrink-0 text-warning" />
				<span class="min-w-0">
					<span class="block font-medium">btw can't make pictures yet.</span>
					<span class="block text-muted-foreground">
						{data.problem} An admin sets this up on the computer btw runs on.
					</span>
				</span>
			</div>
		{/if}

		{#if categories.length > 1}
			<div role="tablist" aria-label="Template groups" class="flex gap-1">
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

		<div role="tabpanel" class="grid grid-cols-2 gap-3 sm:grid-cols-3 sm:gap-4 lg:grid-cols-4">
			{#each shown as template (template.id)}
				<button
					type="button"
					onclick={() => open(template)}
					class="group relative block overflow-hidden rounded-3xl text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
				>
					{@render tile(template, 'aspect-[4/5]', 'size-16 stroke-[1.5]')}
					<span
						class="absolute inset-x-0 bottom-0 bg-linear-to-t from-black/55 to-transparent px-3.5 pt-10 pb-3 text-sm font-semibold text-white"
					>
						{template.name}
					</span>
				</button>
			{:else}
				<p class="col-span-full py-10 text-center text-muted-foreground">No templates yet.</p>
			{/each}
		</div>
	</div>
</div>

<form
	bind:this={describeForm}
	method="POST"
	use:enhance={submit(
		(busy) => (describing = busy),
		(message) => (describeError = message),
		describeFiles
	)}
	class="px-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] sm:px-4"
>
	{#each describeFiles.ids as id (id)}
		<input type="hidden" name="upload" value={id} />
	{/each}
	<div class="mx-auto max-w-3xl">
		<Composer
			bind:value={text}
			name="text"
			placeholder="Describe an image"
			busy={describing || !data.ready}
			attachments={describeFiles}
			onsubmit={() => describeForm?.requestSubmit()}
		/>
		{#if describeError}
			<p class="mt-2 text-center text-sm text-destructive">{describeError}</p>
		{:else}
			<p class="mt-2 hidden text-center text-xs text-muted-foreground sm:block">
				btw makes the picture in a new chat, where you can ask for changes.
			</p>
		{/if}
	</div>
</form>

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
						{@render tile(chosen, 'aspect-[5/4] w-full', 'size-24 stroke-[1.25]')}
						<button
							type="button"
							onclick={close}
							class="absolute top-3 right-3 flex size-9 items-center justify-center rounded-full bg-black/45 text-white backdrop-blur hover:bg-black/60"
							aria-label="Close"
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
									{photos.uploading ? 'Uploading…' : 'Starting…'}
								</div>
							{:else if chosen.imageSource === 'drawing'}
								<button
									type="button"
									onclick={draw}
									disabled={!data.ready}
									class={cn(bigButton, 'bg-primary text-primary-foreground')}
								>
									<PenLineIcon class="size-5" />
									Start drawing
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
										Take a photo
									</button>
								{/if}
								<button
									type="button"
									onclick={() => photoInput?.click()}
									disabled={!data.ready}
									class={cn(bigButton, 'bg-primary text-primary-foreground')}
								>
									Choose a photo
								</button>
							{:else}
								<button
									type="button"
									onclick={next}
									disabled={!data.ready}
									class={cn(bigButton, 'bg-primary text-primary-foreground')}
								>
									Try it
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
							aria-label="Close"
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
								{#if part.kind === 'text'}{part.text}{:else if part.kind === 'image'}{@render imageChip(
										chosen
									)}{:else}{@render settingChip(part.setting)}{/if}
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
							placeholder="Add anything else…"
							class="mt-4 w-full resize-none bg-transparent text-xl leading-relaxed outline-none placeholder:text-muted-foreground/60"
						></textarea>
						{#if templateError || photoProblem}
							<p class="text-sm text-destructive">{templateError ?? photoProblem}</p>
						{/if}
					</div>

					<div
						class="flex items-center gap-3 border-t px-4 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]"
					>
						{#each SHAPES.filter((s) => s.value === shape) as current (current.value)}
							<span class={cn(chip, 'relative inline-flex items-center gap-1.5 py-1.5 text-sm')}>
								<current.icon class="size-4" />
								{current.label}
								<ChevronsUpDownIcon class="size-3.5 text-muted-foreground" />
								<select
									name="shape"
									bind:value={shape}
									aria-label="Shape"
									class="absolute inset-0 cursor-pointer opacity-0"
								>
									{#each SHAPES as option (option.value)}
										<option value={option.value}>{option.label}</option>
									{/each}
								</select>
							</span>
						{/each}
						<span class="min-w-0 flex-1 truncate text-xs text-muted-foreground">
							{photos.uploading
								? 'Uploading the picture…'
								: missingPicture
									? `Add ${chosen.imageSource === 'drawing' ? 'a drawing' : 'a photo'} first.`
									: ''}
						</span>
						<button
							type="submit"
							disabled={starting || missingPicture || photos.uploading || !data.ready}
							class="h-10 rounded-full bg-primary px-5 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-85 disabled:opacity-40"
						>
							{starting ? 'Starting…' : 'Generate'}
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
