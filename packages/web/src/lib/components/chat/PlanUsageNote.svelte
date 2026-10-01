<script lang="ts">
	import type { NolunePlanUsage } from '@nolune/core';
	import { getI18n } from '$lib/i18n';
	import { usedPercent, whenAgain } from '$lib/usage-format';
	import { cn } from '$lib/utils';

	/**
	 * Under the composer, from 80% of a nolune plan's limit: the fuller of the two, as a short bar,
	 * and when it starts again. Nothing below that.
	 */
	let { usage }: { usage: NolunePlanUsage } = $props();
	const { m, intl } = getI18n();

	const worst = $derived.by(() => {
		const rows = [
			{ label: m.planUsage.window, ...usage.window },
			{ label: m.planUsage.week, ...usage.week }
		].map((row) => ({ ...row, percent: usedPercent(row.spent, row.limit) }));
		return rows.reduce((a, b) => (b.percent > a.percent ? b : a));
	});
	/** "5 hours: 85% used · resets 17:44", or "5 hours: limit reached · resets 17:44". */
	const text = $derived.by(() => {
		const said =
			worst.percent >= 100
				? m.planUsage.reached(worst.label)
				: `${worst.label}: ${m.planUsage.used(worst.percent)}`;
		return worst.resetsAt
			? `${said} · ${m.planUsage.resets(whenAgain(worst.resetsAt, intl))}`
			: said;
	});
</script>

{#if worst.percent >= 80}
	<div
		class="mx-auto mt-2 flex max-w-md items-center justify-center gap-2 text-xs"
		role="status"
		aria-live="polite"
	>
		<span class="shrink-0 text-muted-foreground">{m.planUsage.title}</span>
		<span class="h-1.5 w-16 shrink-0 overflow-hidden rounded-full bg-muted">
			<span
				class={cn(
					'block h-full rounded-full',
					worst.percent >= 100 ? 'bg-destructive' : 'bg-warning'
				)}
				style:width="{worst.percent}%"
			></span>
		</span>
		<span
			class={cn(
				'min-w-0 truncate tabular-nums',
				worst.percent >= 100 ? 'text-destructive' : 'text-warning'
			)}
		>
			{text}
		</span>
	</div>
{/if}
