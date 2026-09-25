<script lang="ts">
	import { setMode, userPrefersMode } from 'mode-watcher';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { Switch } from '$lib/components/ui/switch';
	import { Separator } from '$lib/components/ui/separator';
	import { getPreferences } from '$lib/preferences.svelte';

	interface Props {
		open: boolean;
		user: { name: string; email: string };
	}

	let { open = $bindable(), user }: Props = $props();

	const prefs = getPreferences();
	type Theme = 'system' | 'light' | 'dark';
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="gap-0 p-0 sm:max-w-lg">
		<Dialog.Header class="px-6 pt-6 pb-2">
			<Dialog.Title class="text-lg">Settings</Dialog.Title>
			<Dialog.Description class="sr-only">How btw looks on this device.</Dialog.Description>
		</Dialog.Header>

		<div class="divide-y px-6 pb-4 text-sm">
			<div class="flex items-center justify-between gap-4 py-4">
				<span>Theme</span>
				<ToggleGroup.Root
					type="single"
					variant="outline"
					size="sm"
					value={userPrefersMode.current}
					onValueChange={(value) => value && setMode(value as Theme)}
				>
					<ToggleGroup.Item value="system">System</ToggleGroup.Item>
					<ToggleGroup.Item value="light">Light</ToggleGroup.Item>
					<ToggleGroup.Item value="dark">Dark</ToggleGroup.Item>
				</ToggleGroup.Root>
			</div>

			<label class="flex cursor-pointer items-start justify-between gap-4 py-4">
				<span class="space-y-1">
					<span class="block">Show technical details</span>
					<span class="block text-muted-foreground">
						Show the exact commands btw runs, token usage and prompt caching.
					</span>
				</span>
				<Switch
					checked={prefs.technical}
					onCheckedChange={(technical) => prefs.set({ technical })}
				/>
			</label>

			<label class="flex cursor-pointer items-start justify-between gap-4 py-4">
				<span class="space-y-1">
					<span class="block">Always show steps</span>
					<span class="block text-muted-foreground">
						Open the list of what btw did under each reply, instead of keeping it folded.
					</span>
				</span>
				<Switch
					checked={prefs.expandSteps}
					onCheckedChange={(expandSteps) => prefs.set({ expandSteps })}
				/>
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
					Log out
				</button>
			</form>
		</div>
		<p class="px-6 pb-5 text-xs text-muted-foreground">
			These settings are saved on this device only.
		</p>
	</Dialog.Content>
</Dialog.Root>
