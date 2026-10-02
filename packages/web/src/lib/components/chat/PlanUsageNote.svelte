<script lang="ts">
	import type { NolunePlanUsage } from '@nolune/core';
	import { getI18n } from '$lib/i18n';
	import { planLimits, usedPercent, whenItResets } from '$lib/usage-format';
	import { cn } from '$lib/utils';

	/**
	 * Under the composer, from 80% of a nolune plan's limit: the fullest, as a short bar, and when it
	 * starts again. Nothing below that.
	 */
	let { usage }: { usage: NolunePlanUsage } = $props();
	const { m, intl } = getI18n();

	const worst = $derived.by(() => {
		const rows = planLimits(usage).map((row) => ({
			...row,
			label: m.planUsage[row.key],
			percent: usedPercent(row.spent, row.limit)
		}));
		return rows.length ? rows.reduce((a, b) => (b.percent > a.percent ? b : a)) : null;
	});
	/** "This month: 85% used · renews 1 Nov", or "5 hours: limit reached · resets 17:44". */
	const text = $derived.by(() => {
		if (!worst) return '';
		const said =
			worst.percent >= 100
				? m.planUsage.reached(worst.label)
				: `${worst.label}: ${m.planUsage.used(worst.percent)}`;
		const again = whenItResets(worst, intl, m.planUsage);
		return again ? `${said} · ${again}` : said;
	});
</script>

{#if worst && worst.percent >= 80}
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
