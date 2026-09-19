<script lang="ts">
	import Footer from '$lib/components/Footer.svelte';
</script>

<svelte:head>
	<title>Terms of Service — Nolune</title>
</svelte:head>

<div class="legal-page">
	<header class="legal-header">
		<div class="legal-header-inner">
			<a href="/" class="legal-brand">
				<div class="legal-logo" aria-hidden="true">n</div>
				<span class="legal-brand-name">nolune</span>
			</a>
		</div>
	</header>

	<div class="legal-content">
		<h1 class="legal-title">Terms of Service</h1>
		<p class="legal-date">Last updated: September 20, 2026</p>

		<div class="prose">
			<h2>1. Agreement</h2>
			<p>
				Nolune is open-source software published by Triangle Interactive ("we", "us") under the
				<a href="https://github.com/triangle-int/nolune/blob/main/LICENSE">MIT License</a>. By downloading,
				installing, or running it, you agree to these terms and to the license. If you don't agree, don't use it.
			</p>

			<h2>2. What Nolune is</h2>
			<p>
				Nolune is a self-hosted AI companion. You install the server on your own hardware, where it keeps one
				persistent companion with memory, tool access, and optional installable skills. Desktop apps on your
				other computers connect to that same companion. The companion uses third-party model providers you
				configure, with your own API keys, to generate responses.
			</p>

			<h2>3. No accounts or hosting</h2>
			<p>
				We do not operate accounts or hosted servers for Nolune, and there is nothing to pay us for. There is
				nothing to sign up for and nothing to cancel. You are responsible for the computers you run Nolune on, for keeping your API
				token and provider keys private, and for the costs your model provider charges you.
			</p>

			<h2>4. Your data</h2>
			<p>
				You own your data. Conversations, files, memories, and anything you create with your companion stay on
				your hardware and belong to you. We never receive or access them. See our
				<a href="/privacy">Privacy Policy</a> for what the software sends to services you configure.
			</p>

			<h2>5. Optional Google Workspace skill</h2>
			<p>
				Nolune does not include a Google account connection or token broker. If you install the optional official
				gog skill, you configure and authorize the external gog CLI yourself. gog owns those credentials
				locally through its configured keyring or credential backend; Nolune does not copy or store them. Review
				every side effect and revoke access with gog or from your Google account settings.
			</p>

			<h2>6. Acceptable use</h2>
			<p>You agree not to use Nolune to:</p>
			<ul>
				<li>Break the law</li>
				<li>Send spam or bulk unsolicited messages through the email tools</li>
				<li>Access computers, accounts, or data you are not authorized to use</li>
				<li>Generate content that violates your model provider's usage policies</li>
			</ul>

			<h2>7. AI limitations</h2>
			<p>
				Your companion is an AI. It can make mistakes, hallucinate, or produce inaccurate information.
				It should not be relied upon for medical, legal, financial, or safety-critical decisions.
				Actions taken by your companion (sending emails, creating events, running commands, using connected
				computers) are your responsibility. Review the permissions you grant and the approvals it asks for.
			</p>

			<h2>8. No warranty</h2>
			<p>
				The software is provided "as is" without warranties of any kind, as stated in the MIT License. To the
				maximum extent permitted by law, we are not liable for any damages arising from your use of it,
				including losses caused by actions the companion takes on your computers or costs charged by third-party
				providers.
			</p>

			<h2>9. Changes</h2>
			<p>These terms may change as the software does. The current version is always published on this page and in the repository.</p>

			<h2>10. Contact</h2>
			<p>Questions? Email <a href="mailto:support@nolune.dev">support@nolune.dev</a>.</p>
		</div>
	</div>

	<Footer />
</div>

<style>
	.legal-page { min-height: 100dvh; background: oklch(0.04 0.015 260); }
	.legal-header { border-bottom: 1px solid oklch(1 0 0 / 6%); background: oklch(0.05 0.015 260 / 70%); backdrop-filter: blur(24px); }
	.legal-header-inner { max-width: 900px; margin: 0 auto; padding: 1rem 1.5rem; }
	.legal-brand { display: flex; align-items: center; gap: 0.625rem; text-decoration: none; width: fit-content; }
	.legal-logo { width: 1.75rem; height: 1.75rem; border-radius: 0.375rem; display: flex; align-items: center; justify-content: center; font-family: var(--font-display); font-style: italic; font-size: 0.875rem; color: var(--color-warm); background: oklch(1 0 0 / 5%); border: 1px solid oklch(1 0 0 / 10%); }
	.legal-brand-name { font-family: var(--font-display); font-style: italic; font-size: 1.125rem; color: var(--color-text); }
	.legal-content { max-width: 700px; margin: 0 auto; padding: 4rem 1.5rem; }
	.legal-title { font-family: var(--font-display); font-style: italic; font-size: 1.875rem; font-weight: 400; color: var(--color-text); margin-bottom: 0.5rem; }
	.legal-date { font-size: 0.75rem; color: oklch(0.90 0.02 75 / 25%); margin-bottom: 3rem; }

	.prose {
		color: oklch(0.65 0.02 280);
		line-height: 1.75;
	}
	.prose h2 {
		color: oklch(0.88 0.02 280);
		font-size: 1.15rem;
		font-weight: 600;
		margin-top: 2.5rem;
		margin-bottom: 0.75rem;
	}
	.prose p {
		font-size: 0.875rem;
		margin-bottom: 1rem;
	}
	.prose ul {
		font-size: 0.875rem;
		padding-left: 1.5rem;
		margin-bottom: 1rem;
	}
	.prose li {
		margin-bottom: 0.35rem;
	}
	.prose a {
		color: oklch(0.78 0.12 75);
		text-decoration: underline;
		text-underline-offset: 2px;
	}
	.prose a:hover {
		color: oklch(0.85 0.12 75);
	}
</style>
