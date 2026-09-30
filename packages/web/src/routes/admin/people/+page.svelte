<script lang="ts">
	import { enhance } from '$app/forms';
	import LinkIcon from '@lucide/svelte/icons/link';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import TopBar from '$lib/components/TopBar.svelte';
	import UserAvatar from '$lib/components/UserAvatar.svelte';
	import CopyField from '$lib/components/admin/CopyField.svelte';
	import { getI18n } from '$lib/i18n';

	let { data, form } = $props();
	const { m, intl } = getI18n();
	const t = $derived(m.people);
	const uid = $props.id();

	type Person = (typeof data.people)[number];
	/** Whose password is about to be reset, and who is about to be removed. */
	let resetting = $state<Person | null>(null);
	let removing = $state<Person | null>(null);

	/** The add and invite forms, open, and waiting for the server. */
	let adding = $state(false);
	let addBusy = $state(false);
	let inviting = $state(false);
	let inviteBusy = $state(false);
	/** Whether the account being added is an admin's, kept when the server refuses it. */
	let makeAdmin = $state(false);

	function day(date: Date): string {
		return date.toLocaleDateString(intl, { day: 'numeric', month: 'short' });
	}
</script>

<div class="flex h-full flex-col">
	<TopBar />
	<main class="min-h-0 flex-1 overflow-y-auto">
		<div class="mx-auto max-w-2xl space-y-8 px-4 py-8 sm:py-12">
			<div class="space-y-1">
				<h1 class="text-2xl font-semibold">{t.title}</h1>
				<p class="text-muted-foreground">{t.hint}</p>
			</div>

			<section class="space-y-3" aria-labelledby="{uid}-accounts">
				<h2 id="{uid}-accounts" class="text-lg font-medium">{t.accounts}</h2>

				{#if form?.peopleMessage}
					<p class="rounded-2xl bg-muted px-4 py-3 text-sm" role="status">{form.peopleMessage}</p>
				{:else if form?.peopleError}
					<p class="text-sm text-destructive" role="alert">{form.peopleError}</p>
				{/if}

				<ul class="overflow-hidden rounded-2xl border">
					{#each data.people as person (person.id)}
						{@const mine = person.id === data.me}
						<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
							<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
								<UserAvatar
									name={person.name}
									picture={person.picture}
									class="size-9 text-sm max-sm:self-start"
								/>
								<div class="min-w-0 flex-1">
									<div class="flex items-center gap-2">
										<span class="truncate font-medium">{person.name}</span>
										{#if mine}
											<Badge variant="secondary">{t.you}</Badge>
										{/if}
										{#if person.isAdmin}
											<Badge variant="outline">{t.admin}</Badge>
										{/if}
									</div>
									<div class="truncate text-muted-foreground">{person.email}</div>
								</div>
								<!-- Under the text on phones, so it keeps the width. -->
								<div class="flex flex-wrap gap-1 max-sm:basis-full max-sm:pl-9">
									<Button
										variant="ghost"
										size="sm"
										class="text-muted-foreground"
										onclick={() => (resetting = person)}
									>
										{t.resetPassword}
									</Button>
									<!-- Not on their own account: an admin could lock themselves out. -->
									{#if !mine}
										<form method="POST" action="?/admin" use:enhance>
											<input type="hidden" name="id" value={person.id} />
											<input type="hidden" name="on" value={String(!person.isAdmin)} />
											<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground">
												{person.isAdmin ? t.removeAdmin : t.makeAdmin}
											</Button>
										</form>
										<Button
											variant="ghost"
											size="sm"
											class="text-muted-foreground"
											onclick={() => (removing = person)}
										>
											{m.common.remove}
										</Button>
									{/if}
								</div>
							</div>
							{#if form?.reset?.id === person.id}
								<div class="space-y-2 sm:pl-12" role="status">
									<p class="text-muted-foreground">{t.newPassword(person.name)}</p>
									<CopyField text={form.reset.password} label={t.copyPassword} />
								</div>
							{/if}
						</li>
					{/each}
				</ul>

				{#if adding}
					<form
						method="POST"
						action="?/create"
						class="space-y-4 rounded-2xl border p-4 text-sm sm:p-5"
						aria-labelledby="{uid}-add"
						use:enhance={() => {
							addBusy = true;
							return async ({ result, update }) => {
								await update({ reset: false });
								addBusy = false;
								if (result.type === 'success') {
									adding = false;
									makeAdmin = false;
								}
							};
						}}
					>
						<div class="space-y-1">
							<h3 id="{uid}-add" class="text-base font-medium">{t.add}</h3>
							<p class="text-muted-foreground">{t.addHint}</p>
						</div>
						<div class="space-y-2">
							<label for="{uid}-name" class="block font-medium">{t.name}</label>
							<Input
								id="{uid}-name"
								name="name"
								value={form?.name ?? ''}
								required
								autocomplete="off"
								placeholder={t.namePlaceholder}
								class="h-10 rounded-full px-4 sm:w-60"
							/>
						</div>
						<div class="space-y-2">
							<label for="{uid}-email" class="block font-medium">{t.email}</label>
							<Input
								id="{uid}-email"
								name="email"
								type="email"
								value={form?.email ?? ''}
								required
								autocomplete="off"
								spellcheck="false"
								placeholder={t.emailPlaceholder}
								class="h-10 rounded-full px-4 sm:w-80"
							/>
						</div>
						<div class="flex items-start gap-3">
							<Switch
								id="{uid}-admin"
								name="admin"
								bind:checked={makeAdmin}
								aria-describedby="{uid}-admin-hint"
								class="mt-0.5"
							/>
							<div class="space-y-0.5">
								<label for="{uid}-admin" class="block font-medium">{t.makeThemAdmin}</label>
								<p id="{uid}-admin-hint" class="text-muted-foreground">{t.adminHint}</p>
							</div>
						</div>
						{#if form?.createError}
							<p class="text-destructive" role="alert">{form.createError}</p>
						{/if}
						<div class="flex gap-2">
							<Button type="submit" disabled={addBusy} class="h-10 px-5 max-sm:flex-1">
								{addBusy ? t.adding : m.common.add}
							</Button>
							<Button type="button" variant="ghost" class="h-10" onclick={() => (adding = false)}>
								{m.common.cancel}
							</Button>
						</div>
					</form>
				{:else}
					{#if form?.created}
						<div class="space-y-2 rounded-2xl bg-muted/50 p-4 text-sm" role="status">
							<p>{t.added(form.created.name)}</p>
							<div class="space-y-1">
								<div class="text-muted-foreground">{t.address}</div>
								<CopyField text={form.created.address} label={t.copyAddress} />
							</div>
							<div class="space-y-1">
								<div class="text-muted-foreground">{t.email}</div>
								<CopyField text={form.created.email} label={m.common.copy} />
							</div>
							<div class="space-y-1">
								<div class="text-muted-foreground">{t.password}</div>
								<CopyField text={form.created.password} label={t.copyPassword} />
							</div>
						</div>
					{/if}
					<Button variant="outline" class="h-10 rounded-full px-4" onclick={() => (adding = true)}>
						<PlusIcon />
						{t.add}
					</Button>
				{/if}
			</section>

			<section class="space-y-3" aria-labelledby="{uid}-invites">
				<div class="space-y-1">
					<h2 id="{uid}-invites" class="text-lg font-medium">{t.invites}</h2>
					<p class="text-muted-foreground">{t.invitesHint(data.inviteDays)}</p>
				</div>

				{#if form?.invited}
					<div class="space-y-2 rounded-2xl bg-muted/50 p-4 text-sm" role="status">
						<p>{t.inviteCreated(form.invited.name)}</p>
						<CopyField text={form.invited.link} label={t.copyLink} />
					</div>
				{:else if form?.inviteMessage}
					<p class="rounded-2xl bg-muted px-4 py-3 text-sm" role="status">{form.inviteMessage}</p>
				{/if}

				<ul class="overflow-hidden rounded-2xl border">
					{#each data.invites as invite (invite.id)}
						<li
							class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b px-4 py-3 text-sm last:border-b-0"
						>
							<span
								class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground max-sm:self-start"
							>
								<LinkIcon class="size-4" />
							</span>
							<div class="min-w-0 flex-1">
								<div class="truncate font-medium">{t.inviteTitle(invite.name)}</div>
								<div class="text-muted-foreground">
									{t.inviteDetails(invite.createdBy, day(invite.expiresAt))}
								</div>
							</div>
							<form
								method="POST"
								action="?/revokeInvite"
								class="max-sm:basis-full max-sm:pl-9"
								use:enhance
							>
								<input type="hidden" name="id" value={invite.id} />
								<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground">
									{t.revoke}
								</Button>
							</form>
						</li>
					{:else}
						<li class="px-4 py-3 text-sm text-muted-foreground">{t.noInvites}</li>
					{/each}
				</ul>

				{#if inviting}
					<form
						method="POST"
						action="?/invite"
						class="space-y-4 rounded-2xl border p-4 text-sm sm:p-5"
						aria-labelledby="{uid}-invite"
						use:enhance={() => {
							inviteBusy = true;
							return async ({ result, update }) => {
								await update();
								inviteBusy = false;
								if (result.type === 'success') inviting = false;
							};
						}}
					>
						<h3 id="{uid}-invite" class="text-base font-medium">{t.newInvite}</h3>
						<div class="space-y-2">
							<label for="{uid}-for" class="block font-medium">
								{t.inviteFor} <span class="font-normal text-muted-foreground">{t.optional}</span>
							</label>
							<Input
								id="{uid}-for"
								name="name"
								autocomplete="off"
								maxlength={64}
								placeholder={t.inviteForPlaceholder}
								aria-describedby="{uid}-for-hint"
								class="h-10 rounded-full px-4 sm:w-60"
							/>
							<p id="{uid}-for-hint" class="text-muted-foreground">{t.inviteForHint}</p>
						</div>
						<div class="flex gap-2">
							<Button type="submit" disabled={inviteBusy} class="h-10 px-5 max-sm:flex-1">
								{inviteBusy ? t.creating : t.create}
							</Button>
							<Button type="button" variant="ghost" class="h-10" onclick={() => (inviting = false)}>
								{m.common.cancel}
							</Button>
						</div>
					</form>
				{:else}
					<Button
						variant="outline"
						class="h-10 rounded-full px-4"
						onclick={() => (inviting = true)}
					>
						<LinkIcon />
						{t.newInvite}
					</Button>
				{/if}
			</section>
		</div>
	</main>
</div>

<AlertDialog.Root open={resetting !== null} onOpenChange={(open) => !open && (resetting = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{t.resetTitle(resetting?.name ?? '')}</AlertDialog.Title>
			<AlertDialog.Description>{t.resetBody}</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/password"
			use:enhance={() => {
				return async ({ update }) => {
					resetting = null;
					await update();
				};
			}}
		>
			<input type="hidden" name="id" value={resetting?.id ?? ''} />
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit">{t.reset}</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>

<AlertDialog.Root open={removing !== null} onOpenChange={(open) => !open && (removing = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{t.removeTitle(removing?.name ?? '')}</AlertDialog.Title>
			<AlertDialog.Description>{t.removeBody}</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/remove"
			use:enhance={() => {
				return async ({ update }) => {
					removing = null;
					await update();
				};
			}}
		>
			<input type="hidden" name="id" value={removing?.id ?? ''} />
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive"
					>{m.common.remove}</AlertDialog.Action
				>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
