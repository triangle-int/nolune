<script lang="ts">
	import { setMode, userPrefersMode } from 'mode-watcher';
	import CameraIcon from '@lucide/svelte/icons/camera';
	import { invalidateAll } from '$app/navigation';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import * as Select from '$lib/components/ui/select';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { Switch } from '$lib/components/ui/switch';
	import { Separator } from '$lib/components/ui/separator';
	import { errorMessage } from '$lib/http';
	import { getI18n, isLocale, LANGUAGE_NAMES, LOCALES } from '$lib/i18n';
	import { getPreferences, type LanguagePreference } from '$lib/preferences.svelte';
	import PictureCropper from './PictureCropper.svelte';
	import UserAvatar from './UserAvatar.svelte';

	interface Props {
		open: boolean;
		user: { name: string; email: string; picture: string | null };
	}

	let { open = $bindable(), user }: Props = $props();

	const prefs = getPreferences();
	const { m } = getI18n();
	type Theme = 'system' | 'light' | 'dark';

	/** What's in the box; follows the saved name until someone types. */
	let name = $derived(user.name);
	const edited = $derived(name.trim() !== user.name);
	let savingName = $state(false);
	/** A picture just picked, being cropped. */
	let cropping = $state<ImageBitmap | null>(null);
	let savingPicture = $state(false);
	let problem = $state<string | null>(null);
	let fileInput = $state<HTMLInputElement>();
	let content = $state<HTMLElement | null>(null);

	/** Sends a change to the account, then loads the pages' data again so they show it. */
	async function change(path: string, init: RequestInit): Promise<boolean> {
		problem = null;
		try {
			const res = await fetch(path, init);
			if (!res.ok) {
				problem =
					errorMessage(await res.text(), res.headers.get('content-type')) ??
					m.errors.requestFailed(res.status);
				return false;
			}
		} catch {
			problem = m.errors.somethingWentWrong;
			return false;
		}
		await invalidateAll();
		return true;
	}

	async function saveName(event: SubmitEvent) {
		event.preventDefault();
		savingName = true;
		await change('/api/me', {
			method: 'PATCH',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ name })
		});
		savingName = false;
	}

	async function pick(event: Event & { currentTarget: HTMLInputElement }) {
		const file = event.currentTarget.files?.[0];
		// The same file can be picked again after Cancel.
		event.currentTarget.value = '';
		if (!file) return;
		problem = null;
		try {
			cropping = await createImageBitmap(file);
		} catch {
			problem = m.account.cantOpen;
		}
	}

	function stopCropping() {
		cropping?.close();
		cropping = null;
	}

	async function savePicture(picture: Blob) {
		savingPicture = true;
		const saved = await change('/api/me/picture', {
			method: 'PUT',
			headers: { 'content-type': picture.type },
			body: picture
		});
		savingPicture = false;
		if (saved) stopCropping();
	}

	/** Closing leaves nothing half done for next time. */
	function onOpenChange(isOpen: boolean) {
		if (isOpen) return;
		stopCropping();
		name = user.name;
		problem = null;
	}

	/** Everything on the page, and what the server wrote, switches: so the page loads again. */
	function setLanguage(value: string) {
		const language: LanguagePreference = isLocale(value) ? value : 'auto';
		if (language === prefs.language) return;
		prefs.set({ language });
		location.reload();
	}
</script>

<Dialog.Root bind:open {onOpenChange}>
	<!-- Opens on the dialog, not the name: a phone would bring up its keyboard. -->
	<Dialog.Content
		bind:ref={content}
		class="max-h-[calc(100dvh-2rem)] gap-0 overflow-y-auto p-0 sm:max-w-lg"
		onOpenAutoFocus={(event) => {
			event.preventDefault();
			content?.focus();
		}}
	>
		<Dialog.Header class="px-6 pt-6 pb-2">
			<Dialog.Title class="text-lg">{m.settings.title}</Dialog.Title>
			<Dialog.Description class="sr-only">{m.settings.description}</Dialog.Description>
		</Dialog.Header>

		<section class="space-y-3 px-6 pt-4 pb-5 text-sm">
			{#if cropping}
				<PictureCropper
					image={cropping}
					saving={savingPicture}
					oncancel={stopCropping}
					onsave={savePicture}
				/>
			{:else}
				<div class="flex items-start gap-4">
					<!-- For the pointer: the link beside it does the same from the keyboard. -->
					<button
						type="button"
						tabindex="-1"
						class="group relative shrink-0 rounded-full"
						aria-label={user.picture ? m.account.changePicture : m.account.addPicture}
						onclick={() => fileInput?.click()}
					>
						<UserAvatar name={user.name} picture={user.picture} class="size-16 text-2xl" />
						<span
							class="absolute -right-0.5 -bottom-0.5 flex size-6 items-center justify-center rounded-full bg-secondary text-secondary-foreground ring-2 ring-popover transition-colors group-hover:bg-muted"
						>
							<CameraIcon class="size-3.5" />
						</span>
					</button>
					<div class="min-w-0 flex-1 space-y-1.5">
						<form class="flex gap-2" onsubmit={saveName}>
							<Input
								bind:value={name}
								required
								autocomplete="name"
								aria-label={m.account.name}
								placeholder={m.account.name}
								class="h-9 flex-1 px-3.5"
							/>
							{#if edited}
								<Button type="submit" class="h-9 px-4" disabled={savingName}>{m.common.save}</Button
								>
							{/if}
						</form>
						<p class="truncate px-3.5 text-muted-foreground">{user.email}</p>
						<div class="flex flex-wrap gap-x-4 gap-y-1 px-3.5">
							<button
								type="button"
								class="underline-offset-2 hover:underline"
								onclick={() => fileInput?.click()}
							>
								{user.picture ? m.account.changePicture : m.account.addPicture}
							</button>
							{#if user.picture}
								<button
									type="button"
									class="text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
									onclick={() => change('/api/me/picture', { method: 'DELETE' })}
								>
									{m.account.removePicture}
								</button>
							{/if}
						</div>
					</div>
				</div>
				<p class="text-muted-foreground">{m.account.hint}</p>
			{/if}
			{#if problem}
				<p class="text-destructive" role="alert">{problem}</p>
			{/if}
			<input
				bind:this={fileInput}
				type="file"
				accept="image/*"
				class="hidden"
				tabindex="-1"
				onchange={pick}
			/>
		</section>

		<Separator />
		<div class="divide-y px-6 text-sm">
			<div class="flex items-center justify-between gap-4 py-4">
				<span>{m.settings.theme}</span>
				<ToggleGroup.Root
					type="single"
					variant="outline"
					size="sm"
					value={userPrefersMode.current}
					onValueChange={(value) => value && setMode(value as Theme)}
				>
					<ToggleGroup.Item value="system">{m.settings.system}</ToggleGroup.Item>
					<ToggleGroup.Item value="light">{m.settings.light}</ToggleGroup.Item>
					<ToggleGroup.Item value="dark">{m.settings.dark}</ToggleGroup.Item>
				</ToggleGroup.Root>
			</div>

			<div class="flex items-start justify-between gap-4 py-4">
				<span class="space-y-1">
					<span class="block">{m.settings.language}</span>
					<span class="block text-muted-foreground">{m.settings.languageHint}</span>
				</span>
				<Select.Root type="single" value={prefs.language} onValueChange={setLanguage}>
					<Select.Trigger class="max-w-48 shrink-0" aria-label={m.settings.language}>
						<span class="truncate">
							{prefs.language === 'auto' ? m.settings.languageAuto : LANGUAGE_NAMES[prefs.language]}
						</span>
					</Select.Trigger>
					<Select.Content align="end">
						<Select.Item value="auto" label={m.settings.languageAuto} />
						<Select.Separator />
						{#each LOCALES as locale (locale)}
							<Select.Item value={locale} label={LANGUAGE_NAMES[locale]} lang={locale} />
						{/each}
					</Select.Content>
				</Select.Root>
			</div>

			<label class="flex cursor-pointer items-start justify-between gap-4 py-4">
				<span class="space-y-1">
					<span class="block">{m.settings.technical}</span>
					<span class="block text-muted-foreground">{m.settings.technicalHint}</span>
				</span>
				<Switch
					checked={prefs.technical}
					onCheckedChange={(technical) => prefs.set({ technical })}
				/>
			</label>

			<label class="flex cursor-pointer items-start justify-between gap-4 py-4">
				<span class="space-y-1">
					<span class="block">{m.settings.expandSteps}</span>
					<span class="block text-muted-foreground">{m.settings.expandStepsHint}</span>
				</span>
				<Switch
					checked={prefs.expandSteps}
					onCheckedChange={(expandSteps) => prefs.set({ expandSteps })}
				/>
			</label>

			<label class="flex cursor-pointer items-start justify-between gap-4 py-4">
				<span class="space-y-1">
					<span class="block">{m.settings.sounds}</span>
					<span class="block text-muted-foreground">{m.settings.soundsHint}</span>
				</span>
				<Switch checked={prefs.sounds} onCheckedChange={(sounds) => prefs.set({ sounds })} />
			</label>
		</div>
		<p class="px-6 pb-4 text-xs text-muted-foreground">
			{m.settings.deviceOnly}
		</p>

		<Separator />
		<div class="flex justify-end px-6 py-4 text-sm">
			<form method="POST" action="/logout">
				<button class="rounded-full border px-4 py-1.5 font-medium hover:bg-muted" type="submit">
					{m.settings.logOut}
				</button>
			</form>
		</div>
	</Dialog.Content>
</Dialog.Root>
