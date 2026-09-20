<script lang="ts">
 // The composer's computer selector (#80). Presentational: the options and
 // the summary come from lib/computers/target.js over the same rows the
 // Computers tab shows, so the /design-system sample and the live composer
 // cannot drift. The trigger reads what the desktop tools will act on; the
 // menu lists the server home, then every desktop with its state word.
 import * as Select from '$lib/components/ui/select/index.js';
 import Monitor from '@lucide/svelte/icons/monitor';
 import { NO_TARGET, type TargetOption, type TargetSummary } from '$lib/computers/target.js';
 let { options, value, summary, onChange, disabled = false }:
 { options: TargetOption[]; value: string; summary: TargetSummary; onChange: (value: string) => void; disabled?: boolean } = $props();
 const id = $props.id();
 /** The open choice, listed first so a chosen computer can be let go of again. */
 const openLabel = 'Ask me';
 const items = $derived([{ value: NO_TARGET, label: openLabel }, ...options.map(o => ({ value: o.value, label: o.label }))]);
 const title = $derived(value === NO_TARGET ? `${openLabel}: ${summary.detail}` : `${summary.name} · ${summary.detail}`);
</script>
<label class="sr-only" for={id}>Computer for this conversation</label>
<Select.Root type="single" {items} {disabled}
 bind:value={() => value, next => { if (next !== undefined && next !== value) onChange(next); }}>
 <Select.Trigger {id} aria-label="Computer for this conversation" {title} data-status={summary.status} class="picker min-w-0 flex-[3] basis-0 gap-2 border-border bg-card px-3 text-[13px] text-secondary-foreground shadow-none data-[size=default]:h-11 dark:bg-card dark:hover:bg-accent md:max-w-48">
  <Monitor size={15} aria-hidden="true" class="shrink-0" />
  <span class="min-w-0 flex-1 truncate text-left">{summary.name}</span>
 </Select.Trigger>
 <Select.Content side="top" align="start" sideOffset={8} class="min-w-56 border border-border p-1 shadow-lg">
  <Select.Item value={NO_TARGET} label={openLabel} class="min-h-11 pl-3 pr-9">
   <span class="flex flex-col"><span>{openLabel}</span><span class="text-[11px] text-muted-foreground">The only connected desktop; asked when there are several</span></span>
  </Select.Item>
  {#each options as option (option.value)}
   <Select.Item value={option.value} label={option.label} class="min-h-11 pl-3 pr-9">
    <span class="flex min-w-0 flex-col">
     <span class="truncate">{option.label}</span>
     <span class="text-[11px] text-muted-foreground" data-status={option.status}>{option.detail}</span>
    </span>
   </Select.Item>
  {/each}
 </Select.Content>
</Select.Root>
<style>
 /* One state word per row, colored beside its text like the Computers tab. */
 :global([data-slot="select-item"] [data-status="online"]){color:var(--primary)}
 :global([data-slot="select-item"] [data-status="unhealthy"]),:global([data-slot="select-item"] [data-status="restricted"]){color:var(--destructive)}
 /* The element name outranks the trigger's placeholder utility, which also matches the open choice. */
 :global(button.picker[data-status="ambiguous"]){border-color:var(--primary);color:var(--primary)}
 :global(button.picker[data-status="unhealthy"]),:global(button.picker[data-status="restricted"]){color:var(--destructive)}
</style>
