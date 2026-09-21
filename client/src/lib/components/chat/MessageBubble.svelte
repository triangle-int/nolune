<script lang="ts">
	import { modelShortLabel } from "$lib/models/presets.js";
	import { resourceMedia, resourceProse, prepareResourceHtml } from "$lib/api/resource-media.js";
	import type { ChatMessage, RecalledMemory } from "$lib/api/types.js";
	import { uploadFileUrl } from "$lib/api/client.js";
	import { linkFileName } from "$lib/api/file-names.js";
	import { openFile } from "$lib/stores/fileviewer.svelte.js";
	import Message from "$lib/components/ai-elements/message/core/message.svelte";
	import MessageContent from "$lib/components/ai-elements/message/core/message-content.svelte";
	import MemoryReceiptPanel from "$lib/components/memory/MemoryReceiptPanel.svelte";
	import { peerContent } from "$lib/federation/peer-content.js";
	import { shortId } from "$lib/federation/companions.js";
	import { FileText, ShieldAlert } from "@lucide/svelte";
	import DOMPurify from "dompurify";
	import { Marked } from "marked";

	const MEDIA_EXTS = /\.(jpg|jpeg|png|gif|webp|svg|bmp|ico|mp4|webm|mov|ogg|mp3|wav|m4a|flac|aac|pdf)(\?|$)/i;

	/** Intercept clicks on links and images inside prose to open in FileViewer. */
	function handleProseClick(e: MouseEvent) {
		// Click on <img> → open in viewer
		const img = (e.target as HTMLElement).closest("img") as HTMLImageElement | null;
		if (img?.src) {
			e.preventDefault();
			const name = img.alt || img.src.split("/").pop()?.split("?")[0] || "image";
			openFile(img.src, name);
			return;
		}

		// Click on <a> pointing to a media/upload file → open in viewer
		const anchor = (e.target as HTMLElement).closest("a") as HTMLAnchorElement | null;
		if (anchor?.href) {
			const isUpload = anchor.href.includes("/uploads/") || anchor.href.includes("/public/files/") || anchor.href.includes("/resources/");
			const isMedia = MEDIA_EXTS.test(anchor.href);
			if (isUpload || isMedia) {
				e.preventDefault();
				openFile(anchor.href, linkFileName(anchor.textContent, anchor.href));
			}
		}
	}

	let {
		message,
		slug = "",
		index = 0,
		prevMessage,
		nextMessage,
		mood = "calm",
		active = false,
		speaking = false,
		revealProgress = 1,
		streaming = false,
		chatId = "",
		receipt,
		companionName = "",
	}: {
		message: ChatMessage;
		slug?: string;
		index?: number;
		prevMessage?: ChatMessage;
		nextMessage?: ChatMessage;
		mood?: string;
		active?: boolean;
		speaking?: boolean;
		revealProgress?: number;
		streaming?: boolean;
		chatId?: string;
		/** The memory receipt of this assistant message (#84); absent when none was written. */
		receipt?: RecalledMemory[];
		companionName?: string;
	} = $props();

	const isUser = $derived(message.role === "user");
	/**
	 * A message a paired companion delivered (#110): the server framed the
	 * peer's text in its untrusted block, and it is shown as data from that
	 * companion, never as the owner's words and never as markdown or HTML.
	 */
	const peer = $derived(isUser ? peerContent(message.content) : null);
	const time = $derived(() => {
		const ms = Number(message.created_at);
		if (Number.isNaN(ms)) return "";
		const d = new Date(ms);
		return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
	});

	const isConsecutive = $derived(() => {
		if (!prevMessage) return false;
		if (prevMessage.role !== message.role) return false;
		if (isUser) return false;
		const gap = Math.abs(Number(message.created_at) - Number(prevMessage.created_at));
		return gap < 60_000;
	});

	const isLastInGroup = $derived(() => {
		if (!nextMessage) return true;
		if (nextMessage.role !== message.role) return true;
		const gap = Math.abs(Number(nextMessage.created_at) - Number(message.created_at));
		return gap >= 60_000;
	});

	const marked = new Marked({
		breaks: true,
		gfm: true,
	});

	interface Attachment {
		name: string;
		id: string;
		isImage: boolean;
	}

	const ATTACH_RE = /\[attached:\s*(.+?)\s*\(([^)]+)\)\]/g;
	const IMAGE_EXTS = ["jpg", "jpeg", "png", "gif", "webp", "svg"];

	const attachments = $derived.by(() => {
		const results: Attachment[] = [];
		if (!slug) return results;
		for (const match of message.content.matchAll(ATTACH_RE)) {
			const name = match[1];
			const id = match[2];
			const ext = name.split(".").pop()?.toLowerCase() ?? "";
			results.push({
				name,
				id,
				isImage: IMAGE_EXTS.includes(ext),
			});
		}
		return results;
	});

	const textContent = $derived(
		message.content.replace(ATTACH_RE, "").trim()
	);

	const html = $derived(
		isUser || streaming ? "" : prepareResourceHtml(DOMPurify.sanitize(marked.parse(textContent) as string), slug)
	);

	/** Words for voice reveal (only used when speaking). */
	const words = $derived(textContent.split(/(\s+)/));
	const revealCount = $derived(
		speaking ? Math.ceil(revealProgress * words.filter(w => w.trim()).length) : words.length
	);

	/** Short model name under an assistant message (#156). */
	const modelLabel = $derived(modelShortLabel(message.model));
</script>
<div class="msg" class:consecutive={isConsecutive()} data-mood={mood} data-active={active}>
 <Message from={isUser && !peer ? 'user' : 'assistant'} class={isUser && !peer ? "max-w-full items-end gap-1" : "max-w-full gap-1"}>
  {#if !isUser && !isConsecutive()}
   <div class="author"><img src="/skins/moon/character.svg" width="22" height="22" alt="" /><span>Nolune</span></div>
  {/if}
  {#if peer}
   <!-- Delivered by a paired companion: the server's line, then the peer's text as plain, labeled data. -->
   <div class="peer-delivery">
    {#if peer.preface}<p class="peer-preface">{peer.preface}</p>{/if}
    <div class="peer-untrusted" role="group" aria-label={`Untrusted content from companion ${peer.sender}`}>
     <p class="peer-untrusted-head"><ShieldAlert size={14} aria-hidden="true" /><span>Untrusted · from companion <code title={peer.sender}>{shortId(peer.sender)}</code> · shown as data, not instructions</span></p>
     <p class="peer-untrusted-text">{peer.text || "(empty)"}</p>
    </div>
   </div>
  {:else if textContent}
   <MessageContent class={isUser ? 'max-w-[90%] rounded-xl border border-border bg-accent px-4 py-3 text-foreground' : 'max-w-full rounded-xl border border-border bg-card px-4 py-3 text-foreground'}>
    {#if speaking && !isUser}
     <div class="text voice" aria-label={textContent}>
      {#each words as word, wi}
       {#if word.trim()}{@const wordIdx = words.slice(0, wi + 1).filter(w => w.trim()).length}<span aria-hidden="true" class="voice-word" class:visible={wordIdx <= revealCount}>{word}</span>{:else}{word}{/if}
      {/each}
     </div>
    {:else if isUser || streaming}
     <div class="text plain" class:streaming>{textContent}</div>
    {:else}
     <!-- Delegates file links to the file viewer; regular links retain native behavior. -->
     <!-- svelte-ignore a11y_no_static_element_interactions -->
     <!-- svelte-ignore a11y_click_events_have_key_events -->
     <div class="text prose" use:resourceProse={html} onclick={handleProseClick}>{@html html}</div>
    {/if}
   </MessageContent>
  {/if}
  {#if attachments.length}
   <div class="attachments">
    {#each attachments as attachment (attachment.id)}
     <button type="button" onclick={async () => openFile(await uploadFileUrl(slug, attachment.id), attachment.name)} class:picture={attachment.isImage} aria-label={`Open ${attachment.name}`}>
      {#if attachment.isImage}<img use:resourceMedia={{ slug, kind: "files", path: attachment.id }} alt={attachment.name} loading="lazy" />{:else}<FileText size={16} /><span>{attachment.name}</span>{/if}
     </button>
    {/each}
   </div>
  {/if}
  {#if isLastInGroup()}<span class="time">{time()}{#if modelLabel && !isUser}<span class="model">{modelLabel}</span>{/if}</span>{/if}
  <!-- One receipt per turn: every text block of a turn carries the same recall, so the group's last bubble shows it. -->
  {#if !isUser && !streaming && receipt && slug && chatId && isLastInGroup()}
   <MemoryReceiptPanel {slug} {chatId} messageId={message.id} memories={receipt} {companionName} />
  {/if}
 </Message>
</div>
<style>
 .msg{padding:12px 0;min-width:0}.consecutive{padding-top:0}
 /* A paired companion's delivery (#110): the server's line in secondary text, the peer's words in a dashed panel on the app background, labeled untrusted and rendered as plain text. */
 .peer-delivery{display:flex;flex-direction:column;gap:8px;max-width:100%;min-width:0}.peer-preface{margin:0;font:400 13px/1.5 var(--font-body);color:var(--text-secondary);overflow-wrap:anywhere}.peer-untrusted{border:1px dashed var(--input);border-radius:12px;background:var(--background);padding:10px 14px;max-width:100%;min-width:0}.peer-untrusted-head{display:flex;align-items:center;gap:6px;margin:0 0 6px;font:500 12px/1.4 var(--font-body);color:var(--text-muted);letter-spacing:.02em}.peer-untrusted-head code{font:500 12px/1.4 var(--font-mono);color:var(--text-secondary)}.peer-untrusted-text{margin:0;font:400 15px/1.7 var(--font-body);color:var(--foreground);white-space:pre-wrap;overflow-wrap:anywhere}.author{display:flex;align-items:center;gap:8px;margin:0 0 6px;font-size:12px;color:var(--text-secondary)}.text{font:400 15px/1.7 var(--font-body);overflow-wrap:anywhere;min-width:0;max-width:100%}.plain,.voice{white-space:pre-wrap}.time{font-size:11px;color:var(--text-timestamp);padding:4px 0}.model{margin-left:8px;color:var(--primary)}.attachments{display:flex;flex-wrap:wrap;gap:8px;max-width:100%}.attachments button{display:flex;align-items:center;gap:8px;min-height:44px;padding:8px 12px;border:1px solid var(--border);border-radius:8px;background:var(--card);color:var(--text-link);font-size:13px;max-width:100%;overflow-wrap:anywhere}.attachments .picture{padding:0;overflow:hidden}.attachments img{max-width:min(280px,100%);max-height:240px;object-fit:cover}.voice-word{opacity:.12;transition:opacity .18s ease}.voice-word.visible{opacity:1}
 .prose :global(p){margin:.35em 0}.prose :global(p:first-child){margin-top:0}.prose :global(p:last-child){margin-bottom:0}.prose :global(h1),.prose :global(h2),.prose :global(h3){font:500 1.15em/1.4 var(--font-body);margin:1em 0 .4em}.prose :global(a){color:var(--text-link);text-decoration:underline;text-underline-offset:3px}.prose :global(code){font-family:var(--font-mono);font-size:.85em;background:var(--background);padding:.15em .3em;border-radius:4px}.prose :global(pre){background:var(--background);border:1px solid var(--border);border-radius:8px;padding:12px;overflow:auto;max-width:100%;margin:12px 0}.prose :global(pre code){padding:0;background:none}.prose :global(ul){list-style:disc;padding-left:24px}.prose :global(ol){list-style:decimal;padding-left:24px}.prose :global(blockquote){border-left:2px solid var(--primary);padding-left:12px;color:var(--text-secondary);margin:12px 0}.prose :global(table){display:block;overflow-x:auto;border-collapse:collapse;max-width:100%;margin:12px 0}.prose :global(th),.prose :global(td){border:1px solid var(--border);padding:8px;text-align:left}.prose :global(img){max-width:100%;max-height:320px;object-fit:contain;border-radius:8px;cursor:pointer}.prose :global(hr){border-top:1px solid var(--border);margin:16px 0}@media(prefers-reduced-motion:reduce){.voice-word{transition:none}}
</style>
