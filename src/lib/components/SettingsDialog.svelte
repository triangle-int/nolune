<script lang="ts">
	import { setMode, userPrefersMode } from 'mode-watcher';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Select from '$lib/components/ui/select';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { Switch } from '$lib/components/ui/switch';
	import { Separator } from '$lib/components/ui/separator';
	import { getI18n, isLocale, LANGUAGE_NAMES, LOCALES } from '$lib/i18n';
	import { getPreferences, type LanguagePreference } from '$lib/preferences.svelte';

	interface Props {
		open: boolean;
		user: { name: string; email: string };
	}

	let { open = $bindable(), user }: Props = $props();

	const prefs = getPreferences();
	const { m } = getI18n();
	type Theme = 'system' | 'light' | 'dark';

	/** Everything on the page, and what the server wrote, switches: so the page loads again. */
	function setLanguage(value: string) {
		const language: LanguagePreference = isLocale(value) ? value : 'auto';
		if (language === prefs.language) return;
		prefs.set({ language });
		location.reload();
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="gap-0 p-0 sm:max-w-lg">
		<Dialog.Header class="px-6 pt-6 pb-2">
			<Dialog.Title class="text-lg">{m.settings.title}</Dialog.Title>
			<Dialog.Description class="sr-only">{m.settings.description}</Dialog.Description>
		</Dialog.Header>

		<div class="divide-y px-6 pb-4 text-sm">
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

		<Separator />
		<div class="flex items-center justify-between gap-4 px-6 py-4 text-sm">
			<span class="min-w-0">
				<span class="block truncate">{user.name}</span>
				<span class="block truncate text-muted-foreground">{user.email}</span>
			</span>
			<form method="POST" action="/logout">
				<button class="rounded-full border px-4 py-1.5 font-medium hover:bg-muted" type="submit">
					{m.settings.logOut}
				</button>
			</form>
		</div>
		<p class="px-6 pb-5 text-xs text-muted-foreground">
			{m.settings.deviceOnly}
		</p>
	</Dialog.Content>
</Dialog.Root>
