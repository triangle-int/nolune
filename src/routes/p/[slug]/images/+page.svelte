<script lang="ts">
	import { onDestroy } from 'svelte';
	import { enhance } from '$app/forms';
	import type { SubmitFunction } from '@sveltejs/kit';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import ImageIcon from '@lucide/svelte/icons/image';
	import ImagePlusIcon from '@lucide/svelte/icons/image-plus';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RectangleHorizontalIcon from '@lucide/svelte/icons/rectangle-horizontal';
	import RectangleVerticalIcon from '@lucide/svelte/icons/rectangle-vertical';
	import ScanIcon from '@lucide/svelte/icons/scan';
	import SquareIcon from '@lucide/svelte/icons/square';
	import WandSparklesIcon from '@lucide/svelte/icons/wand-sparkles';
	import XIcon from '@lucide/svelte/icons/x';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Composer from '$lib/components/chat/Composer.svelte';
	import StepIcon from '$lib/components/chat/StepIcon.svelte';
	import { Attachments } from '$lib/uploads.svelte';
	import { cn } from '$lib/utils';

	let { data } = $props();

	type Template = (typeof data.templates)[number];

	const SHAPES = [
		{ value: 'square', label: 'Square', icon: SquareIcon },
		{ value: 'portrait', label: 'Portrait', icon: RectangleVerticalIcon },
		{ value: 'landscape', label: 'Landscape', icon: RectangleHorizontalIcon },
		{ value: 'auto', label: 'Auto', icon: ScanIcon }
	] as const;

	const categories = $derived([...new Set(data.templates.map((t) => t.category))]);
	let tab = $state<string | null>(null);
	const activeTab = $derived(tab && categories.includes(tab) ? tab : categories[0]);
	const shown = $derived(data.templates.filter((t) => t.category === activeTab));

	/** The template whose dialog is open. */
	let chosen = $state<Template | null>(null);
	let starting = $state(false);
	/** Errors from the server stay with the form they came from. */
	let templateError = $state<string | null>(null);
	/** The template's pictures upload as soon as they're picked, like files in the chat. */
	const photos = new Attachments(() => data.profile.slug);
	let photoInput = $state<HTMLInputElement>();
	let photoNote = $state<string | null>(null);
	let dragging = $state(false);
	const maxPhotos = $derived(Math.max(1, chosen?.maxImages ?? 1));
	const photoProblem = $derived(
		photoNote ?? photos.files.find((f) => f.status === 'failed')?.error ?? null
	);
	const missingPicture = $derived(chosen?.image === 'required' && photos.ids.length === 0);

	let text = $state('');
	let describing = $state(false);
	let describeError = $state<string | null>(null);
	let describeForm = $state<HTMLFormElement>();
	const describeFiles = new Attachments(() => data.profile.slug);

	function isPicture(file: File): boolean {
		// HEIC from a Mac often has no type; the server checks the content anyway.
		return file.type.startsWith('image/') || /\.(heic|heif)$/i.test(file.name);
	}

	/** Adds pictures to the template; with room for one, a new one replaces the old. */
	function addPhotos(list: FileList | null | undefined) {
		photoNote = null;
		const files = [...(list ?? [])];
		const pictures = files.filter(isPicture);
		if (pictures.length < files.length) photoNote = 'Only pictures can be used here.';
		if (!pictures.length) return;
		if (maxPhotos === 1) {
			for (const file of photos.files) photos.remove(file.key);
			photos.add(pictures.slice(-1));
			return;
		}
		const room = maxPhotos - photos.files.length;
		if (pictures.length > room) photoNote = `At most ${maxPhotos} pictures.`;
		photos.add(pictures.slice(0, Math.max(0, room)));
	}

	/** Takes the pictures off the server too: they were never sent. */
	function dropPhotos() {
		for (const file of photos.files) photos.remove(file.key);
		photoNote = null;
	}

	function choose(template: Template) {
		dropPhotos();
		templateError = null;
		chosen = template;
	}

	function close() {
		if (starting) return;
		dropPhotos();
		chosen = null;
	}

	function coverUrl(template: Template): string | null {
		if (template.cover === null) return null;
		const slug = encodeURIComponent(data.profile.slug);
		return `/api/p/${slug}/templates/${encodeURIComponent(template.id)}/cover?v=${template.cover}`;
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
					onclick={() => choose(template)}
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

<Dialog.Root open={chosen !== null} onOpenChange={(open) => !open && close()}>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] gap-5 overflow-y-auto sm:max-w-lg">
		{#if chosen}
			{#key chosen.id}
				<div class="flex items-center gap-4 pr-8">
					{@render tile(chosen, 'size-16 shrink-0 rounded-2xl', 'size-8 stroke-[1.5]')}
					<div class="min-w-0 space-y-1">
						<Dialog.Title class="text-lg">{chosen.name}</Dialog.Title>
						{#if chosen.description}
							<Dialog.Description>{chosen.description}</Dialog.Description>
						{/if}
					</div>
				</div>

				<form
					method="POST"
					use:enhance={submit(
						(busy) => (starting = busy),
						(message) => (templateError = message),
						photos
					)}
					class="grid gap-5"
				>
					<input type="hidden" name="template" value={chosen.id} />
					{#each photos.ids as id (id)}
						<input type="hidden" name="upload" value={id} />
					{/each}

					{#if chosen.image !== 'none'}
						<div class="grid gap-2">
							<span class="text-sm font-medium">
								{chosen.imageLabel ?? 'Picture'}
								{#if chosen.image === 'optional'}
									<span class="font-normal text-muted-foreground">(optional)</span>
								{/if}
							</span>
							<input
								bind:this={photoInput}
								type="file"
								accept="image/*"
								multiple={maxPhotos > 1}
								class="hidden"
								onchange={(event) => {
									addPhotos(event.currentTarget.files);
									event.currentTarget.value = '';
								}}
							/>
							{#if photos.files.length}
								<div class="flex flex-wrap gap-2">
									{#each photos.files as file (file.key)}
										<div class="relative size-28 shrink-0 overflow-hidden rounded-2xl bg-muted">
											{#if file.preview}
												<img src={file.preview} alt={file.name} class="size-full object-cover" />
											{:else}
												<!-- A format the browser can't show, like HEIC in Chrome. -->
												<span
													class="flex size-full items-center justify-center p-2 text-center text-xs break-all text-muted-foreground"
													>{file.name}</span
												>
											{/if}
											{#if file.status === 'uploading'}
												<span
													class="absolute inset-0 flex items-center justify-center bg-black/35 text-white"
												>
													<LoaderCircleIcon class="size-6 animate-spin" />
													<span class="sr-only">Uploading</span>
												</span>
											{:else if file.status === 'failed'}
												<span
													class="absolute inset-0 flex items-center justify-center bg-destructive/70 p-2 text-center text-xs text-white"
												>
													{file.error ?? 'Upload failed'}
												</span>
											{/if}
											<button
												type="button"
												onclick={() => photos.remove(file.key)}
												class="absolute top-1 right-1 flex size-6 items-center justify-center rounded-full bg-black/60 text-white hover:bg-black/80"
												aria-label={`Remove ${file.name}`}
											>
												<XIcon class="size-3.5" />
											</button>
										</div>
									{/each}
									{#if maxPhotos > 1 && photos.files.length < maxPhotos}
										<button
											type="button"
											onclick={() => photoInput?.click()}
											class="flex size-28 items-center justify-center rounded-2xl border border-dashed text-muted-foreground hover:bg-muted"
											aria-label="Add another picture"
										>
											<PlusIcon class="size-6" />
										</button>
									{/if}
								</div>
							{:else}
								<button
									type="button"
									onclick={() => photoInput?.click()}
									ondragover={(event) => {
										if (!event.dataTransfer?.types.includes('Files')) return;
										event.preventDefault();
										dragging = true;
									}}
									ondragleave={() => (dragging = false)}
									ondrop={(event) => {
										event.preventDefault();
										dragging = false;
										addPhotos(event.dataTransfer?.files);
									}}
									class={cn(
										'flex h-32 flex-col items-center justify-center gap-1.5 rounded-2xl border border-dashed text-sm text-muted-foreground transition-colors hover:bg-muted',
										dragging && 'border-primary bg-muted'
									)}
								>
									<ImagePlusIcon class="size-6" />
									<span class="font-medium text-foreground">Choose a photo</span>
									<span class="max-sm:hidden">or drop it here</span>
								</button>
							{/if}
							{#if photoProblem}
								<p class="text-sm text-destructive">{photoProblem}</p>
							{/if}
						</div>
					{/if}

					{#each chosen.settings as setting (setting.id)}
						{#if setting.type === 'select'}
							<fieldset class="grid gap-2">
								<legend class="mb-2 text-sm font-medium">{setting.label}</legend>
								<div class="flex flex-wrap gap-2">
									{#each setting.options as option (option.value)}
										<label
											class="cursor-pointer rounded-full border px-3.5 py-1.5 text-sm transition-colors select-none hover:bg-muted has-checked:border-primary has-checked:bg-primary has-checked:text-primary-foreground has-focus-visible:ring-2 has-focus-visible:ring-ring"
										>
											<input
												type="radio"
												name={`setting:${setting.id}`}
												value={option.value}
												checked={option.value === setting.default}
												class="sr-only"
											/>
											{option.label}
										</label>
									{/each}
								</div>
							</fieldset>
						{:else}
							<label class="grid gap-2">
								<span class="text-sm font-medium">
									{setting.label}
									{#if !setting.required}
										<span class="font-normal text-muted-foreground">(optional)</span>
									{/if}
								</span>
								<Input
									name={`setting:${setting.id}`}
									value={setting.default}
									placeholder={setting.placeholder ?? ''}
									required={setting.required}
									class="h-10 rounded-full px-4"
								/>
							</label>
						{/if}
					{/each}

					<fieldset class="grid gap-2">
						<legend class="mb-2 text-sm font-medium">Shape</legend>
						<div class="flex flex-wrap gap-2">
							{#each SHAPES as shape (shape.value)}
								<label
									class="flex cursor-pointer items-center gap-1.5 rounded-full border px-3.5 py-1.5 text-sm transition-colors select-none hover:bg-muted has-checked:border-primary has-checked:bg-primary has-checked:text-primary-foreground has-focus-visible:ring-2 has-focus-visible:ring-ring"
								>
									<input
										type="radio"
										name="shape"
										value={shape.value}
										checked={shape.value === chosen.size}
										class="sr-only"
									/>
									<shape.icon class="size-4" />
									{shape.label}
								</label>
							{/each}
						</div>
					</fieldset>

					<label class="grid gap-2">
						<span class="text-sm font-medium">
							Anything else? <span class="font-normal text-muted-foreground">(optional)</span>
						</span>
						<Textarea
							name="extra"
							rows={2}
							placeholder="Like “make it pink” or “add our dog”"
							class="min-h-16 rounded-2xl px-4 py-2.5"
						/>
					</label>

					{#if templateError}
						<p class="text-sm text-destructive">{templateError}</p>
					{/if}

					<div class="grid gap-2">
						<Button
							type="submit"
							disabled={starting || missingPicture || photos.uploading || !data.ready}
							class="h-11 rounded-full text-base"
						>
							<WandSparklesIcon />
							{starting ? 'Starting…' : 'Generate'}
						</Button>
						<p class="text-center text-xs text-muted-foreground">
							{photos.uploading
								? 'Uploading the photo…'
								: missingPicture
									? 'Choose a photo first.'
									: 'btw makes it in a new chat, where you can ask for changes.'}
						</p>
					</div>
				</form>
			{/key}
		{/if}
	</Dialog.Content>
</Dialog.Root>
