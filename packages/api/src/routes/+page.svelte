<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const dollars = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;
	const when = (ms: number | null) =>
		ms === null
			? ''
			: new Date(ms).toLocaleString(undefined, {
					weekday: 'short',
					hour: 'numeric',
					minute: '2-digit'
				});
	const day = (ms: number) =>
		new Date(ms).toLocaleDateString(undefined, { month: 'long', day: 'numeric' });
	const share = (spent: number, limit: number) =>
		limit > 0 ? Math.min(100, Math.round((spent / limit) * 100)) : 0;
	/** "40% · again Thu 18:40", made here: Svelte trims the space at the start of an {#if}. */
	const detail = (limit: { spent: number; limit: number; again: string | null }) =>
		[`${share(limit.spent, limit.limit)}%`, limit.again].filter(Boolean).join(' · ');

	/** The month's credits, and the 5-hour and weekly limits on a plan that has them. */
	const limits = $derived.by(() => {
		const u = data.usage;
		if (!u) return [];
		return [
			u.window && {
				label: '5 hours',
				...u.window,
				again: u.window.resetsAt ? `again ${when(u.window.resetsAt)}` : null
			},
			u.week && { label: 'This week', ...u.week, again: `again ${when(u.week.resetsAt)}` },
			{
				label: 'This month',
				...u.month,
				again: u.month.resetsAt ? `renews ${day(u.month.resetsAt)}` : null
			}
		].filter((limit) => !!limit);
	});

	/** Back from Checkout: what was paid for shows up once Stripe's event has. */
	let gaveUp = $state(false);

	$effect(() => {
		if (!data.back || data.back.granted || gaveUp) return;
		const started = Date.now();
		const timer = setInterval(() => {
			if (Date.now() - started > 60_000) gaveUp = true;
			else void invalidateAll();
		}, 2000);
		return () => clearInterval(timer);
	});

	const status = $derived.by(() => {
		const s = data.subscription;
		if (!s) return null;
		if (s.status === 'past_due' || s.status === 'unpaid') {
			return "The last payment didn't go through. Stripe tries again over the next days, and the plan keeps what's left of its credits until then; a new card in Manage settles it.";
		}
		if (s.endsAt) return `Cancelled: the plan ends on ${day(s.endsAt)}.`;
		if (s.renewsAt) return `Renews on ${day(s.renewsAt)}.`;
		return null;
	});
</script>

<svelte:head>
	<title>Your plan · nolune</title>
</svelte:head>

<section class="flex flex-col gap-1">
	<h1 class="text-2xl font-semibold">Your plan</h1>
	<p class="text-muted-foreground">{data.email}</p>
</section>

{#if data.back && !data.back.granted}
	<p class="rounded-xl bg-muted p-4 text-sm" role="status" aria-live="polite">
		{#if gaveUp}
			Stripe has the payment, and it's taking a while to reach us. It shows up here within a few
			minutes; reload then.
		{:else if data.back.what === 'plan'}
			Thanks! Starting the plan…
		{:else}
			Thanks! Adding the credits…
		{/if}
	</p>
{/if}

{#if form?.error}
	<p class="text-danger text-sm" role="alert">{form.error}</p>
{/if}

{#if data.usage}
	{@const u = data.usage}
	<dl class="flex flex-col gap-4">
		{#each limits as limit (limit.label)}
			<div class="flex flex-col gap-1.5">
				<div class="flex justify-between text-sm">
					<dt class="font-medium">{limit.label}</dt>
					<dd class="text-muted-foreground">
						{detail(limit)}
					</dd>
				</div>
				<div class="h-2 overflow-hidden rounded-full bg-muted">
					<div
						class="h-full rounded-full bg-primary"
						style:width="{share(limit.spent, limit.limit)}%"
					></div>
				</div>
			</div>
		{/each}
		<div class="flex justify-between text-sm">
			<dt class="font-medium">Credits left</dt>
			<dd class="text-muted-foreground">
				{u.credits.extra > 0
					? `${dollars(u.credits.plan)} + ${dollars(u.credits.extra)} extra`
					: dollars(u.credits.plan)}
			</dd>
		</div>
	</dl>
	{#if status}
		<p class="text-sm text-muted-foreground">{status}</p>
	{/if}
	{#if data.selling}
		<form method="POST" action="?/manage">
			<button class="rounded-full border border-border px-4 py-2 text-sm font-medium">
				Manage
			</button>
		</form>
	{/if}

	{#if data.offers}
		<section class="flex flex-col gap-3 rounded-xl border border-border p-4 text-sm">
			<div class="flex flex-col gap-1">
				<h2 class="font-medium">{data.offers.pack.name}</h2>
				{#if data.offers.pack.description}
					<p class="text-muted-foreground">{data.offers.pack.description}</p>
				{/if}
			</div>
			<form method="POST" action="?/buyCredits">
				<button class="rounded-full bg-primary px-4 py-2 font-medium text-primary-foreground">
					Buy for {data.offers.pack.price}
				</button>
			</form>
		</section>
	{/if}
{:else}
	{#if data.subscription || data.back?.what === 'plan'}
		<!-- Paid for, and Stripe's event is on its way: the note above says so. -->
		{#if !data.back}
			<p class="rounded-xl bg-muted p-4 text-sm">
				Stripe has the subscription, and the plan starts once its payment reaches us.
			</p>
		{/if}
	{:else if data.offers}
		<section class="flex flex-col gap-4 rounded-xl border border-border p-4">
			<div class="flex flex-col gap-1">
				<h2 class="font-medium">{data.offers.plan.name}</h2>
				<p class="text-sm text-muted-foreground">
					{data.offers.plan.description ??
						'Chats, pictures and memory search for nolune on your computer, with no API keys.'}
				</p>
			</div>
			<form method="POST" action="?/subscribe">
				<button
					class="w-full rounded-full bg-primary px-4 py-2.5 font-medium text-primary-foreground"
				>
					Subscribe for {data.offers.plan.price} a month
				</button>
			</form>
		</section>
	{:else}
		<p class="rounded-xl bg-muted p-4 text-sm">
			No plan yet. With one, nolune on your computer gets chats, pictures and memory search with no
			API keys.
			{data.billingError ?? 'Plans open soon.'}
		</p>
	{/if}
	{#if data.extra > 0}
		<p class="text-sm text-muted-foreground">
			{dollars(data.extra)} of extra credits wait for a plan.
		</p>
	{/if}
	{#if data.subscription && status}
		<p class="text-sm text-muted-foreground">{status}</p>
	{/if}
{/if}

{#if data.usage || data.extra > 0}
	<form method="POST" action="?/extraCredits" class="flex items-start gap-3 text-sm">
		<input type="hidden" name="on" value={String(!data.extraPastLimits)} />
		<div class="flex flex-1 flex-col gap-0.5">
			<span class="font-medium">Go on with extra credits past a limit</span>
			<span class="text-muted-foreground">
				{data.extraPastLimits
					? 'On: chats go on with extra credits once a limit is reached. Background work still waits.'
					: 'Off: chats wait for the limit to reset, and extra credits are kept.'}
			</span>
		</div>
		<button
			role="switch"
			aria-checked={data.extraPastLimits}
			aria-label="Go on with extra credits past a limit"
			class="relative mt-0.5 h-6 w-10 shrink-0 rounded-full transition-colors {data.extraPastLimits
				? 'bg-primary'
				: 'bg-border'}"
		>
			<span
				class="absolute top-0.5 left-0.5 size-5 rounded-full bg-background transition-transform {data.extraPastLimits
					? 'translate-x-4'
					: ''}"
			></span>
		</button>
	</form>
{/if}

<form method="POST" action="?/signOut">
	<button class="text-sm text-muted-foreground underline underline-offset-4">Sign out</button>
</form>
