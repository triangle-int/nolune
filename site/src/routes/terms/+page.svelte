<script lang="ts">
	import { resolve } from '$app/paths';
	import Prose from '$lib/Prose.svelte';

	// The terms of the nolune plan, at nolune.dev/terms: Checkout, the customer portal and the
	// account page link them. Keep them to what packages/api does, and the prices to Stripe's.
	const GITHUB = 'https://github.com/triangle-int/nolune';
	const EMAIL = 'timur@triangleint.com';
	const ACCOUNT = 'https://api.nolune.dev/';
	const UPDATED = 'October 2, 2026';
</script>

<svelte:head>
	<title>Terms · nolune</title>
	<meta
		name="description"
		content="The terms of the nolune plan: what a subscription gives, how it renews and ends, and how it may be used."
	/>
</svelte:head>

<Prose>
	<h1>Terms</h1>
	<p class="updated">Last updated {UPDATED}</p>
	<p class="lede">
		These terms are for the nolune plan, a subscription to nolune itself sold by Triangle
		Interactive, LLC (“we”). nolune, the program, is open source under the
		<a href="{GITHUB}/blob/main/LICENSE">MIT license</a>, and free to use with your own keys without
		them.
	</p>

	<h2>The short version</h2>
	<ul>
		<li>
			The plan is credits for model use, each month, for your family's nolune: no keys of your own
			to get.
		</li>
		<li>
			It renews every month until you cancel, and cancelling keeps it to the end of the month.
		</li>
		<li>
			Use it for your family's nolune, within the law and the model makers' rules, and don't resell
			it.
		</li>
	</ul>

	<h2>Your account</h2>
	<p>
		You sign in to nolune's API (<code>api.nolune.dev</code>) with a code sent to your email, and
		link your family's nolune to your account. You need to be an adult to subscribe. Everyone in
		your family's nolune uses the plan you link it to, and you're responsible for what's done with
		your account and the nolune linked to it.
	</p>

	<h2>What the plan gives</h2>
	<ul>
		<li>
			Each month, credits for chats, pictures and memory search, spent at what OpenRouter charges
			for each request. While the launch offer lasts, the plan is $20 a month for $25 of credits.
		</li>
		<li>
			Up to $12.50 of the credits you haven't spent carry over to the next month. The rest end with
			the month.
		</li>
		<li>
			Automations and other work nolune does in the background may spend a tenth of the month's
			credits in a day, so one can't spend the month by itself; the people in your family can spend
			the rest as they like.
		</li>
		<li>
			Extra credits come in packs of $10, last a year, and are spent once the month's credits run
			out, when you've turned that on on your account page.
		</li>
		<li>
			Which models the plan offers follows what OpenRouter offers, and changes as models come and
			go.
		</li>
	</ul>
	<p>
		Prices are in US dollars, and your bank may show them in your currency. We'll tell subscribers
		at least a month before a price or the launch offer changes; the month you've paid for keeps
		what it came with.
	</p>

	<h2>Paying, renewing and cancelling</h2>
	<p>
		The plan is sold through Stripe, which takes the payment and, where it sells as merchant of
		record (through Link), charges the taxes and handles the purchase under its own terms. The plan
		renews each month on the day it started, until you cancel. You can cancel any time on
		<a href={ACCOUNT}>your account page</a> (Manage): the plan goes on to the end of the month you've
		paid for, and doesn't renew.
	</p>
	<p>
		If a payment doesn't go through, Stripe tries again over the next two weeks. Until then your
		family keeps what's left of the month's credits, without new ones; after the last try, the plan
		ends.
	</p>
	<p>
		Payments aren't refunded once the month has started, except where the law says otherwise. If
		something went wrong, write to us and we'll look at it. A payment that's refunded takes back
		what's left of the credits it paid for.
	</p>

	<h2>Using it fairly</h2>
	<ul>
		<li>Use the plan for your family's nolune. Don't share, sell or pass on its access.</li>
		<li>
			Follow the law, and the usage policies of OpenRouter and of the company that makes the model
			you use.
		</li>
		<li>Don't try to get around the plan's limits, or to use another account's credits.</li>
	</ul>
	<p>
		If an account breaks these, we may stop it from using the plan until it's sorted out, and for
		serious or repeated breaches end it.
	</p>

	<h2>What we can promise</h2>
	<p>
		We run the plan with care, but it's provided as it is: it may be down at times, models may be
		slow or unavailable, and what they answer can be wrong, so check what matters. As far as the law
		allows, we're not liable for losses that come from using the plan, beyond what you paid us for
		it in the three months before.
	</p>

	<h2>Your information</h2>
	<p>
		<a href={resolve('/privacy')}>The privacy policy</a> says what the plan's API keeps and what passes
		through it.
	</p>

	<h2>Changes and questions</h2>
	<p>
		When these terms change, we'll update this page and the date at the top, and tell subscribers
		before a change that affects them applies; every version is in
		<a href="{GITHUB}/commits/main/site/src/routes/terms/+page.svelte">nolune's repository</a>. For
		questions, write to <a href="mailto:{EMAIL}">{EMAIL}</a>.
	</p>
</Prose>
