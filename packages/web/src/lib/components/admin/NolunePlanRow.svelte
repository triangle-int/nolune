<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidate } from '$app/navigation';
	import MoonStarIcon from '@lucide/svelte/icons/moon-star';
	import type { NolunePlanStatus, NolunePlanUsage } from '@nolune/core';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import Rich from '$lib/components/Rich.svelte';
	import PlanUsageBars from '$lib/components/chat/PlanUsageBars.svelte';
	import { planUsage } from '$lib/plan-usage.svelte';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	/*
	 * The nolune plan's row in Models & keys: linking nolune to the plan with a code someone
	 * approves on nolune's link page (from any device), checking the link and where the limits
	 * stand, and unlinking it.
	 */

	interface Props {
		/** The link as nolune keeps it, and a link under way. Never the token. */
		plan: {
			pending: { code: string; url: string; expiresAt: number } | null;
			signInError: string | null;
			status: NolunePlanStatus;
		};
		/** What the last action on this row said. */
		result: { planError?: string; planMessage?: string; usage?: NolunePlanUsage | null } | null;
	}

	let { plan, result }: Props = $props();
	const { m } = getI18n();

	let starting = $state(false);
	let checking = $state(false);
	let unlinking = $state(false);

	const linked = $derived(plan.status.signedIn);
	const problem = $derived(result?.planError ?? plan.signInError);
	/** Where the limits stand: the last Check's, else as the gateway last heard. */
	const usage = $derived(result?.usage ?? planUsage.current?.usage ?? null);

	$effect(() => {
		if (linked) void planUsage.refresh();
	});

	// The code is approved on another page, maybe on another device: ask until it has been.
	$effect(() => {
		if (!plan.pending) return;
		const timer = setInterval(() => invalidate('nolune:nolune-plan'), 3000);
		return () => clearInterval(timer);
	});

	/** "linked as …" at the start of a line. */
	function sentence(text: string): string {
		return `${text[0].toUpperCase()}${text.slice(1)}`;
	}

	/** A problem's first sentence: the rest says how to link, which the buttons do here. */
	function firstSentence(text: string): string {
		return text.split('. ')[0].replace(/\.?$/, '.');
	}
</script>

<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
		<span
			class={cn(
				'flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted max-sm:self-start',
				linked ? 'text-foreground' : 'text-muted-foreground'
			)}
		>
			<MoonStarIcon class="size-4" />
		</span>
		<div class="min-w-0 flex-1">
			<div class="font-medium">{m.admin.nolunePlan}</div>
			<div class="text-muted-foreground">{m.admin.nolunePlanAbout}</div>
			{#if linked}
				<div class="break-words text-muted-foreground">{sentence(linked)}</div>
			{/if}
		</div>
		<div class="flex flex-wrap gap-1 max-sm:basis-full max-sm:pl-9">
			{#if plan.pending}
				<form method="POST" action="?/nolunePlanCancel" use:enhance>
					<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground">
						{m.common.cancel}
					</Button>
				</form>
			{:else if linked}
				<form
					method="POST"
					action="?/nolunePlanCheck"
					use:enhance={() => {
						checking = true;
						return async ({ update }) => {
							await update();
							checking = false;
						};
					}}
				>
					<Button
						type="submit"
						variant="ghost"
						size="sm"
						class="text-muted-foreground"
						disabled={checking}
					>
						{checking ? m.common.checking : m.admin.nolunePlanCheck}
					</Button>
				</form>
				<Button
					variant="ghost"
					size="sm"
					class="text-muted-foreground"
					onclick={() => (unlinking = true)}
				>
					{m.admin.nolunePlanUnlink}
				</Button>
			{:else}
				<form
					method="POST"
					action="?/nolunePlanLink"
					use:enhance={() => {
						starting = true;
						return async ({ update }) => {
							await update();
							starting = false;
						};
					}}
				>
					<Button type="submit" size="sm" disabled={starting}>
						{starting ? m.admin.nolunePlanStarting : m.admin.nolunePlanLink}
					</Button>
				</form>
			{/if}
		</div>
	</div>

	{#if plan.pending}
		<div class="space-y-3 sm:pl-12" aria-live="polite">
			<p class="text-muted-foreground">
				<!-- eslint-disable svelte/no-navigation-without-resolve -- nolune's link page, another site -->
				<Rich text={m.admin.nolunePlanOpen}>
					{#snippet link()}<a
							href={plan.pending?.url}
							target="_blank"
							rel="noreferrer"
							class="underline">{m.admin.nolunePlanLinkPage}</a
						>{/snippet}
				</Rich>
				<!-- eslint-enable svelte/no-navigation-without-resolve -->
			</p>
			<p
				class="w-fit rounded-xl bg-muted px-4 py-2 font-mono text-xl font-semibold tracking-[0.2em]"
			>
				{plan.pending.code}
			</p>
			<p class="text-muted-foreground">{m.admin.nolunePlanWaiting}</p>
		</div>
	{/if}
	{#if problem}
		<p class="text-destructive sm:pl-12" role="alert">{problem}</p>
	{:else if linked && usage}
		<PlanUsageBars {usage} class="max-w-md sm:pl-12" />
	{:else if result?.planMessage}
		<p class="text-muted-foreground sm:pl-12" role="status">{result.planMessage}</p>
	{:else if linked}
		<p class="text-muted-foreground sm:pl-12">{m.admin.nolunePlanLinked}</p>
	{:else if plan.status.problem && !plan.pending}
		<p class="text-muted-foreground sm:pl-12">{firstSentence(plan.status.problem)}</p>
	{/if}
</li>

<AlertDialog.Root open={unlinking} onOpenChange={(open) => !open && (unlinking = false)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.admin.nolunePlanUnlinkTitle}</AlertDialog.Title>
			<AlertDialog.Description>{m.admin.nolunePlanUnlinkBody}</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/nolunePlanUnlink"
			use:enhance={() => {
				return async ({ update }) => {
					unlinking = false;
					await update();
				};
			}}
		>
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive"
					>{m.admin.nolunePlanUnlink}</AlertDialog.Action
				>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
