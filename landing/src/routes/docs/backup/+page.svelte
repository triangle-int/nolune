<script lang="ts">
	import DocsPage from '$lib/components/DocsPage.svelte';
</script>

<DocsPage slug="backup">
	<p class="docs-lede">Everything the server knows is a file under <code>~/.nolune</code>. There is no database to dump: a backup is a copy of that directory, and a restore is putting it back. For the companion alone, the server can also export one archive.</p>

	<h2 id="layout">What is where</h2>
	<pre><code>~/.nolune/
├── config.toml               port, auth token, provider keys
├── browser_sessions.json     paired browsers (hashes only)
├── federation/               the companion's signing identity and private key
├── skills/                   installed skills
├── vectors/                  derived search index, rebuilt automatically
└── instances/
    └── companion/            the one companion this server hosts
        ├── soul.md           personality
        ├── memory/           long-term memory, one Markdown file per topic
        ├── chats/            conversation history
        ├── uploads/          files you shared
        ├── drops/            things it made on its own
        └── …                 mood, schedules, settings, skills</code></pre>
	<div class="callout">
		<p><code>config.toml</code> holds the auth token and your provider API keys, and <code>federation/signing_key.json</code> is a private key. Treat a backup of the directory as you would treat those secrets.</p>
	</div>

	<h2 id="copy">Back up the whole server</h2>
	<p>Stop the server so nothing is written while you copy, then archive the directory:</p>
	<pre><code>nolune gateway stop          # or Ctrl-C in the terminal running it
tar czf nolune-backup.tgz -C ~ .nolune
nolune gateway start</code></pre>
	<p>Keep the archive wherever you keep other backups. The <code>vectors/</code> index is derived from <code>memory/</code> and can be left out; the server rebuilds it in the background on the next start.</p>
	<h3 id="restore">Restore, or move to another computer</h3>
	<ol class="steps">
		<li>Stop the server on the target machine, or <a href="/docs/install">install</a> it there first; <code>nolune onboard</code> is safe to rerun and keeps an existing <code>config.toml</code>.</li>
		<li>Replace <code>~/.nolune</code> with the copy: <code>tar xzf nolune-backup.tgz -C ~</code>.</li>
		<li>Start the server again. Because <code>federation/</code> came along, the companion keeps its identity; nothing in it names the old host, port, or path.</li>
		<li>Browsers that reach the server at a new address pair again with <code>nolune pair</code>; the desktop app only needs the new address.</li>
	</ol>

	<h2 id="export">Export the companion as one archive</h2>
	<p>Under <strong>Settings → Data</strong>, <strong>Export</strong> downloads <code>companion.tar.gz</code>: the companion directory verbatim, rooted at <code>companion/</code>, with a small manifest (<code>companion/companion.json</code>) that names the format version. The same file comes from the API with the auth token from <code>config.toml</code>:</p>
	<pre><code>curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:26559/api/instances/companion/export -o companion.tar.gz</code></pre>
	<p>The archive is built in-process, contains only regular files and directories, and is withheld rather than truncated if the export fails part-way, so a complete download always restores. It does <em>not</em> contain <code>config.toml</code>, the paired browsers, the federation identity, or globally installed skills: it carries the companion's memory and settings, not the server's. For a full backup, copy the directory as above.</p>
	<p>Restoring the archive with <strong>Import</strong> under Settings → Data is still being finished (<a href="https://github.com/triangle-int/nolune/issues/74" target="_blank" rel="noopener">issue #74</a>); until it lands, the server answers that import is unavailable. You can always restore by hand: stop the server, replace <code>~/.nolune/instances/companion/</code> with the <code>companion/</code> directory from the archive, and start again. The search index rebuilds itself.</p>

	<h2 id="memory">Memory is plain text</h2>
	<p><code>memory/</code> is ordinary Markdown organised by topic, and it is the source of truth for what Nolune remembers. You can read it, edit it, keep it in version control, or sync it between backups like any other folder; the Memory page in the web interface edits the same files. Delete <code>vectors/</code> at any time to force the search index to rebuild from it.</p>

	<h2 id="profiles">Profiles</h2>
	<p>A named profile keeps the same layout under its own root, <code>~/.nolune-profiles/&lt;name&gt;/</code>, beside <code>~/.nolune</code> rather than inside it. Back each one up separately; the export route and Settings of each profile's server cover only that profile's companion.</p>
</DocsPage>
