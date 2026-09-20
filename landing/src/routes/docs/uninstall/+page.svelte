<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
</script>

<DocsPage slug="uninstall">
	<p class="docs-lede">Uninstalling removes the background service, the binary, and the log. Whether your data goes with them is your choice, and it is asked, never assumed: without a terminal to ask in, the script keeps your data.</p>

	<h2 id="script">With the uninstall script</h2>
	<pre><code>curl -fsSL https://nolune.dev/uninstall.sh | bash</code></pre>
	<p>Read it first at <a href="/uninstall.sh">nolune.dev/uninstall.sh</a>; it is <code>scripts/uninstall.sh</code> from the repository. In a terminal it asks whether to remove everything or keep your data, then hands over to <code>nolune uninstall</code> and removes the <code>PATH</code> line the installer added to your shell's rc file. To skip the question and keep <code>~/.nolune</code>:</p>
	<pre><code>KEEP_DATA=1 curl -fsSL https://nolune.dev/uninstall.sh | bash</code></pre>
	<p>Set <code>NOLUNE_DIR</code> the same way if you installed somewhere other than <code>~/.nolune</code>.</p>

	<h2 id="command">With the nolune command</h2>
	<p>The script delegates to the binary, which you can run directly:</p>
	<div class="table-scroll">
		<table>
			<thead><tr><th>Command</th><th>What it removes</th></tr></thead>
			<tbody>
				<tr><td><code>nolune uninstall --keep-data</code></td><td>The service, the binary, and the log. <code>~/.nolune</code> (config, memory, chats, uploads) stays where it is.</td></tr>
				<tr><td><code>nolune uninstall</code></td><td>The same, then asks before deleting <code>~/.nolune</code>. Without a terminal it refuses rather than guessing.</td></tr>
				<tr><td><code>nolune uninstall --yes</code></td><td>Everything, including your data, without asking.</td></tr>
				<tr><td><code>nolune gateway uninstall</code></td><td>Only the background service. The binary and your data stay, and <code>nolune gateway</code> still runs the server in the foreground.</td></tr>
			</tbody>
		</table>
	</div>
	<p>Kept data can be removed later with <code>nolune uninstall --yes</code>, or by deleting the directory yourself. If a foreground <code>nolune gateway</code> is still running, the command says so; stop it with <kbd>Ctrl-C</kbd>.</p>

	<h2 id="desktop-app">The desktop app</h2>
	<p>Turning <strong>Run in background</strong> off in the app runs <code>nolune gateway uninstall</code> and lets the app manage the server again. Removing the app itself does not remove a server it installed: run one of the commands above on that computer. <strong>Disconnect</strong> in the app forgets the saved connection and token and clears the app's browser data; the server is not affected.</p>

	<h2 id="profiles">Profiles</h2>
	<p>Add <code>--profile &lt;name&gt;</code> to act on one named profile; its root, <code>~/.nolune-profiles/&lt;name&gt;/</code>, is a sibling of <code>~/.nolune</code>, so removing one profile cannot touch another. All profiles run the one binary under <code>~/.nolune/bin</code>: uninstalling the default profile, even with <code>--keep-data</code>, warns which profiles' services will stop at their next restart. Reinstall and run <code>nolune gateway install --profile &lt;name&gt;</code> to restore them, or remove them with <code>nolune gateway uninstall --profile &lt;name&gt;</code>.</p>

	<h2 id="what-remains">What remains</h2>
	<ul>
		<li>Nothing outside <code>~/.nolune</code> (or your profile roots), the service definition, and the <code>PATH</code> line the installer added. There is no account to close and nothing stored elsewhere.</li>
		<li>Anything you gave your model provider is subject to that provider's retention, not ours; Nolune sent them only what each request needed.</li>
	</ul>
</DocsPage>
