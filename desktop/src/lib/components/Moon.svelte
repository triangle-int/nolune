<script lang="ts">
  /**
   * Little Moon avatar. Same path as client/static/skins/moon/character.svg;
   * the eyes of each expression come from the shared table
   * (client/src/lib/companion/expressions.js), so the overlay's face matches
   * the static SVG the client shows for the same state.
   */
  import { MOON_BODY, eyesMarkup } from "../../../../client/src/lib/companion/expressions.js";

  let {
    size = 32,
    label = "",
    expression = "idle",
    class: className = "",
  }: { size?: number | string; label?: string; expression?: string; class?: string } = $props();

  const eyes = $derived(eyesMarkup(expression, "var(--primary-foreground)"));
</script>

<svg
  class={className}
  viewBox="0 0 320 320"
  fill="none"
  width={size}
  height={size}
  role={label ? "img" : undefined}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : "true"}
  data-expression={expression}
>
  <path class="moon-body" d={MOON_BODY} fill="var(--primary)" />
  <g class="moon-eyes">{@html eyes}</g>
</svg>

<style>
  svg {
    display: block;
    flex-shrink: 0;
    overflow: visible;
  }
</style>
