import { defineConfig } from 'vitest/config';

export default defineConfig({
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node',
		setupFiles: ['../core/src/test/setup.ts'],
		expect: { requireAssertions: true }
	}
});
