import { cliCommand, paths } from '@nolune/core';
import { describe, expect, it, vi } from 'vitest';
import { appManaged, logFile, renderUnit } from './service.ts';

vi.mock('@nolune/core', async (importOriginal) => {
	const core = await importOriginal<typeof import('@nolune/core')>();
	return { ...core, cliCommand: vi.fn(core.cliCommand) };
});

function setting(unit: string, key: string): string | undefined {
	return new RegExp(`^${key}=(.*)$`, 'm').exec(unit)?.[1];
}

describe('the systemd unit', () => {
	it('runs `nolune start` with this nolune home, again whenever it stops, into the gateway log', () => {
		vi.mocked(cliCommand).mockReturnValueOnce(['/usr/bin/node', '/opt/nolune/dist/cli.js']);
		const unit = renderUnit();
		expect(setting(unit, 'ExecStart')).toBe('"/usr/bin/node" "/opt/nolune/dist/cli.js" "start"');
		expect(setting(unit, 'Environment')).toBe(`"NOLUNE_HOME=${paths.home}"`);
		expect(setting(unit, 'WorkingDirectory')).toBe(paths.home);
		expect(setting(unit, 'Restart')).toBe('always');
		expect(setting(unit, 'StandardOutput')).toBe(`append:${logFile}`);
		expect(setting(unit, 'StandardError')).toBe(`append:${logFile}`);
		expect(setting(unit, 'WantedBy')).toBe('default.target');
	});

	it('keeps spaces, quotes, backslashes, % and $ in paths as they are', () => {
		vi.mocked(cliCommand).mockReturnValueOnce([
			'/home/anna/100% $HOME/node',
			'/srv/100% "nolune"/a\\b$HOME.js'
		]);
		// Only the arguments expand $NAME, not the program's path.
		expect(setting(renderUnit(), 'ExecStart')).toBe(
			'"/home/anna/100%% $HOME/node" "/srv/100%% \\"nolune\\"/a\\\\b$$HOME.js" "start"'
		);
	});
});

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
