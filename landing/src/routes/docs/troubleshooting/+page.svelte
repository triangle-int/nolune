<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
</script>

<DocsPage slug="troubleshooting">
	<p class="docs-lede">Most problems are one of these. Each answer names the message you will see and the command that fixes it.</p>

	<h2 id="port-in-use">Port 26559 is already in use</h2>
	<p>The server says <code>port 26559 is already in use on 0.0.0.0. Another Nolune gateway or service is probably running</code>, or <code>nolune gateway install</code> refuses because <code>something is already listening on port 26559</code>.</p>
	<ol class="steps">
		<li>Check for a service: <code>nolune gateway status</code>. If one is running, you already have a server; open <code>http://localhost:26559</code>, or stop it with <code>nolune gateway stop</code> before starting one in the foreground.</li>
		<li>Check for a foreground server: a terminal still running <code>nolune gateway</code>, or the desktop app with a local install open. Press <kbd>Ctrl-C</kbd> there, or quit the app.</li>
		<li>If the port belongs to something else, change <code>port</code> in <code>~/.nolune/config.toml</code> (or under <strong>Settings → Advanced</strong>) and restart. The desktop app and paired browsers use the new address.</li>
	</ol>
	<p>The desktop app never shadows a server you started yourself: installing again over its own server is an upgrade, but anything else on the port makes it stop and tell you.</p>

	<h2 id="pairing">The browser or desktop app asks for a pairing code</h2>
	<p>Every new browser and desktop app pairs once, with the same kind of code. On the computer running the server:</p>
	<pre><code>nolune pair</code></pre>
	<ul>
		<li><strong>The code was refused or expired.</strong> A code works once and expires after a few minutes; run <code>nolune pair</code> again and enter the new one.</li>
		<li><strong><code>nolune pair</code> says the server is not reachable.</strong> It talks to the running server; start it with <code>nolune gateway</code> (or <code>nolune gateway start</code>) and retry.</li>
		<li><strong><code>nolune pair</code> says the server rejected the token.</strong> The service was started with a different <code>NOLUNE_AUTH_TOKEN</code> than the one in <code>config.toml</code>; run <code>nolune pair</code> with the same value, or start the server without the override.</li>
		<li><strong>It says pairing is disabled.</strong> <code>auth_token</code> is empty in <code>config.toml</code>, so browsers open Nolune without pairing (the desktop app then connects with <strong>Use an API token instead</strong> and any value). Only leave it that way on a machine nobody else can reach.</li>
		<li><strong>A code made in a browser is refused by the desktop app.</strong> Codes from <strong>Settings → Connections → Pair a device</strong> in a browser only work at that browser's address. Enter the same server URL in the app, or use <code>nolune pair</code>, whose codes work at any address.</li>
	</ul>
	<p>Paired devices, and a code for the next one, are under <strong>Settings → Connections</strong> once you are in.</p>

	<h2 id="path">nolune: command not found</h2>
	<p>The installer added <code>~/.nolune/bin</code> to the <code>PATH</code> in your shell's rc file, but the terminal you ran it in was opened before that. Open a new terminal, or in the current one:</p>
	<pre><code>export PATH="$HOME/.nolune/bin:$PATH"</code></pre>
	<p>Every command also works by full path: <code>~/.nolune/bin/nolune pair</code>. If new terminals still cannot find it, the rc file the installer wrote (<code>~/.zshrc</code>, <code>~/.bashrc</code>, and so on, matching your login shell) may not be the one your terminal reads; add the line above to the right file.</p>

	<h2 id="service">Is the service running, and where are its logs?</h2>
	<pre><code>nolune gateway status
nolune gateway logs</code></pre>
	<p><code>status</code> answers one of three ways: <code>background service is not installed</code> (the server runs in the foreground or under the desktop app, or not at all), <code>service is installed but not running</code> (start it with <code>nolune gateway start</code> and read the logs), or running with its process id. <code>logs</code> follows <code>~/.nolune/nolune.log</code> on macOS and the user journal (<code>journalctl --user</code>) on Linux.</p>
	<p>A foreground server prints its logs to the terminal, and the desktop app shows them under <strong>Show logs</strong>. For more detail in the foreground, run <code>RUST_LOG=debug nolune gateway</code>; the service logs at <code>info</code>. The service is user-level: it starts when you log in and restarts on its own if the server exits.</p>

	<h2 id="install-script">The installer could not download the server</h2>
	<p><code>download failed — could not fetch …</code> means <code>github.com</code> was unreachable or slow; the script retries transient errors, so check the connection and run it again. On the nightly channel the release lookup goes through the GitHub API, which rate-limits anonymous requests: set <code>GITHUB_TOKEN</code> to a token of yours for that run. <code>unsupported OS</code> or <code>unsupported architecture</code> means the script has no build for this machine; see <a href="/docs/prerequisites">Prerequisites</a>.</p>
	<p><code>nolune did not become ready</code> means the download worked but the server did not start; run <code>~/.nolune/bin/nolune gateway</code> by hand to see why, which is usually <a href="#port-in-use">the port</a>.</p>

	<h2 id="desktop-app">The desktop app cannot connect</h2>
	<ul>
		<li>Enter the server's <strong>root</strong> address, scheme and port included, such as <code>http://192.168.1.20:26559</code>. A path after the port (<code>/nolune</code>) is rejected.</li>
		<li>Pair it with a one-time code, just like a browser: see <a href="#pairing">above</a>. If the app says it is <strong>no longer paired</strong>, it was revoked under <strong>Settings → Connections</strong>; get a fresh code and pair again.</li>
		<li><strong>This server is too old for pairing codes</strong> means the server predates desktop pairing. Update it, or choose <strong>Use an API token instead</strong> and enter <code>auth_token</code> from <code>~/.nolune/config.toml</code> on the server. <strong>Test connection</strong> checks a token without saving.</li>
		<li>From another computer the server must be reachable over the network: firewalls on the server, and a server bound to a different <code>host</code> in <code>config.toml</code>, are the usual causes.</li>
		<li>Computer use needs the operating-system permissions the app lists in its settings; a granted permission shows as a badge, the rest have a <strong>Grant</strong> button.</li>
	</ul>

	<h2 id="model-provider">Replies fail with a provider error</h2>
	<p>Nolune passes your model provider's answer through. An authentication error means the key under <strong>Settings → Connections</strong> is wrong or revoked; a rate or quota error is that provider's limit, not Nolune's. Background work (memory extraction, check-ins, reflection) uses the <strong>Background</strong> preset on the same page and is skipped, with a log line, when that preset has no usable key.</p>

	<h2 id="help">Still stuck?</h2>
	<p>Open an issue on <a href="https://github.com/triangle-int/nolune/issues" target="_blank" rel="noopener">GitHub</a> with the command you ran, the message you saw, and <code>nolune version</code>. For anything security-related, follow the <a href="https://github.com/triangle-int/nolune/blob/main/SECURITY.md" target="_blank" rel="noopener">security policy</a> instead of a public issue.</p>
</DocsPage>
