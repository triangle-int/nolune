import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { homedir, userInfo } from 'node:os';
import { dirname, isAbsolute, join } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { cliCommand, paths } from '@nolune/core';

export const logFile = join(paths.logs, 'gateway.log');

export interface ServiceStatus {
	installed: boolean;
	loaded: boolean;
	pid: number | null;
}

/** What runs the gateway in the background on this system. Both run it as this user. */
interface ServiceManager {
	/** The file `install` writes. */
	file: string;
	render(): string;
	/** Writes the file and (re)starts the gateway. True when it then starts with the computer rather than at login. */
	install(): Promise<boolean>;
	uninstall(): void;
	restart(): void;
	status(): ServiceStatus;
}

/** macOS LaunchAgent: runs as the logged-in user, so commands get their home folder and files. */
const LABEL = 'dev.nolune.gateway';
const plistPath = join(homedir(), 'Library', 'LaunchAgents', `${LABEL}.plist`);

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

function launchctl(...args: string[]) {
	return spawnSync('launchctl', args, { encoding: 'utf8' });
}

function escapeXml(text: string): string {
	return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function renderPlist(): string {
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

const launchAgent: ServiceManager = {
	file: plistPath,
	render: renderPlist,
	async install() {
		mkdirSync(dirname(plistPath), { recursive: true });
		mkdirSync(paths.logs, { recursive: true });
		writeFileSync(plistPath, renderPlist());
		launchctl('bootout', target());
		// bootout finishes asynchronously; bootstrap fails with an I/O error until it has.
		for (let attempt = 0; ; attempt++) {
			const result = launchctl('bootstrap', `gui/${process.getuid?.() ?? 501}`, plistPath);
			if (result.status === 0) return false;
			if (attempt >= 10) throw new Error(`launchctl bootstrap failed: ${result.stderr.trim()}`);
			await sleep(300);
		}
	},
	uninstall() {
		launchctl('bootout', target());
		if (existsSync(plistPath)) unlinkSync(plistPath);
	},
	restart() {
		const result = launchctl('kickstart', '-k', target());
		if (result.status !== 0) {
			throw new Error('The service is not loaded. Run `nolune service install` first.');
		}
	},
	status() {
		const result = launchctl('print', target());
		const pid = /\bpid = (\d+)/.exec(result.stdout ?? '')?.[1];
		return {
			installed: existsSync(plistPath) || (appManaged() && result.status === 0),
			loaded: result.status === 0,
			pid: pid ? Number(pid) : null
		};
	}
};

/**
 * Linux: a systemd user service, so like the LaunchAgent it runs as this user. systemd runs a
 * user's services from their first login to their last logout, or from boot when they linger
 * (`loginctl enable-linger`), which install turns on when logind lets it.
 */
const UNIT = 'nolune.service';
const configHome = process.env.XDG_CONFIG_HOME;
const unitPath = join(
	configHome && isAbsolute(configHome) ? configHome : join(homedir(), '.config'),
	'systemd',
	'user',
	UNIT
);

/** `%` starts a specifier in a unit file (`%h` is the home folder), so a literal one is doubled. */
function unitValue(text: string): string {
	return text.replace(/%/g, '%%');
}

/** One quoted word, as an Environment= assignment is. */
function unitWord(text: string): string {
	return `"${unitValue(text).replace(/[\\"]/g, '\\$&')}"`;
}

/**
 * ExecStart=: the program's path is taken as it is (systemd refuses one with quotes or
 * backslashes), while its arguments expand `$NAME`, so a literal `$` in them is `$$`.
 */
function execLine([program, ...args]: string[]): string {
	return [unitWord(program), ...args.map((a) => unitWord(a).replace(/\$/g, () => '$$'))].join(' ');
}

/** Type=exec and append: need systemd 240 (2018) or later. */
export function renderUnit(): string {
	return `[Unit]
Description=nolune gateway

[Service]
Type=exec
ExecStart=${execLine([...cliCommand(), 'start'])}
Environment=${unitWord(`NOLUNE_HOME=${paths.home}`)}
WorkingDirectory=${unitValue(paths.home)}
Restart=always
RestartSec=10
StandardOutput=append:${unitValue(logFile)}
StandardError=append:${unitValue(logFile)}

[Install]
WantedBy=default.target
`;
}

/** Runs `systemctl --user`, failing when there's no systemd user manager to talk to. */
function systemctl(...args: string[]) {
	const result = spawnSync('systemctl', ['--user', ...args], { encoding: 'utf8' });
	const unreachable = result.error
		? 'there is no systemctl'
		: /Failed to connect to .*bus|not been booted with systemd/.test(result.stderr)
			? result.stderr.trim().split('\n')[0]
			: null;
	if (unreachable) {
		throw new Error(
			`\`nolune service\` runs nolune as a systemd user service, and this user's systemd can't be reached (${unreachable}). Log in as this user directly, not with su or sudo, or run \`nolune start\` under your own process manager.`
		);
	}
	return result;
}

function systemctlOrFail(...args: string[]): void {
	const result = systemctl(...args);
	if (result.status !== 0) {
		throw new Error(`systemctl --user ${args.join(' ')} failed: ${result.stderr.trim()}`);
	}
}

function lingering(): boolean {
	return existsSync(join('/var/lib/systemd/linger', userInfo().username));
}

const systemdUnit: ServiceManager = {
	file: unitPath,
	render: renderUnit,
	async install() {
		// Fails here, before anything is written, when there's no user manager.
		systemctl('show-environment');
		mkdirSync(dirname(unitPath), { recursive: true });
		mkdirSync(paths.logs, { recursive: true });
		writeFileSync(unitPath, renderUnit());
		// Without a reload, restart would run the unit as it was before.
		systemctlOrFail('daemon-reload');
		systemctlOrFail('enable', UNIT);
		systemctlOrFail('restart', UNIT);
		if (lingering()) return true;
		return spawnSync('loginctl', ['enable-linger', '--no-ask-password']).status === 0;
	},
	uninstall() {
		systemctl('disable', '--now', UNIT);
		if (existsSync(unitPath)) unlinkSync(unitPath);
		systemctl('daemon-reload');
		systemctl('reset-failed', UNIT);
	},
	restart() {
		const result = systemctl('restart', UNIT);
		if (result.status === 0) return;
		if (!existsSync(unitPath)) {
			throw new Error('The service is not installed. Run `nolune service install` first.');
		}
		throw new Error(`systemctl --user restart ${UNIT} failed: ${result.stderr.trim()}`);
	},
	status() {
		const { stdout } = systemctl('show', UNIT, '--property=ActiveState,MainPID');
		const state = /^ActiveState=(.*)$/m.exec(stdout)?.[1];
		const pid = Number(/^MainPID=(\d+)$/m.exec(stdout)?.[1] ?? 0);
		return {
			installed: existsSync(unitPath),
			// "activating" while it waits to be started again after exiting, "failed" if it gave up.
			loaded: state !== undefined && state !== 'inactive',
			pid: state === 'active' && pid > 0 ? pid : null
		};
	}
};

function manager(): ServiceManager {
	if (process.platform === 'darwin') return launchAgent;
	if (process.platform === 'linux') return systemdUnit;
	throw new Error(
		'`nolune service` runs nolune in the background on macOS (a LaunchAgent) and Linux (a systemd user service). Here, run `nolune start` under your own process manager.'
	);
}

/** The LaunchAgent's plist or the systemd unit that `install` writes here. */
export function renderServiceFile(): string {
	return manager().render();
}

export async function installService(): Promise<{ file: string; atBoot: boolean }> {
	const service = manager();
	return { file: service.file, atBoot: await service.install() };
}

export function uninstallService(): void {
	manager().uninstall();
}

export function restartService(): void {
	manager().restart();
}

export function serviceStatus(): ServiceStatus {
	return manager().status();
}
