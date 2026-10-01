<script lang="ts">
	import type { NolunePlanUsage } from '@nolune/core';
	import { getI18n } from '$lib/i18n';
	import { dollars, usedPercent, whenAgain } from '$lib/usage-format';
	import { cn } from '$lib/utils';

	/** The nolune plan's 5-hour and weekly limits as bars, and the credits left. */
	let { usage, class: className }: { usage: NolunePlanUsage; class?: string } = $props();
	const { m, intl } = getI18n();

	/** "60% used · resets 17:44". */
	function detail(percent: number, resetsAt: number | null): string {
		const used = m.planUsage.used(percent);
		return resetsAt ? `${used} · ${m.planUsage.resets(whenAgain(resetsAt, intl))}` : used;
	}

	const rows = $derived([
		{ label: m.planUsage.window, ...usage.window },
		{ label: m.planUsage.week, ...usage.week }
	]);
</script>

<div class={cn('space-y-3', className)}>
	{#each rows as row (row.label)}
		{@const percent = usedPercent(row.spent, row.limit)}
		<div class="space-y-1.5">
			<div class="flex items-baseline justify-between gap-3 text-xs">
				<span class="font-medium text-foreground">{row.label}</span>
				<span class="text-muted-foreground tabular-nums">
					{detail(percent, row.resetsAt)}
				</span>
			</div>
			<div
				class="h-1.5 overflow-hidden rounded-full bg-muted"
				role="progressbar"
				aria-label={row.label}
				aria-valuemin={0}
				aria-valuemax={100}
				aria-valuenow={percent}
			>
				<div
					class={cn(
						'h-full rounded-full transition-[width] duration-500',
						percent >= 100 ? 'bg-destructive' : percent >= 80 ? 'bg-warning' : 'bg-foreground/70'
					)}
					style:width="{Math.max(percent, percent > 0 ? 2 : 0)}%"
				></div>
			</div>
		</div>
	{/each}
	<div class="flex items-baseline justify-between gap-3 text-xs">
		<span class="text-muted-foreground">{m.planUsage.credits}</span>
		<span class="text-muted-foreground tabular-nums">
			{dollars(usage.credits.plan)}{#if usage.credits.extra > 0}
				<span class="ml-1">{m.planUsage.extra(dollars(usage.credits.extra))}</span>{/if}
		</span>
	</div>
</div>
