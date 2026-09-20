<script lang="ts">
 import PromptInput from '$lib/components/ai-elements/prompt-input/core/root.svelte';
 import PromptTextarea from '$lib/components/ai-elements/prompt-input/controls/textarea.svelte';
 import PromptSubmit from '$lib/components/ai-elements/prompt-input/controls/submit.svelte';
 import PromptToolbar from '$lib/components/ai-elements/prompt-input/layout/toolbar.svelte';
 import type { Message, PromptInputAttachment } from '$lib/components/ai-elements/prompt-input/context/types.js';
 import PromptAttachments from './PromptAttachments.svelte';
 import PromptAttachButton from './PromptAttachButton.svelte';
 import type { Snippet } from 'svelte';
 import * as Select from '$lib/components/ui/select/index.js';
 /** A preset the picker can offer (#156): the user's own name for a model, and what the model cannot do (#28) when known. */
 type PresetOption = { id: string; name: string; model: string; warnings?: { chip: string; detail: string }[] };
 let { onSend, onStop, disabled = false, agentRunning = false, presets = [], presetId = null, onPresetChange, footer, onFileAdd }:
 { onSend: (text: string, files?: File[]) => void | boolean | Promise<void | boolean>; onStop: () => void; disabled?: boolean; agentRunning?: boolean; presets?: PresetOption[]; presetId?: string | null; onPresetChange?: (id: string) => void; footer?: Snippet; onFileAdd?: () => void } = $props();
 const modelId = $props.id();
 const presetItems = $derived(presets.map(p => ({ value: p.id, label: p.name })));
 const currentPreset = $derived(presets.find(p => p.id === presetId));
 let value = $state('');
 let attachments = $state<PromptInputAttachment[]>([]);
 let submitting = $state(false);
 let error = $state('');
 let textarea: HTMLTextAreaElement | null = $state(null);
 const busy = $derived(disabled || submitting);
 async function submit(message: Message) {
  if (busy || agentRunning) return false;
  submitting = true;
  error = '';
  try {
   const result = await onSend(message.text.trim(), message.attachments.length ? message.attachments.map(a => a.file) : undefined);
   if (result === false) { error = 'Message not sent. Your draft and files are still here.'; return false; }
  } catch {
   error = 'Message not sent. Your draft and files are still here.';
   return false;
  } finally { submitting = false; }
 }
 $effect(() => {
  if (!busy && textarea && document.activeElement === document.body) textarea.focus();
 });
</script>
<div class="composer">
 <PromptInput class="border-input bg-card shadow-none focus-within:border-ring" bind:attachments multiple disabled={busy || agentRunning} onSubmit={submit} {onFileAdd} onError={e => error = e.message}>
  <PromptAttachments disabled={busy} />
  <PromptTextarea bind:value bind:ref={textarea} aria-label="Message Nolune" placeholder="What’s on your mind?" disabled={submitting || (disabled && !agentRunning)} class="min-h-20 p-4 text-base md:text-base" />
  <PromptToolbar class="gap-2 px-2 pb-2">
   <div class="flex min-w-0 items-center gap-1">
    <PromptAttachButton disabled={busy || agentRunning} />
    {#if presets.length > 0 && onPresetChange}
     <label class="sr-only" for={modelId}>Model preset for this conversation</label>
     <Select.Root type="single" items={presetItems} disabled={busy}
      bind:value={() => presetId ?? '', next => { if (next) onPresetChange?.(next); }}>
      <Select.Trigger id={modelId} aria-label="Model preset for this conversation" title={currentPreset?.model} class="w-36 gap-3 border-border bg-card px-3 text-[13px] text-secondary-foreground shadow-none data-[size=default]:h-11 dark:bg-card dark:hover:bg-accent">
       <span data-slot="select-value" class="truncate">{currentPreset?.name ?? "Model"}</span>
      </Select.Trigger>
      <Select.Content side="top" align="start" sideOffset={8} class="min-w-48 border border-border p-1 shadow-lg">
       {#each presets as preset (preset.id)}
        <Select.Item value={preset.id} label={preset.name} class="min-h-11 pl-3 pr-9"><span class="flex flex-col"><span>{preset.name}</span><span class="text-[11px] text-muted-foreground">{preset.model}{#if preset.warnings?.length}{' — '}{preset.warnings.map(w => w.chip).join(', ')}{/if}</span></span></Select.Item>
       {/each}
      </Select.Content>
     </Select.Root>
    {:else}<span class="hint">Enter to send</span>{/if}
   </div>
   <PromptSubmit class="size-11 shrink-0 bg-primary text-primary-foreground hover:bg-primary/90" status={agentRunning ? 'streaming' : submitting || disabled ? 'submitted' : 'ready'} {onStop} disabled={!agentRunning && (busy || (!value.trim() && !attachments.length))} />
  </PromptToolbar>
 </PromptInput>
 {#if error}<p role="alert" class="error">{error}</p>{/if}
 {#each currentPreset?.warnings ?? [] as warning (warning.chip)}<p role="status" class="limit">{warning.detail}</p>{/each}
 {@render footer?.()}
</div>
<style>
 /* The form provides the focus border; an inner outline would divide the composer. */
 .composer :global(textarea:focus-visible){outline:none}

 .composer{width:100%;min-width:0}.hint{font-size:12px;color:var(--text-muted)}.error{font-size:13px;color:var(--destructive);padding:8px 4px;margin:0}.limit{font-size:13px;line-height:1.5;color:var(--text-secondary);padding:8px 4px 0;margin:0}
</style>
