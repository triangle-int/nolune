<script lang="ts">
 import { play } from '$lib/sounds.js';
 import { hapticLight, hapticMedium } from '$lib/haptics.js';
 import { fetchChatPreset, fetchModelPresets, updateChatPreset, type ModelPreset } from '$lib/api/client.js';
 import { effectivePresetId } from '$lib/models/presets.js';
 import PromptComposer from './PromptComposer.svelte';
 let { slug, chatId, onSend, onStop, disabled = false, agentRunning = false, mood = 'calm', uploadProgress = null }:
 { slug: string; chatId: string; onSend: (content: string, files?: File[]) => void | boolean | Promise<void | boolean>; onStop: () => void; disabled?: boolean; agentRunning?: boolean; mood?: string; uploadProgress?: { fileIndex: number; fileCount: number; loaded: number; total: number } | null } = $props();
 // Per-conversation model preset (#156): the pin when set, else the Chat slot.
 let presets = $state<ModelPreset[]>([]);
 let presetId = $state<string | null>(null);
 let modelError = $state('');
 let changingPreset = false;
 $effect(() => {
  const currentSlug = slug;
  const currentChat = chatId;
  Promise.all([fetchModelPresets(), fetchChatPreset(currentSlug, currentChat)])
   .then(([models, pin]) => {
    if (currentSlug !== slug || currentChat !== chatId) return;
    presets = models.presets;
    presetId = effectivePresetId(pin.preset, { chat_preset: pin.default_preset, background_preset: models.background_preset }, models.presets);
   })
   .catch(() => {});
 });
 async function changePreset(id: string) {
  if (changingPreset || id === presetId) return;
  changingPreset = true;
  modelError = '';
  try { const pin = await updateChatPreset(slug, chatId, id); presetId = pin.effective_preset; hapticLight(); }
  catch { modelError = 'Could not switch the model for this conversation. Please try again.'; }
  finally { changingPreset = false; }
 }
 async function send(content: string, files?: File[]) { play('message_send'); hapticLight(); return await onSend(content, files); }
</script>
<div class="chat-composer" data-mood={mood}>
 <PromptComposer onSend={send} {onStop} {disabled} {agentRunning} {presets} {presetId} onPresetChange={changePreset} onFileAdd={() => { play('attachment_added'); hapticMedium(); }}>
  {#snippet footer()}
   {#if uploadProgress}
    {@const pct = uploadProgress.total > 0 ? Math.min(100, uploadProgress.loaded / uploadProgress.total * 100) : 0}
    <div class="upload"><progress value={pct} max="100" aria-label="File upload progress"></progress><span role="status">Uploading file {uploadProgress.fileIndex + 1} of {uploadProgress.fileCount} · {pct.toFixed(0)}%</span></div>
   {/if}
   {#if modelError}<p role="alert">{modelError}</p>{/if}
  {/snippet}
 </PromptComposer>
</div>
<style>
 .chat-composer{padding:12px 24px max(16px,env(safe-area-inset-bottom));width:100%;max-width:688px;margin:0 auto;min-width:0;flex-shrink:0}.upload{display:flex;flex-direction:column;gap:6px;padding:8px 0;color:var(--text-secondary);font-size:12px}progress{width:100%;height:4px;accent-color:var(--primary)}p{color:var(--destructive);font-size:13px}@media(max-width:720px){.chat-composer{padding-left:12px;padding-right:12px}}
</style>
