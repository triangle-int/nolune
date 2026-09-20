<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
</script>

<DocsPage slug="upgrade">
	<p class="docs-lede">Nolune checks for updates on its own and tells you when a newer release exists. How you apply one depends on how the server was installed: a server from the desktop app is upgraded by installing again from the app; a server from the one-line installer has an update script that the web interface can run for you. Either way, your data is never touched by an update.</p>

	<h2 id="apply">Apply an update</h2>
	<p><strong>Settings → Advanced</strong> in the web interface shows the installed version and what is new, and lets you switch the release channel between stable and nightly. Whichever method you use, check the result with <code>nolune version</code>.</p>

	<h3 id="desktop-install">From the desktop app</h3>
	<p>For a server you installed with <strong>Install on this computer</strong>, choose it again: installing over the app's own server is the upgrade. On the connection screen click <strong>Disconnect</strong> so the install section shows, then <strong>Install on this computer</strong>. The app stops the server it owns, downloads the current release over <code>~/.nolune/bin/nolune</code>, starts it again and saves the connection back; the workspace and its auth token are kept. If <strong>Run in background</strong> is on, turn it off in the app's Settings first, because the reinstall refuses while a service holds the port, and turn it back on afterwards.</p>
	<div class="callout">
		<p>The desktop install writes no update script, so the two methods below apply only to a server from the <a href="/docs/install#one-liner">one-line installer</a>. The <strong>Update available</strong> pill still appears for a desktop install, but clicking it only shows <em>Updating…</em> for about half a minute before the pill returns: the server answers that it has no update script to run.</p>
	</div>

	<h3 id="web-interface">From the web interface</h3>
	<p>When a newer release is out, an <strong>Update available</strong> pill appears in the web interface. Click it: the server runs <code>~/.nolune/bin/update</code> and, once the binary has changed, exits so it can come back on the new version. Whether it comes back by itself depends on how it was started; see <a href="#restart">Restart the server</a>.</p>

	<h3 id="terminal">From a terminal on the server</h3>
	<p>The installer left an update script beside the binary:</p>
	<pre><code>~/.nolune/bin/update</code></pre>
	<p>It downloads the current release for your platform, replaces <code>~/.nolune/bin/nolune</code>, and prints <code>already at &lt;version&gt;</code> when there is nothing to do. It follows the channel you installed from; <code>NOLUNE_CHANNEL=nightly ~/.nolune/bin/update</code> switches a stable install to nightly for that run. Rerunning the <a href="/docs/install#one-liner">one-line installer</a> does the same and then starts the server again.</p>

	<h2 id="restart">Restart the server</h2>
	<p>A running process keeps the old binary until it restarts. A reinstall from the desktop app restarts the server as part of the install. After the pill, the server exits on its own once the binary has changed; after <code>~/.nolune/bin/update</code> or a binary you replaced yourself, it keeps running the old one. In both cases it has to come back the way it was started:</p>
	<ul>
		<li><strong>Background service</strong> (<code>nolune gateway install</code>): <code>nolune gateway restart</code>. After the pill, launchd or systemd brings the service back on its own.</li>
		<li><strong>Foreground</strong> in a terminal: after the pill the process has already exited, so run <code>nolune gateway</code> again. After the script, stop it with <kbd>Ctrl-C</kbd> first, then run <code>nolune gateway</code>.</li>
		<li><strong>Desktop app</strong> managing a server from the one-line installer: quit and reopen the app; it starts the binary in <code>~/.nolune/bin</code> again. That also applies after the pill, since the app does not start the server again until it is reopened. The app's own updater only updates the app, not the server.</li>
	</ul>
	<p>Not sure which applies? <code>nolune gateway status</code> says whether a service is installed and running; if it is not, the server was started in the foreground or by the desktop app.</p>

	<h2 id="profiles">Several profiles on one machine</h2>
	<p>Profiles share the one binary under <code>~/.nolune/bin</code>, so a single update covers all of them. Restart each service afterwards: <code>nolune gateway restart --profile &lt;name&gt;</code> for every named profile, and <code>nolune gateway restart</code> for the default one.</p>

	<h2 id="app-updates">Desktop app updates</h2>
	<p>The desktop app checks for its own updates and installs them like any other app; you can also download the latest build from <a href="https://github.com/triangle-int/nolune/releases" target="_blank" rel="noopener">GitHub Releases</a>. Updating the app does not update a server, and updating the server does not require a new app. Keep both current; either order works.</p>

	<h2 id="rollback">Going back</h2>
	<p>Every release keeps its binaries on GitHub Releases. To pin an older version, download the <code>nolune-server-&lt;target&gt;</code> asset for your platform from that release, replace <code>~/.nolune/bin/nolune</code> with it, make it executable, and restart as above. <a href="/docs/backup">Back up</a> first: a newer server may have written files an older one does not read.</p>
</DocsPage>
