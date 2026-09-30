import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { cliCommand, paths } from '@nolune/core';

/** macOS LaunchAgent: runs as the logged-in user, so commands get their home folder and files. */
const LABEL = 'dev.nolune.gateway';
const plistPath = join(homedir(), 'Library', 'LaunchAgents', `${LABEL}.plist`);
export const logFile = join(paths.logs, 'gateway.log');

/**
 * Run from nolune.app (macos/): its Node sits inside the app, and the app registers this
 * LaunchAgent itself, from its bundle, under the same label.
 */
export function appManaged(execPath = process.execPath): boolean {
	return /\.app\/Contents\/MacOS\/node$/.test(execPath);
}

function target(): string {
	return `gui/${process.getuid?.() ?? 501}/${LABEL}`;
}

function requireMac(): void {
	if (process.platform !== 'darwin') {
		throw new Error(
			'`nolune service` manages a macOS LaunchAgent. On other systems run `nolune start` under your own process manager (e.g. a systemd user unit).'
		);
	}
}

function launchctl(...args: string[]) {
	return spawnSync('launchctl', args, { encoding: 'utf8' });
}

function escapeXml(text: string): string {
	return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

export function renderPlist(): string {
	const args = [...cliCommand(), 'start']
		.map((a) => `\t\t<string>${escapeXml(a)}</string>`)
		.join('\n');
	return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>${LABEL}</string>
	<key>ProgramArguments</key>
	<array>
${args}
	</array>
	<key>EnvironmentVariables</key>
	<dict>
		<key>NOLUNE_HOME</key>
		<string>${escapeXml(paths.home)}</string>
	</dict>
	<key>WorkingDirectory</key>
	<string>${escapeXml(paths.home)}</string>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<true/>
	<key>ThrottleInterval</key>
	<integer>10</integer>
	<key>StandardOutPath</key>
	<string>${escapeXml(logFile)}</string>
	<key>StandardErrorPath</key>
	<string>${escapeXml(logFile)}</string>
</dict>
</plist>
`;
}

export async function installService(): Promise<string> {
	requireMac();
	mkdirSync(dirname(plistPath), { recursive: true });
	mkdirSync(paths.logs, { recursive: true });
	writeFileSync(plistPath, renderPlist());
	launchctl('bootout', target());
	// bootout finishes asynchronously; bootstrap fails with an I/O error until it has.
	for (let attempt = 0; ; attempt++) {
		const result = launchctl('bootstrap', `gui/${process.getuid?.() ?? 501}`, plistPath);
		if (result.status === 0) return plistPath;
		if (attempt >= 10) throw new Error(`launchctl bootstrap failed: ${result.stderr.trim()}`);
		await sleep(300);
	}
}

export function uninstallService(): void {
	requireMac();
	launchctl('bootout', target());
	if (existsSync(plistPath)) unlinkSync(plistPath);
}

export function restartService(): void {
	requireMac();
	const result = launchctl('kickstart', '-k', target());
	if (result.status !== 0) {
		throw new Error('The service is not loaded. Run `nolune service install` first.');
	}
}

export function serviceStatus(): { installed: boolean; loaded: boolean; pid: number | null } {
	requireMac();
	const result = launchctl('print', target());
	const pid = /\bpid = (\d+)/.exec(result.stdout ?? '')?.[1];
	return {
		installed: existsSync(plistPath) || (appManaged() && result.status === 0),
		loaded: result.status === 0,
		pid: pid ? Number(pid) : null
	};
}
