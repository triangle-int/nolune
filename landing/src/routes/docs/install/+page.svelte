<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
</script>

<DocsPage slug="install">
	<p class="docs-lede">There are two ways to get a server. Both end the same way: the <code>nolune</code> binary in <code>~/.nolune/bin</code>, a workspace in <code>~/.nolune</code>, and the server running in the foreground. No background service is created unless you ask for one.</p>

	<h2 id="desktop-app">From the desktop app</h2>
	<p>Download the desktop app from <a href="https://github.com/triangle-int/nolune/releases" target="_blank" rel="noopener">GitHub Releases</a>. On first run, choose <strong>Install on this computer</strong>. The app downloads the server for your platform into <code>~/.nolune/bin</code>, prepares the workspace, starts the server as a process it owns, and opens your companion. Nothing to paste and no terminal required.</p>
	<ul>
		<li><strong>Show logs</strong> reveals the server output; a failed install expands it and offers <strong>Retry install</strong>.</li>
		<li><strong>Use nightly builds</strong> sits behind an advanced toggle and installs the <code>nightly</code> release instead of the latest stable one.</li>
		<li>The server runs while the app is open and starts again with it. Turn on <strong>Run in background</strong> in the app to hand it to a user-level service that keeps running after you quit; the connection screen says which mode is active.</li>
	</ul>
	<p>If a server already runs somewhere else, choose <strong>Connect to an existing server</strong> instead; see <a href="#connect-your-computers">Connect your computers</a> below.</p>

	<h2 id="one-liner">With the one-line installer</h2>
	<p>On macOS or Linux, in a terminal:</p>
	<pre><code>curl -fsSL https://nolune.dev/install.sh | bash</code></pre>
	<p>The script is a thin wrapper. Read it at <a href="/install.sh">nolune.dev/install.sh</a> before you run it; it is <code>scripts/install.sh</code> from the repository, served from this site at the deployed commit. It:</p>
	<ol class="steps">
		<li>Detects your platform and downloads the matching server binary from GitHub Releases into <code>~/.nolune/bin</code>, along with a small <code>update</code> script beside it.</li>
		<li>Adds <code>~/.nolune/bin</code> to the <code>PATH</code> in your shell's rc file, so new terminals find <code>nolune</code>.</li>
		<li>Runs <code>nolune onboard</code>, which prepares <code>~/.nolune</code> and writes <code>config.toml</code> with a generated auth token.</li>
		<li>Runs <code>nolune gateway</code> in the foreground, waits until it is ready, and opens <code>http://localhost:26559</code> in your browser.</li>
	</ol>
	<p>The server keeps running in that terminal so you can watch its logs. Press <kbd>Ctrl-C</kbd> to stop it and run <code>nolune gateway</code> to start it again.</p>
	<div class="table-scroll">
		<table>
			<thead><tr><th>Variable</th><th>Effect</th></tr></thead>
			<tbody>
				<tr><td><code>NOLUNE_CHANNEL=nightly</code></td><td>Install the nightly release instead of the latest stable one.</td></tr>
				<tr><td><code>NOLUNE_DIR=/path</code></td><td>Install somewhere other than <code>~/.nolune</code>.</td></tr>
				<tr><td><code>GITHUB_TOKEN=…</code></td><td>Authenticate the release lookup the nightly channel makes, if GitHub rate-limits you.</td></tr>
			</tbody>
		</table>
	</div>
	<p>Set a variable in front of the command, for example <code>NOLUNE_CHANNEL=nightly curl -fsSL https://nolune.dev/install.sh | bash</code>.</p>

	<h2 id="pair-a-browser">Pair a browser</h2>
	<p>The first browser has to be paired. On the computer running the server:</p>
	<pre><code>nolune pair</code></pre>
	<p>It prints a one-time code. Enter that code in the browser at <code>http://localhost:26559</code>, then follow the onboarding: choose a name, add your model provider key, and meet your companion. The code works once and expires after a few minutes; run <code>nolune pair</code> again for each additional browser or device.</p>
	<p>Paired browsers stay signed in. Review or revoke them, or mint a code for another device, under <strong>Settings → Connections</strong>. Until you open a new terminal, the command is <code>~/.nolune/bin/nolune pair</code>.</p>

	<h2 id="background-service">Keep it running</h2>
	<p>To keep the server running without a terminal, opt in to the background service:</p>
	<pre><code>nolune gateway install</code></pre>
	<p>This registers a user-level launchd agent on macOS or a systemd user unit on Linux and starts it. It needs no elevated privileges, survives logouts and reboots, and is managed with <code>nolune gateway start</code>, <code>stop</code>, <code>restart</code>, <code>status</code>, and <code>logs</code>. Stop a foreground <code>nolune gateway</code> first; the install refuses while something else listens on the port. <code>nolune gateway uninstall</code> removes the service again and leaves your data untouched. The service is not available on Windows yet.</p>

	<h2 id="the-nolune-command">The nolune command</h2>
	<p>Everything the installers do, you can do yourself:</p>
	<div class="table-scroll">
		<table>
			<thead><tr><th>Command</th><th>What it does</th></tr></thead>
			<tbody>
				<tr><td><code>nolune onboard</code></td><td>Prepares <code>~/.nolune</code> and <code>config.toml</code> with a generated auth token. Safe to rerun. <code>--json</code> prints the result for scripts, <code>--port</code> picks the port.</td></tr>
				<tr><td><code>nolune gateway</code></td><td>Runs the server in the foreground and prints <code>nolune: ready &lt;url&gt;</code> once it listens (<code>nolune gateway run</code> is the explicit form).</td></tr>
				<tr><td><code>nolune gateway install</code></td><td>Registers a user-level launchd agent (macOS) or systemd user unit (Linux) that runs the gateway, and starts it.</td></tr>
				<tr><td><code>nolune gateway uninstall</code></td><td>Stops and removes that service; data is untouched.</td></tr>
				<tr><td><code>nolune gateway start</code> / <code>stop</code> / <code>restart</code></td><td>Control the service once installed.</td></tr>
				<tr><td><code>nolune gateway status</code> / <code>logs</code></td><td>Show whether the service is installed and running, or stream its logs.</td></tr>
				<tr><td><code>nolune pair</code></td><td>Prints a one-time code so a browser can sign in.</td></tr>
				<tr><td><code>nolune version</code></td><td>Prints the installed server version.</td></tr>
				<tr><td><code>nolune uninstall --keep-data</code></td><td>Removes the service, binary, and log but keeps <code>~/.nolune</code>.</td></tr>
				<tr><td><code>nolune uninstall --yes</code></td><td>Removes everything, including your data.</td></tr>
				<tr><td><code>--profile &lt;name&gt;</code></td><td>Any of the above for a second, fully isolated server on the same machine: its own data root at <code>~/.nolune-profiles/&lt;name&gt;/</code>, config, port, auth token, log, and background service, from the same binary. <code>nolune onboard --profile molinka</code> then <code>nolune gateway install --profile molinka</code>; the default profile stays <code>~/.nolune</code>. Every profile runs the one binary under <code>~/.nolune/bin/</code>, so <code>nolune uninstall</code> on the default profile warns which profiles' services lose it.</td></tr>
			</tbody>
		</table>
	</div>
	<p>Advanced configuration lives in <code>~/.nolune/config.toml</code>; the <a href="https://github.com/triangle-int/nolune#configuration" target="_blank" rel="noopener">README</a> lists the environment overrides such as <code>NOLUNE_HOME</code>, <code>NOLUNE_AUTH_TOKEN</code>, and <code>NOLUNE_PUBLIC_URL</code>.</p>

	<h2 id="connect-your-computers">Connect your computers</h2>
	<p>Install the desktop app on the computers where you want Nolune to act. On each, choose <strong>Connect to an existing server</strong> and enter the server's root address (for example <code>http://192.168.1.20:26559</code>; a base path such as <code>/nolune</code> is not supported) and the auth token from <code>~/.nolune/config.toml</code> on the server. <strong>Test connection</strong> checks it without saving; <strong>Save connection</strong> stores the token in the operating system's credential store and opens the companion.</p>
	<p>Each connected machine becomes another place where the same companion can see the screen, use apps, work with files, and run commands, once you grant the operating-system permissions the app asks for. Connected computers appear on the <strong>Computers</strong> page in the web interface.</p>
</DocsPage>
