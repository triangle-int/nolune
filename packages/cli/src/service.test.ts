import { describe, expect, it } from 'vitest';
import { appManaged } from './service.ts';

describe('appManaged', () => {
	it("is the Node inside nolune.app, which runs the gateway from the app's own LaunchAgent", () => {
		expect(appManaged('/Applications/nolune.app/Contents/MacOS/node')).toBe(true);
		expect(appManaged('/Users/tim/Applications/nolune.app/Contents/MacOS/node')).toBe(true);
	});

	it('is not a Node installed any other way', () => {
		expect(appManaged('/opt/homebrew/bin/node')).toBe(false);
		expect(appManaged('/Users/tim/.nvm/versions/node/v24.11.1/bin/node')).toBe(false);
		expect(appManaged('/Applications/nolune.app/Contents/Resources/node')).toBe(false);
	});
});
