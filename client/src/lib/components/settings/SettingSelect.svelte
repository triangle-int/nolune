<script lang="ts">
	import * as Select from "$lib/components/ui/select/index.js";
	import { cn } from "$lib/utils.js";

	// The shared shadcn Select for a Settings row: 44px trigger and options on
	// the popover tokens, one list of string values in, one string value out.
	type Option = { value: string; label: string };

	let {
		value = $bindable(""),
		options,
		onValueChange,
		disabled = false,
		placeholder = "",
		id,
		class: className,
		"aria-label": ariaLabel,
		"aria-labelledby": ariaLabelledby,
	}: {
		value?: string;
		options: Option[];
		onValueChange?: (value: string) => void;
		disabled?: boolean;
		placeholder?: string;
		id?: string;
		class?: string;
		"aria-label"?: string;
		"aria-labelledby"?: string;
	} = $props();

	const selected = $derived(options.find((option) => option.value === value));

	// Controlled: with onValueChange the parent owns the value (a failed save
	// reverts it); without, the pick is written back through bind:value.
	function pick(next: string) {
		if (onValueChange) onValueChange(next);
		else value = next;
	}
</script>

<Select.Root type="single" bind:value={() => value, pick} {disabled}>
	<Select.Trigger {id} aria-label={ariaLabel} aria-labelledby={ariaLabelledby} class={cn("h-11 w-full min-w-0 bg-card text-foreground dark:bg-card", className)}>
		<span data-slot="select-value" class:text-muted-foreground={!selected}>{selected?.label ?? placeholder}</span>
	</Select.Trigger>
	<Select.Content class="max-h-80">
		{#each options as option (option.value)}
			<Select.Item value={option.value} label={option.label} class="min-h-11">{option.label}</Select.Item>
		{/each}
	</Select.Content>
</Select.Root>
