<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
</script>

<DocsPage slug="upgrade">
	<p class="docs-lede">Nolune checks for updates on its own and tells you when a newer release exists. Applying one is two steps: replace the binary, then restart the server the same way it was started. Your data is never touched by an update.</p>

	<h2 id="apply">Apply an update</h2>
	<p><strong>From the web interface.</strong> When a newer release is out, an <strong>Update available</strong> pill appears in the web interface. Click it: the server runs its update script and exits so it can come back on the new version. <strong>Settings → Advanced</strong> shows the installed version, what is new, and lets you switch the release channel between stable and nightly.</p>
	<p><strong>From a terminal on the server.</strong> The installer left an update script beside the binary:</p>
	<pre><code>~/.nolune/bin/update</code></pre>
	<p>It downloads the current release for your platform, replaces <code>~/.nolune/bin/nolune</code>, and prints <code>already at &lt;version&gt;</code> when there is nothing to do. It follows the channel you installed from; <code>NOLUNE_CHANNEL=nightly ~/.nolune/bin/update</code> switches a stable install to nightly for that run. Rerunning the <a href="/docs/install#one-liner">one-line installer</a> does the same and then starts the server again.</p>
	<p>Check the result with <code>nolune version</code>.</p>

	<h2 id="restart">Restart the server</h2>
	<p>A running process keeps the old binary until it restarts. Restart the way the server was started:</p>
	<ul>
		<li><strong>Background service</strong> (<code>nolune gateway install</code>): <code>nolune gateway restart</code>. After an update from the web interface, launchd or systemd brings the service back on its own.</li>
		<li><strong>Foreground</strong> in a terminal: stop it with <kbd>Ctrl-C</kbd> and run <code>nolune gateway</code> again.</li>
		<li><strong>Desktop app</strong> managing the server: quit and reopen the app; it restarts the server it owns. The app's own updater only updates the app, not the server.</li>
	</ul>
	<p>Not sure which applies? <code>nolune gateway status</code> says whether a service is installed and running; if it is not, the server was started in the foreground or by the desktop app.</p>

	<h2 id="profiles">Several profiles on one machine</h2>
	<p>Profiles share the one binary under <code>~/.nolune/bin</code>, so a single update covers all of them. Restart each service afterwards: <code>nolune gateway restart --profile &lt;name&gt;</code> for every named profile, and <code>nolune gateway restart</code> for the default one.</p>

	<h2 id="desktop-app">Desktop app updates</h2>
	<p>The desktop app checks for its own updates and installs them like any other app; you can also download the latest build from <a href="https://github.com/triangle-int/nolune/releases" target="_blank" rel="noopener">GitHub Releases</a>. Updating the app does not update a server, and updating the server does not require a new app. Keep both current; either order works.</p>

	<h2 id="rollback">Going back</h2>
	<p>Every release keeps its binaries on GitHub Releases. To pin an older version, download the <code>nolune-server-&lt;target&gt;</code> asset for your platform from that release, replace <code>~/.nolune/bin/nolune</code> with it, make it executable, and restart. <a href="/docs/backup">Back up</a> first: a newer server may have written files an older one does not read.</p>
</DocsPage>
