<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { getI18n } from '$lib/i18n';

	let { data, form } = $props();
	const { m, intl } = getI18n();
	const t = $derived(m.invite);
	const uid = $props.id();
	let pending = $state(false);

	/** Used, expired or taken back, now or while the form was open. */
	const gone = $derived(!data.invite || (form && 'gone' in form));
</script>

<main class="flex min-h-full flex-col items-center justify-center p-4">
	<div class="w-full max-w-sm space-y-6 py-8">
		<div class="space-y-2 text-center">
			<div class="text-4xl font-semibold tracking-tight">nolune</div>
			{#if data.signedInAs}
				<h1 class="text-2xl font-medium">{t.signedInTitle(data.signedInAs)}</h1>
				<p class="text-sm text-muted-foreground">{t.signedInBody}</p>
			{:else if gone}
				<h1 class="text-2xl font-medium">{t.goneTitle}</h1>
				<p class="text-sm text-muted-foreground">{t.goneBody}</p>
			{:else}
				<h1 class="text-2xl font-medium">{t.title}</h1>
				<p class="text-sm text-muted-foreground">
					{#if data.invite?.createdBy}{t.from(data.invite.createdBy)}{/if}
					{t.hint}
				</p>
			{/if}
		</div>

		{#if data.signedInAs}
			<Button href={resolve('/')} class="h-12 w-full text-base">{t.open}</Button>
		{:else if gone}
			<Button href={resolve('/login')} variant="outline" class="h-12 w-full text-base">
				{t.signIn}
			</Button>
		{:else if data.invite}
			<form
				method="POST"
				class="space-y-4"
				use:enhance={() => {
					pending = true;
					return async ({ update }) => {
						await update({ reset: false });
						pending = false;
					};
				}}
			>
				<div class="space-y-2">
					<label for="{uid}-name" class="block text-sm font-medium">{t.name}</label>
					<Input
						id="{uid}-name"
						name="name"
						autocomplete="name"
						required
						maxlength={64}
						value={form && 'name' in form ? form.name : (data.invite.name ?? '')}
						aria-describedby="{uid}-name-hint"
						class="h-12 rounded-full px-5 text-base"
					/>
					<p id="{uid}-name-hint" class="text-xs text-muted-foreground">{t.nameHint}</p>
				</div>
				<div class="space-y-2">
					<label for="{uid}-email" class="block text-sm font-medium">{t.email}</label>
					<Input
						id="{uid}-email"
						name="email"
						type="email"
						autocomplete="email"
						required
						value={form && 'email' in form ? form.email : ''}
						class="h-12 rounded-full px-5 text-base"
					/>
				</div>
				<div class="space-y-2">
					<label for="{uid}-password" class="block text-sm font-medium">{t.password}</label>
					<Input
						id="{uid}-password"
						name="password"
						type="password"
						autocomplete="new-password"
						required
						minlength={data.minPassword}
						aria-describedby="{uid}-password-hint"
						class="h-12 rounded-full px-5 text-base"
					/>
					<p id="{uid}-password-hint" class="text-xs text-muted-foreground">
						{t.passwordHint(data.minPassword)}
					</p>
				</div>
				<div class="space-y-2">
					<label for="{uid}-again" class="block text-sm font-medium">{t.passwordAgain}</label>
					<Input
						id="{uid}-again"
						name="again"
						type="password"
						autocomplete="new-password"
						required
						class="h-12 rounded-full px-5 text-base"
					/>
				</div>
				{#if form && 'problem' in form}
					<p class="text-center text-sm text-destructive" role="alert">{form.problem}</p>
				{/if}
				<Button type="submit" disabled={pending} class="h-12 w-full text-base">
					{pending ? t.creating : t.create}
				</Button>
				<p class="text-center text-xs text-muted-foreground">
					{t.until(
						data.invite.expiresAt.toLocaleDateString(intl, { day: 'numeric', month: 'long' })
					)}
				</p>
			</form>
		{/if}
	</div>
</main>
