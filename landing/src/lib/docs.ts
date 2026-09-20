// The self-hosting docs, in reading order. The index page lists them and the
// docs layout renders them as the section nav; each has a page under
// src/routes/docs/<slug>/, guarded by server/tests/landing_static_site.rs.
export interface DocsSection {
	slug: string;
	title: string;
	summary: string;
}

export const sections: DocsSection[] = [
	{
		slug: 'prerequisites',
		title: 'Prerequisites',
		summary: 'An always-on Mac or Linux computer, a model provider key, and the desktop app for the machines Nolune should use.'
	},
	{
		slug: 'install',
		title: 'Install',
		summary: 'From the desktop app or the one-line installer, then pair a browser. Every step the installers take, and the nolune command behind them.'
	},
	{
		slug: 'upgrade',
		title: 'Upgrade',
		summary: 'Apply an update from Settings or the command line, then restart the server the way it was started.'
	},
	{
		slug: 'backup',
		title: 'Backup',
		summary: 'Everything is a file under ~/.nolune. Copy the directory, or export the companion as one archive.'
	},
	{
		slug: 'uninstall',
		title: 'Uninstall',
		summary: 'Remove the service and binary with or without your data, from the script or with nolune uninstall.'
	},
	{
		slug: 'troubleshooting',
		title: 'Troubleshooting',
		summary: 'Port 26559 in use, pairing codes, nolune not on your PATH, and where the service logs are.'
	}
];

export function section(slug: string): DocsSection {
	const found = sections.find((entry) => entry.slug === slug);
	if (!found) throw new Error(`unknown docs section: ${slug}`);
	return found;
}
