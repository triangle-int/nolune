<script lang="ts">
	import Footer from '$lib/components/Footer.svelte';
</script>

<svelte:head>
	<title>Privacy Policy — Nolune</title>
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
		<h1 class="legal-title">Privacy Policy</h1>
		<p class="legal-date">Last updated: September 20, 2026</p>

		<div class="prose">
			<h2>1. Who we are</h2>
			<p>
				Nolune is open-source, self-hosted software published by Triangle Interactive ("we", "us", "our").
				You run the Nolune server on your own hardware. We do not host it for you, and we do not operate
				accounts, sign-ups, or hosted companions. This policy explains what this website and the software do
				with information.
			</p>

			<h2>2. This website</h2>
			<p>
				nolune.dev is a static site. It has no accounts, forms, analytics trackers, or third-party tracking
				scripts. Our hosting provider keeps ordinary server logs (such as request paths, IP addresses, and
				user agents) for a limited time to operate and secure the site. The install script is downloaded from
				this site and runs on your machine; it sends nothing back to us.
			</p>

			<h2>3. The software</h2>
			<p>
				Nolune keeps one companion per server. Its identity, memory, conversations, uploads, and settings are
				stored as files on the computer where you run the server. Connected computers, chats, and relationship
				scopes are contexts of that one companion, not separate copies with separate data. We have no access
				to any of it. Nolune does not send usage metrics, telemetry, or crash reports to us.
			</p>
			<p>
				The software makes outbound requests only to services you configure or invoke:
			</p>
			<ul>
				<li><strong>Your model provider</strong> (Anthropic or OpenAI, with your own API key) receives the conversation and memory context needed to answer each request, subject to that provider's privacy policy.</li>
				<li><strong>Update checks</strong> fetch release metadata from GitHub so Nolune can tell you a newer version exists.</li>
				<li><strong>Skills, extensions, email, and GitHub integrations</strong> talk to the third parties you connect them to, with credentials you provide and can remove at any time.</li>
			</ul>

			<h2>4. Optional Google Workspace access</h2>
			<p>
				Nolune does not provide a central Google OAuth service and does not copy or store Google OAuth
				credentials. If you install the optional official gog skill, the external gog CLI owns its credentials
				locally through gog's configured keyring or credential backend. Google content is requested only when
				you invoke the skill or CLI and remains subject to your Google account permissions.
			</p>

			<h2>5. Retention and deletion</h2>
			<p>
				Everything Nolune stores lives under the data directory on your server (by default <code>~/.nolune</code>).
				You can inspect, export, correct, or forget memories from the Memory page, export the whole companion as
				one archive from Settings, and delete everything by uninstalling with data removal. Nothing is retained
				anywhere else because nothing is sent anywhere else.
			</p>

			<h2>6. Security</h2>
			<p>
				Connections to your server use the address and API token you configure; browsers pair for a revocable
				session. Keep your token and provider API keys private, connect only computers you control, and review
				the operating-system permissions you grant the desktop app. See our
				<a href="https://github.com/triangle-int/nolune/blob/main/SECURITY.md">security policy</a> to report a
				vulnerability.
			</p>

			<h2>7. Changes</h2>
			<p>This policy may change as the software does. The current version is always published on this page and in the repository.</p>

			<h2>8. Contact</h2>
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
		color: oklch(0.78 0.02 280);
		font-size: 0.95rem;
		font-weight: 600;
		margin-top: 1.5rem;
		margin-bottom: 0.5rem;
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
