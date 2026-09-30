// @ts-check
import starlight from '@astrojs/starlight';
import { defineConfig } from 'astro/config';
import starlightLlmsTxt from 'starlight-llms-txt';

const GITHUB = 'https://github.com/triangle-int/nolune';

// The docs live at nolune.dev/docs: the landing page (site/vercel.json) forwards /docs here. The build
// goes into dist/docs, so the paths are the same on this project's own address.
export default defineConfig({
	site: 'https://nolune.dev',
	base: '/docs',
	outDir: './dist/docs',
	integrations: [
		starlight({
			title: 'nolune',
			description:
				'How to set up and use nolune, an AI assistant that runs on your computer and helps the whole family.',
			logo: {
				light: './src/assets/wordmark-light.svg',
				dark: './src/assets/wordmark-dark.svg',
				replacesTitle: true
			},
			favicon: '/favicon.svg',
			head: [
				{ tag: 'meta', attrs: { property: 'og:image', content: 'https://nolune.dev/og.png' } },
				{ tag: 'meta', attrs: { name: 'twitter:card', content: 'summary_large_image' } }
			],
			social: [{ icon: 'github', label: 'GitHub', href: GITHUB }],
			editLink: { baseUrl: `${GITHUB}/edit/main/docs/` },
			customCss: [
				'@fontsource-variable/figtree',
				'@fontsource/fira-mono/400.css',
				'@fontsource/fira-mono/500.css',
				'./src/styles/theme.css'
			],
			sidebar: [
				{ label: 'Start here', items: ['index', 'getting-started'] },
				{
					label: 'Guides',
					items: [
						'guides/family',
						'guides/chats',
						'guides/memory',
						'guides/automations',
						'guides/images',
						'guides/skills',
						'guides/auto-mode',
						'guides/models',
						'guides/remote-access'
					]
				},
				{ label: 'Reference', items: ['reference/commands'] }
			],
			// llms.txt and its longer versions, for assistants that read the docs.
			plugins: [starlightLlmsTxt({ projectName: 'nolune' })]
		})
	]
});
