<script lang="ts">
 import { play } from '$lib/sounds.js';
 import { hapticLight, hapticMedium } from '$lib/haptics.js';
 import { fetchChatPreset, fetchMachines, fetchModelPresets, updateChatPreset, type MachineInfo, type ModelPreset } from '$lib/api/client.js';
 import type { ServerEvent } from '$lib/api/types.js';
 import { effectivePresetId } from '$lib/models/presets.js';
 import { applyMachineEvent, buildSpaces, homeSpace, reconcileListing } from '$lib/computers/spaces.js';
 import { NO_TARGET, normalizeTarget, requestTarget, runningLabel, targetOptions, targetStorageKey, targetSummary } from '$lib/computers/target.js';
 import { getCompanion } from '$lib/stores/companion.svelte.js';
 import { getWebSocket } from '$lib/stores/websocket.svelte.js';
 import PromptComposer from './PromptComposer.svelte';
 /** The computer a message acts on (#80): what the request carries and how the chat bar names it. */
 export type ChatTarget = { machineId: string | null; label: string };
 let { slug, chatId, onSend, onStop, onTargetChange, disabled = false, agentRunning = false, mood = 'calm', uploadProgress = null }:
 { slug: string; chatId: string; onSend: (content: string, files?: File[]) => void | boolean | Promise<void | boolean>; onStop: () => void; onTargetChange?: (target: ChatTarget) => void; disabled?: boolean; agentRunning?: boolean; mood?: string; uploadProgress?: { fileIndex: number; fileCount: number; loaded: number; total: number } | null } = $props();
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
 // Per-conversation computer (#80): the same rows as the Computers tab, kept
 // fresh from machine events and a poll (routine heartbeats are silent on the
 // socket), so the state word beside each computer is the one the tab shows.
 // The choice is a per-viewer convenience remembered in this browser; the
 // server never defaults a computer from it. Until the first listing for a
 // companion arrives the rows are unknown (`null`), and a remembered
 // computer is sent as it is rather than read as forgotten; the listing is
 // kept across the companion's conversations. A poll that was in flight when
 // an event landed never reverts that row: the response is folded in around
 // what changed since it was requested, as the Computers tab does.
 const POLL_SECS = 15;
 const ws = getWebSocket();
 const companion = getCompanion();
 let machines = $state<MachineInfo[] | null>(null);
 let now = $state(Math.floor(Date.now() / 1000));
 let targetId = $state<string>(NO_TARGET);
 let epoch = 0;
 /** The companion the rows belong to; another one starts from no listing. */
 let listedSlug = '';
 /** Bumped on every event applied to a row; a fetch captures it when it starts. */
 let generation = 0;
 /** The generation at which each row last changed from an event. */
 const touched = new Map<string, number>();
 async function loadMachines() {
  const started = epoch;
  const startedAt = generation;
  try {
   const listing = (await fetchMachines(slug)).machines;
   if (started !== epoch) return;
   const changedSince = [...touched].filter(([, at]) => at > startedAt).map(([id]) => id);
   machines = machines === null ? listing : reconcileListing(machines, listing, changedSince);
   now = Math.floor(Date.now() / 1000);
  } catch { /* the last listing stays (or none has arrived); the composer never blocks on it */ }
 }
 function remembered(key: string): string {
  try { return localStorage.getItem(key) ?? NO_TARGET; } catch { return NO_TARGET; }
 }
 function remember(key: string, value: string) {
  try { if (value === NO_TARGET) localStorage.removeItem(key); else localStorage.setItem(key, value); } catch { /* per-viewer convenience only */ }
 }
 $effect(() => {
  const key = targetStorageKey(slug, chatId);
  epoch += 1;
  if (listedSlug !== slug) { listedSlug = slug; machines = null; touched.clear(); }
  targetId = remembered(key);
  loadMachines();
  const unsub = ws.subscribe((event: ServerEvent) => {
   if ((event.type === 'machine_updated' || event.type === 'machine_forgotten') && event.instance_slug === slug) {
    // One computer's event is not a listing: before the first one arrives
    // it is asked for again rather than mistaken for the whole list.
    if (machines === null) { loadMachines(); return; }
    generation += 1;
    touched.set(event.type === 'machine_updated' ? event.machine.machine_id : event.machine_id, generation);
    machines = applyMachineEvent(machines, event);
    now = Math.floor(Date.now() / 1000);
   }
  });
  const poll = setInterval(loadMachines, POLL_SECS * 1000);
  const onVisible = () => { if (document.visibilityState === 'visible') loadMachines(); };
  document.addEventListener('visibilitychange', onVisible);
  return () => { epoch += 1; unsub(); clearInterval(poll); document.removeEventListener('visibilitychange', onVisible); };
 });
 const companionName = $derived(companion.context?.companion_name ?? '');
 const spaces = $derived(machines === null ? null : buildSpaces(machines, now, homeSpace({ connected: ws.connected, companionName, nowSeconds: now }), companionName));
 const targets = $derived(targetOptions(spaces));
 // A remembered computer that was forgotten since reads like no choice; one
 // the listing has not confirmed or denied yet is kept as it is.
 const effectiveTarget = $derived(normalizeTarget(targetId, spaces));
 const summary = $derived(targetSummary(effectiveTarget, spaces));
 $effect(() => {
  onTargetChange?.({ machineId: requestTarget(effectiveTarget), label: runningLabel(effectiveTarget, spaces) });
 });
 function changeTarget(id: string) {
  targetId = id;
  remember(targetStorageKey(slug, chatId), id);
  hapticLight();
 }
 async function send(content: string, files?: File[]) { play('message_send'); hapticLight(); return await onSend(content, files); }
</script>
<div class="chat-composer" data-mood={mood}>
 <PromptComposer onSend={send} {onStop} {disabled} {agentRunning} {presets} {presetId} onPresetChange={changePreset} {targets} targetId={effectiveTarget} targetSummary={summary} onTargetChange={changeTarget} onFileAdd={() => { play('attachment_added'); hapticMedium(); }}>
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
