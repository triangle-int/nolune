<script lang="ts">
	// nolune's privacy policy, at nolune.dev/privacy: the App Store asks for one, and the iOS app
	// links it. Keep it to what the code does: packages/relay for the relay, ios/ for the app.
	const GITHUB = 'https://github.com/triangle-int/nolune';
	const EMAIL = 'timur@triangleint.com';
	const UPDATED = 'October 1, 2026';
</script>

<svelte:head>
	<title>Privacy · nolune</title>
	<meta
		name="description"
		content="What nolune, its apps, its relay and this website do with your information: your family's chats stay on your family's computer."
	/>
</svelte:head>

<main class="wrap">
	<article>
		<h1>Privacy</h1>
		<p class="updated">Last updated {UPDATED}</p>
		<p class="lede">
			nolune runs on your family's own computer, and what your family writes to it stays there. This
			page says what passes through what Triangle Interactive, LLC (“we”) runs, and what we keep.
		</p>

		<h2>The short version</h2>
		<ul>
			<li>
				nolune doesn't send us your chats, memory, files or accounts. We have no server that keeps
				them.
			</li>
			<li>There are no ads, analytics or trackers in nolune, its apps or this website.</li>
			<li>
				Two things pass through a server we run, and only when you use them: the relay, when your
				family's nolune has an address on <code>nolune.family</code>, and notifications in the
				iPhone app. We keep none of what passes through.
			</li>
		</ul>

		<h2>Your family's nolune</h2>
		<p>
			nolune runs on a computer in your family, and keeps conversations, memory, files and accounts
			there. Whoever runs it decides who has an account, and can see and delete everything on that
			computer. Everyone in a shared profile can read its chats and memory.
		</p>
		<p>
			To answer, nolune sends the model your family chose (Anthropic, OpenAI, xAI, OpenRouter, or a
			server of your own) what it needs: the conversation, the files in it, and what it remembers
			that matters. The same goes for the other services your family sets it up with, like making
			pictures or searching the web. Their privacy policies cover what they do with it.
		</p>
		<p>
			Once a day, nolune asks GitHub whether there's a new release, so admins hear of it. GitHub
			sees the request, as any website sees a visit. <code>nolune config set update-check off</code>
			stops it.
		</p>

		<h2>The relay</h2>
		<p>
			When your family's nolune has an address through the relay, like
			<code>smiths.nolune.family</code>, whoever opens it reaches your computer through our server.
			The connections on both sides of it are encrypted, but the relay itself passes your requests
			on in the clear, as any hosted tunnel does. It doesn't store or log what passes through.
		</p>
		<p>What it keeps about each address:</p>
		<ul>
			<li>its name, and a hash of the secret your nolune connects with;</li>
			<li>when it was registered, and when your nolune was last connected;</li>
			<li>how much traffic passed through it this month, for the monthly limit;</li>
			<li>
				the network it was registered from (an IPv4 address, or the first half of an IPv6 address),
				to limit how many addresses one network can have;
			</li>
			<li>and, if we ever block it, why.</li>
		</ul>
		<p>
			Its log says when addresses are registered, connect and disconnect, and what went wrong when
			something does. It has no IP addresses in it, and its size is capped, so older lines are
			deleted as new ones come in.
		</p>
		<p>
			<code>nolune relay disable</code> deletes what the relay keeps about your address, and an
			address whose nolune hasn't connected in 90 days is deleted on its own. If you'd rather nobody
			stood in between, use a tunnel of your own, or
			<a href="{GITHUB}/tree/main/packages/relay">run your own relay</a>.
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
				If you share analytics with app developers in your device's settings, Apple may send us
				crash reports. They don't include what you've written.
			</li>
		</ul>

		<h2>The Mac app</h2>
		<p>
			It runs nolune on your Mac, as above, and sends us nothing. It and nolune's other downloads
			come from GitHub and npm, under their privacy policies.
		</p>

		<h2>This website</h2>
		<p>
			nolune.dev and its docs are static pages hosted by Vercel. They set no cookies and have no
			analytics or trackers, and their fonts come from here. Vercel keeps the usual server logs,
			like IP addresses, to run and protect its service, under
			<a href="https://vercel.com/legal/privacy-policy">its privacy policy</a>.
		</p>

		<h2>Children</h2>
		<p>
			nolune is made for families, and a family may make accounts for its children on its own
			nolune. Those accounts and what they write stay on the family's computer. We don't collect
			information from anyone, children included.
		</p>

		<h2>Changes and questions</h2>
		<p>
			When this changes, we'll update this page and the date at the top; every version is in
			<a href="{GITHUB}/commits/main/site/src/routes/privacy/+page.svelte">nolune's repository</a>.
			For questions, write to <a href="mailto:{EMAIL}">{EMAIL}</a>, or
			<a href="{GITHUB}/issues">open an issue on GitHub</a>.
		</p>
	</article>
</main>

<style>
	article {
		max-width: 72ch;
		padding-block: 72px 96px;
		line-height: 1.75;
	}
	h1,
	h2 {
		color: var(--cream);
		font-family: var(--sans);
	}
	h1 {
		margin: 0;
		font-weight: 900;
		font-size: clamp(2.6rem, 5vw, 4rem);
		letter-spacing: -0.03em;
	}
	h2 {
		margin: 56px 0 0;
		font-weight: 800;
		font-size: 1.6rem;
		letter-spacing: -0.01em;
	}
	.updated {
		margin-top: 12px;
		color: var(--muted);
		font-size: 0.9rem;
	}
	.lede {
		margin-top: 32px;
		font-size: 1.1rem;
	}
	p,
	ul {
		margin: 18px 0 0;
	}
	ul {
		padding-left: 1.2em;
	}
	li + li {
		margin-top: 10px;
	}
	/* The page's own monospace, at its size (browsers make `code` smaller). */
	code {
		font: inherit;
		color: var(--cream);
		overflow-wrap: anywhere;
	}
	a {
		color: var(--cream);
		text-underline-offset: 3px;
	}
	a:hover {
		color: var(--rust);
	}

	@media (max-width: 560px) {
		article {
			padding-block: 48px 72px;
		}
	}
</style>
