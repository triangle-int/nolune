<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
</script>

<DocsPage slug="prerequisites">
	<p class="docs-lede">Nolune is two things: a server that holds one companion's identity and memory, and a desktop app that lets that companion see and use the computers you connect. Here is what each needs.</p>

	<h2>A home for the server</h2>
	<p>The server should run on a computer that stays on: a Mac mini, a home server, or a spare laptop that does not sleep. It is a single binary, and everything it knows lives as files under <code>~/.nolune</code>.</p>
	<ul>
		<li><strong>macOS</strong> on Apple silicon or Intel.</li>
		<li><strong>Linux</strong> on x86_64 or arm64.</li>
		<li><strong>Windows</strong>: the desktop app can install and run the server on a 64-bit PC, and each release attaches a Windows server binary. The one-line installer and the background service are macOS and Linux only for now, so on Windows the server runs while the desktop app is open or while a terminal runs <code>nolune gateway</code>.</li>
	</ul>
	<p>Disk use is modest. The binary is small and the data directory grows with your memories, chat history, and the files you upload.</p>

	<h2>A model provider</h2>
	<p>Nolune brings no model of its own. You add an <strong>Anthropic</strong> or <strong>OpenAI</strong> API key during onboarding, or later under <strong>Settings → Connections</strong>, where you can also name the models it may call. That provider's pricing and usage limits apply; Nolune adds none of its own and needs no account with us.</p>

	<h2>Network</h2>
	<p>The server listens on port <code>26559</code> on every interface by default, and the installer opens <code>http://localhost:26559</code> on the machine it runs on. Other computers on your network reach it at <code>http://&lt;address&gt;:26559</code>; if you reach it through a reverse proxy, tunnel, or other address, set <code>NOLUNE_PUBLIC_URL</code> to that address so shared file links resolve.</p>
	<p>Nolune exposes nothing to the internet by itself. Browsers must <a href="/docs/install#pair-a-browser">pair</a> before they can open it, and the desktop app and CLI authenticate with the token in <code>~/.nolune/config.toml</code>. Only expose the server beyond your network on purpose, over HTTPS you control.</p>

	<h2>For the one-line installer</h2>
	<p><code>curl</code> and <code>bash</code>, plus a route to <code>github.com</code>: the script downloads the server from GitHub Releases and installs nothing else. Read it first at <a href="/install.sh">nolune.dev/install.sh</a>; it is the same file as <code>scripts/install.sh</code> in the repository at the deployed commit.</p>

	<h2>The desktop app</h2>
	<p>Install the desktop app on each computer you want Nolune to act on. Builds for macOS, Windows, and Linux are on <a href="https://github.com/triangle-int/nolune/releases" target="_blank" rel="noopener">GitHub Releases</a>. Computer use needs operating-system permissions (on macOS, screen recording and accessibility); the app's settings show which are granted and let you grant the rest. Connect only computers you control.</p>
	<p>A phone needs nothing installed: when the server is reachable, its web interface works in the phone's browser after pairing.</p>
</DocsPage>
