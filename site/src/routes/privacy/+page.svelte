<script lang="ts">
	import Prose from '$lib/Prose.svelte';

	// nolune's privacy policy, at nolune.dev/privacy: the App Store asks for one, and the iOS app
	// links it. Keep it to what the code does: packages/relay for the relay, packages/api for the
	// nolune plan, ios/ for the app.
	const GITHUB = 'https://github.com/triangle-int/nolune';
	const EMAIL = 'timur@triangleint.com';
	const UPDATED = 'October 2, 2026';
</script>

<svelte:head>
	<title>Privacy · nolune</title>
	<meta
		name="description"
		content="What nolune, its apps, its relay, the nolune plan and this website do with your information: your family's chats stay on your family's computer."
	/>
</svelte:head>

<Prose>
	<h1>Privacy</h1>
	<p class="updated">Last updated {UPDATED}</p>
	<p class="lede">
		nolune runs on your family's own computer, and what your family writes to it stays there. This
		page says what passes through what Triangle Interactive, LLC (“we”) runs, and what we keep.
	</p>

	<h2>The short version</h2>
	<ul>
		<li>
			nolune doesn't send us your chats, memory, files or accounts, and we have no server that keeps
			them. On the nolune plan, chats pass through our server on their way to the model, and it
			keeps none of them.
		</li>
		<li>There are no ads, analytics or trackers in nolune, its apps or this website.</li>
		<li>
			Three things pass through a server we run, and only when you use them: the relay, when your
			family's nolune has an address on <code>nolune.family</code>, notifications in the iPhone app,
			and requests on the nolune plan. We keep none of what passes through.
		</li>
		<li>
			If you subscribe to the nolune plan, we keep your email address and what's left of your plan,
			and Stripe handles the payment.
		</li>
	</ul>

	<h2>Your family's nolune</h2>
	<p>
		nolune runs on a computer in your family, and keeps conversations, memory, files and accounts
		there. Whoever runs it decides who has an account, and can see and delete everything on that
		computer. Everyone in a shared profile can read its chats and memory.
	</p>
	<p>
		To answer, nolune sends the model your family chose (Anthropic, OpenAI, xAI, OpenRouter, the
		nolune plan below, or a server of your own) what it needs: the conversation, the files in it,
		and what it remembers that matters. The same goes for the other services your family sets it up
		with, like making pictures or searching the web. Their privacy policies cover what they do with
		it.
	</p>
	<p>
		Once a day, nolune asks GitHub whether there's a new release, so admins hear of it. GitHub sees
		the request, as any website sees a visit. <code>nolune config set update-check off</code>
		stops it.
	</p>

	<h2>The relay</h2>
	<p>
		When your family's nolune has an address through the relay, like
		<code>smiths.nolune.family</code>, whoever opens it reaches your computer through our server.
		The connections on both sides of it are encrypted, but the relay itself passes your requests on
		in the clear, as any hosted tunnel does. It doesn't store or log what passes through.
	</p>
	<p>What it keeps about each address:</p>
	<ul>
		<li>its name, and a hash of the secret your nolune connects with;</li>
		<li>when it was registered, and when your nolune was last connected;</li>
		<li>how much traffic passed through it this month, for the monthly limit;</li>
		<li>
			the network it was registered from (an IPv4 address, or the first half of an IPv6 address), to
			limit how many addresses one network can have;
		</li>
		<li>and, if we ever block it, why.</li>
	</ul>
	<p>
		Its log says when addresses are registered, connect and disconnect, and what went wrong when
		something does. It has no IP addresses in it, and its size is capped, so older lines are deleted
		as new ones come in.
	</p>
	<p>
		<code>nolune relay disable</code> deletes what the relay keeps about your address, and an
		address whose nolune hasn't connected in 90 days is deleted on its own. If you'd rather nobody
		stood in between, use a tunnel of your own, or
		<a href="{GITHUB}/tree/main/packages/relay">run your own relay</a>.
	</p>

	<h2>The nolune plan</h2>
	<p>
		The nolune plan is a subscription to nolune itself: chats, pictures and memory search without
		keys of your own. When your family's nolune uses it, its requests go through nolune's API, a
		server we run at <code>api.nolune.dev</code>, on their way to the model.
	</p>
	<p>
		They carry what nolune would otherwise send the model itself: the conversation, the files in it,
		what nolune remembers that matters, a picture to make or change, and words to search memory by.
		The API passes them on to OpenRouter, which passes them to the company that runs the model your
		family picked; their privacy policies cover what they do with it. The API doesn't store or log
		any of it. It reads only what it needs to check and charge a request, like which model it's for
		and how long it is, and takes what OpenRouter says it cost off your plan.
	</p>
	<p>What it keeps about your account:</p>
	<ul>
		<li>your email address, which you sign in with, and when the account was made;</li>
		<li>
			for each browser you're signed in on, and each nolune you've linked: when it signed in, its IP
			address, and which browser or program it is, while that session lasts;
		</li>
		<li>
			your plan: what its credits were this month and what's left of them, which payment each came
			from and when they run out, what background work spent today, and when the plan renews;
		</li>
		<li>and which customer you are at Stripe.</li>
	</ul>
	<p>
		Sign-in codes are sent by Resend, which sees your email address and the code; a code lasts 5
		minutes. Payments go through Stripe, which keeps your payment details: we never see your card
		number. We give Stripe your email address and your account's id, so a payment finds its account.
		From Stripe we learn what was bought and when, and whether the subscription is paid, cancelled
		or behind. Stripe may email you about it, like when a payment doesn't go through.
	</p>
	<p>
		The API's log says what went wrong when something does, and its size is capped, so older lines
		are deleted as new ones come in. Its database is copied once a day, and each copy is deleted
		after 14 days. Signing out, or unlinking nolune, ends that session. To delete your account,
		write to us: we'll delete it and everything above, apart from what Stripe has to keep about
		payments.
	</p>
	<h2>The iPhone and iPad app</h2>
	<p>
		The app keeps the address of your family's nolune on your device. Everything else it shows is
		your family's nolune, from your family's computer.
	</p>
	<ul>
		<li>
			It uses the camera and your photos only when you take or pick something to send to your
			family's nolune, and saves to your photos only when you ask it to.
		</li>
		<li>
			If you allow notifications, Apple gives the app a token for your device, which the app gives
			your family's nolune. For each notification, your family's nolune sends its title and text,
			the profile's name and that token to the relay, which passes them to Apple to deliver. The
			relay keeps none of it. Signing out, or connecting to another nolune, stops them.
		</li>
		<li>
			If you share analytics with app developers in your device's settings, Apple may send us crash
			reports. They don't include what you've written.
		</li>
	</ul>

	<h2>The Mac app</h2>
	<p>
		It runs nolune on your Mac, as above, and sends us nothing. It and nolune's other downloads come
		from GitHub and npm, under their privacy policies.
	</p>

	<h2>This website</h2>
	<p>
		nolune.dev and its docs are static pages hosted by Vercel. They set no cookies and have no
		analytics or trackers, and their fonts come from here. Vercel keeps the usual server logs, like
		IP addresses, to run and protect its service, under
		<a href="https://vercel.com/legal/privacy-policy">its privacy policy</a>.
	</p>

	<h2>Children</h2>
	<p>
		nolune is made for families, and a family may make accounts for its children on its own nolune.
		Those accounts and what they write stay on the family's computer. We don't collect information
		from children: the nolune plan's account is for the adult who pays for it.
	</p>

	<h2>Changes and questions</h2>
	<p>
		When this changes, we'll update this page and the date at the top; every version is in
		<a href="{GITHUB}/commits/main/site/src/routes/privacy/+page.svelte">nolune's repository</a>.
		For questions, write to <a href="mailto:{EMAIL}">{EMAIL}</a>, or
		<a href="{GITHUB}/issues">open an issue on GitHub</a>.
	</p>
</Prose>
