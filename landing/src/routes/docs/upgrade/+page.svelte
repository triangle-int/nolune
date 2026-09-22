<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
</script>

<DocsPage slug="upgrade">
	<p class="docs-lede">Nolune checks for updates on its own and tells you when a newer release exists. In most cases the <strong>Update available</strong> pill in the web interface applies one for you: a server the desktop app started is updated by the app itself, and a server from the one-line installer runs the update script the installer left beside it. Either way, your data is never touched by an update.</p>

	<h2 id="apply">Apply an update</h2>
	<p><strong>Settings → Advanced</strong> in the web interface shows the installed version and what is new, and lets you switch the release channel between stable and nightly. Whichever method you use, check the result with <code>nolune version</code>.</p>

	<h3 id="web-interface">From the web interface</h3>
	<p>When a newer release is out, an <strong>Update available</strong> pill appears in the web interface. Click it and what happens next depends on who owns the server.</p>
	<p>For a server the <strong>desktop app</strong> started, the app does the update: it downloads the release for the channel the server records, stops the server it owns, replaces <code>~/.nolune/bin/nolune</code>, and starts it again on the same port and workspace. The pill shows <em>Updating…</em> until the new server is answering, then the interface reloads on it. Nothing else to restart. If the new release will not start, the app puts the previous one back and starts that instead, and the pill reports what went wrong.</p>
	<p>For a server from the <a href="/docs/install#one-liner">one-line installer</a>, the server runs <code>~/.nolune/bin/update</code> and, once the binary has changed, exits so it can come back on the new version. Whether it comes back by itself depends on how it was started; see <a href="#restart">Restart the server</a>.</p>
	<p>When the pill cannot do either — a server with no update script that the app does not own, most often a desktop install switched to <strong>Run in background</strong> — it now says so instead of appearing to work. Use <a href="#desktop-install">Install on this computer</a> for that case.</p>

	<h3 id="desktop-install">By installing again from the desktop app</h3>
	<p>Installing over the app's own server is also an upgrade, and it is the way to move a server the app does not currently own. On the connection screen click <strong>Disconnect</strong> so the install section shows, then <strong>Install on this computer</strong>. The app stops the server it owns, downloads the current release over <code>~/.nolune/bin/nolune</code>, starts it again and saves the connection back; the workspace and its auth token are kept. If <strong>Run in background</strong> is on, turn it off in the app's Settings first, because the reinstall refuses while a service holds the port, and turn it back on afterwards.</p>

	<h3 id="terminal">From a terminal on the server</h3>
	<p>The installer left an update script beside the binary:</p>
	<pre><code>~/.nolune/bin/update</code></pre>
	<p>It downloads the current release for your platform, replaces <code>~/.nolune/bin/nolune</code>, and prints <code>already at &lt;version&gt;</code> when there is nothing to do. It follows the channel you installed from; <code>NOLUNE_CHANNEL=nightly ~/.nolune/bin/update</code> switches a stable install to nightly for that run. Rerunning the <a href="/docs/install#one-liner">one-line installer</a> does the same and then starts the server again.</p>

	<h2 id="restart">Restart the server</h2>
	<p>A running process keeps the old binary until it restarts. A desktop-app update or reinstall restarts the server itself, so nothing below applies to it. After the pill on a one-line install, the server exits on its own once the binary has changed; after <code>~/.nolune/bin/update</code> or a binary you replaced yourself, it keeps running the old one. In both cases it has to come back the way it was started:</p>
	<ul>
		<li><strong>Background service</strong> (<code>nolune gateway install</code>): <code>nolune gateway restart</code>. After the pill, launchd or systemd brings the service back on its own.</li>
		<li><strong>Foreground</strong> in a terminal: after the pill the process has already exited, so run <code>nolune gateway</code> again. After the script, stop it with <kbd>Ctrl-C</kbd> first, then run <code>nolune gateway</code>.</li>
		<li><strong>Desktop app</strong>: nothing to do after the pill — the app starts the new server itself. After <code>~/.nolune/bin/update</code> or a binary you replaced by hand, quit and reopen the app; it starts the binary in <code>~/.nolune/bin</code> again. The app's own updater still only updates the app.</li>
	</ul>
	<p>Not sure which applies? <code>nolune gateway status</code> says whether a service is installed and running; if it is not, the server was started in the foreground or by the desktop app.</p>

	<h2 id="profiles">Several profiles on one machine</h2>
	<p>Profiles share the one binary under <code>~/.nolune/bin</code>, so a single update covers all of them. Restart each service afterwards: <code>nolune gateway restart --profile &lt;name&gt;</code> for every named profile, and <code>nolune gateway restart</code> for the default one.</p>

	<h2 id="app-updates">Desktop app updates</h2>
	<p>The desktop app checks for its own updates and installs them like any other app; you can also download the latest build from <a href="https://github.com/triangle-int/nolune/releases" target="_blank" rel="noopener">GitHub Releases</a>. Updating the app does not update a server, and updating the server does not require a new app. Keep both current; either order works.</p>

	<h2 id="rollback">Going back</h2>
	<p>Every release keeps its binaries on GitHub Releases. To pin an older version, download the <code>nolune-server-&lt;target&gt;</code> asset for your platform from that release, replace <code>~/.nolune/bin/nolune</code> with it, make it executable, and restart as above. <a href="/docs/backup">Back up</a> first: a newer server may have written files an older one does not read.</p>
</DocsPage>
