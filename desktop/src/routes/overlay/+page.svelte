<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Moon from "$lib/components/Moon.svelte";
  import { overlayEvent, overlayInitialState } from "$lib/overlay-state.js";
  import { companionStatusText, reduceCompanion, type CompanionState } from "../../../../client/src/lib/companion/state.js";
  import { companionExpression } from "../../../../client/src/lib/companion/expressions.js";

  type Flash = { id: number; text: string; icon: string };

  let visible = $state(false);
  let actionQueue = $state<Flash[]>([]);
  let idCounter = 0;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;

  // What the companion is doing on this computer, through the same reducer
  // as the client (#86): working while actions arrive, over when the relay
  // connection ends. The face and motion follow the shared expression table;
  // under prefers-reduced-motion the face stays and the moon holds still.
  let companion = $state.raw<CompanionState>(overlayInitialState());
  let reducedMotion = $state(false);
  const look = $derived(companionExpression(companion.kind, { reducedMotion }));
  const status = $derived(companionStatusText(companion));

  function resetHideTimer() {
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      visible = false;
    }, 10000);
  }

  // Simple outlined icons (24px grid, stroked with currentColor), keyed by action name.
  const icons: Record<string, string> = {
    screenshot: '<path d="M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2"/><circle cx="12" cy="12" r="3"/>',
    click: '<path d="M6 3l12 9-5 1.5L16 20l-2.5 1-3-6.5L6 18z"/>',
    move: '<path d="M5 19L19 5M9 5h10v10"/>',
    scroll: '<path d="M12 4v16M8 8l4-4 4 4M8 16l4 4 4-4"/>',
    type: '<rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7 10h.01M11 10h.01M15 10h.01M8 14h8"/>',
    key: '<path d="M4 20l8-8M12 12l3-3M18 6l2-2M14 10l2 2"/><circle cx="17" cy="7" r="3"/>',
    command: '<path d="M5 7l5 5-5 5M12 17h7"/>',
    desktop: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>',
    action: '<path d="M13 3L5 13h6l-1 8 8-10h-6z"/>',
  };

  const actionIcons: Record<string, string> = {
    screenshot: "screenshot", left_click: "click", right_click: "click", middle_click: "click",
    double_click: "click", mouse_move: "move", scroll: "scroll", type: "type", key: "key",
    bash: "command", switch_desktop: "desktop",
  };

  function flashAction(name: string, text: string) {
    const id = ++idCounter;
    const icon = icons[actionIcons[name] ?? "action"] ?? icons.action;
    actionQueue = [...actionQueue, { id, text, icon }];
    setTimeout(() => { actionQueue = actionQueue.filter(a => a.id !== id); }, 3000);
  }

  onMount(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    reducedMotion = query.matches;
    const onMotion = (e: MediaQueryListEvent) => (reducedMotion = e.matches);
    query.addEventListener("change", onMotion);

    const unlistenAction = listen<string>("computer-use-action", (e) => {
      const event = overlayEvent("computer-use-action", e.payload);
      if (event) companion = reduceCompanion(companion, event);
      let name = "";
      try {
        name = String(JSON.parse(e.payload).action ?? "");
      } catch {
        // Not JSON: no icon to pick.
      }
      visible = true;
      flashAction(name, event && event.type === "action" ? event.summary : e.payload);
      resetHideTimer();
    });

    const unlistenDone = listen("computer-use-idle", () => {
      const event = overlayEvent("computer-use-idle", undefined);
      if (event) companion = reduceCompanion(companion, event);
      visible = false;
    });

    return () => {
      query.removeEventListener("change", onMotion);
      unlistenAction.then(fn => fn());
      unlistenDone.then(fn => fn());
    };
  });
</script>

<div class="overlay" class:overlay-visible={visible}>
  <div class="pip" data-motion={look.motion} data-kind={companion.kind}>
    <Moon size={48} expression={look.expression} />
  </div>

  <p class="status" role="status" aria-live="polite">{status}</p>

  <div class="flash-stack" aria-live="polite">
    {#each actionQueue as flash (flash.id)}
      <div class="flash">
        <svg class="flash-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          {@html flash.icon}
        </svg>
        <span class="flash-text">{flash.text}</span>
      </div>
    {/each}
  </div>
</div>

<style>
  :global(html), :global(body) {
    background: transparent !important;
    margin: 0;
    padding: 0;
    overflow: hidden;
  }

  .overlay {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 99999;
    opacity: 0;
    transition: opacity 0.5s ease;
  }
  .overlay-visible { opacity: 1; }

  .pip {
    position: absolute;
    bottom: 16px;
    right: 16px;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--card);
    border: 1px solid var(--border);
    box-shadow: 0 4px 16px rgb(0 0 0 / 30%);
    animation: pip-in 0.5s cubic-bezier(0.34, 1.56, 0.64, 1) both;
  }

  /* Motion per state, from the shared table: the moon nods while it works,
     breathes at rest, holds still when stopped or when the viewer asked. */
  .pip :global(.moon-body) {
    transform-origin: 160px 165px;
  }
  .pip[data-motion="nod"] :global(.moon-body) {
    animation: nod 1.2s ease-in-out infinite;
  }
  .pip[data-motion="breathe"] :global(.moon-body),
  .pip[data-motion="float"] :global(.moon-body) {
    animation: float 6s ease-in-out infinite;
  }
  .pip[data-motion="settle"] :global(.moon-body) {
    animation: settle 0.7s cubic-bezier(0.34, 1.56, 0.64, 1) 1;
  }

  .pip :global(.moon-eyes) {
    transform-box: fill-box;
    transform-origin: center;
  }
  .pip:not([data-motion="none"]) :global(.moon-eyes) {
    animation: blink 6.5s ease-in-out infinite;
  }

  @keyframes pip-in {
    from { opacity: 0; transform: scale(0.5) translateY(10px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  @keyframes float {
    0%, 100% { transform: translateY(0) rotate(-2deg); }
    50% { transform: translateY(-6px) rotate(2deg); }
  }

  @keyframes nod {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(5px); }
  }

  @keyframes settle {
    0% { transform: scale(1); }
    40% { transform: scale(1.06); }
    100% { transform: scale(1); }
  }

  @keyframes blink {
    0%, 42%, 46%, 73%, 77%, 100% { transform: scaleY(1); }
    44%, 75% { transform: scaleY(0.08); }
  }

  /* The words: what the companion is doing here, for a screen reader and for
     anyone who turned motion off. Visually it sits beside the pip. */
  .status {
    position: absolute;
    right: 92px;
    bottom: 34px;
    margin: 0;
    max-width: calc(100vw - 124px);
    padding: 6px 12px;
    border-radius: var(--radius-control);
    background: var(--card);
    border: 1px solid var(--border);
    box-shadow: 0 4px 16px rgb(0 0 0 / 30%);
    font: 500 12px/1.4 var(--font-body);
    color: var(--foreground);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .flash-stack {
    position: absolute;
    bottom: 90px;
    right: 16px;
    display: flex;
    flex-direction: column-reverse;
    gap: 6px;
    align-items: flex-end;
  }

  .flash {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-radius: var(--radius-control);
    background: var(--card);
    border: 1px solid var(--border);
    box-shadow: 0 4px 16px rgb(0 0 0 / 30%);
    animation: flash-in 3s ease both;
    white-space: nowrap;
  }

  @keyframes flash-in {
    0% { opacity: 0; transform: translateX(16px); }
    8% { opacity: 1; transform: translateX(0); }
    80% { opacity: 1; }
    100% { opacity: 0; transform: translateX(8px); }
  }

  .flash-icon {
    width: 14px;
    height: 14px;
    color: var(--primary);
    flex-shrink: 0;
  }

  .flash-text {
    font: 500 12px/1.4 var(--font-mono);
    color: var(--foreground);
  }

  @media (prefers-reduced-motion: reduce) {
    .pip,
    .pip :global(.moon-body),
    .pip :global(.moon-eyes),
    .flash {
      animation: none;
    }
    .overlay {
      transition: none;
    }
  }
</style>
