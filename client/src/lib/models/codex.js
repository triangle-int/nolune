/**
 * The Codex login (#27), as the Settings tile and onboarding read it from
 * `GET /api/config/codex/status`. A ChatGPT login through the local
 * `codex` binary is not an API key: the status carries whether the pinned
 * binary is there, who codex is logged in as (an email and a plan, never a
 * credential) and what a person needs to finish a login. This module is
 * pure so every state's copy is testable.
 *
 * @typedef {'ready' | 'not_installed' | 'incompatible' | 'unusable'} BinaryState
 * @typedef {{ state: BinaryState, pinned_version: string, path?: string, version?: string, message?: string }} CodexBinary
 * @typedef {{ kind: string, email?: string, plan?: string }} CodexAccount
 * @typedef {'browser' | 'device_code'} LoginMethod
 * @typedef {{ id: string, method: LoginMethod, state: 'pending' | 'completed' | 'failed', auth_url?: string, verification_url?: string, user_code?: string, error?: string }} CodexLogin
 * @typedef {{ binary: CodexBinary, installed: boolean, compatible: boolean, logged_in: boolean, account: CodexAccount | null, login: CodexLogin | null, error?: string }} CodexStatus
 * @typedef {'loading' | 'not_installed' | 'incompatible' | 'unusable' | 'unavailable' | 'logged_out' | 'pending' | 'failed' | 'logged_in'} TileState
 * @typedef {{ url: string, code: string | null, note: string }} LoginInstructions
 * @typedef {{ state: TileState, headline: string, detail: string | null, binary: string, canLogin: boolean, canLogout: boolean, instructions: LoginInstructions | null }} CodexView
 * @typedef {{ ok: false, error: string, message: string, status: number }} CodexFailure
 */

/** How often a pending login is polled for its outcome. */
export const CODEX_LOGIN_POLL_MS = 2000;

/** The npm package the pinned release installs from. */
const CODEX_PACKAGE = "@openai/codex";

/** The server's sentence, ended with a full stop so another can follow it. @param {string | undefined | null} text */
function sentence(text) {
	const trimmed = (text ?? "").trim();
	if (!trimmed) return "";
	return /[.!?]$/.test(trimmed) ? `${trimmed} ` : `${trimmed}. `;
}

/** @param {string | undefined | null} plan */
function planLabel(plan) {
	if (!plan) return "";
	return plan[0].toUpperCase() + plan.slice(1);
}

/**
 * Who codex is logged in as: the email with the plan when there is one, the
 * kind when the app-server gave no email.
 * @param {CodexAccount | null | undefined} account
 */
export function codexAccountLabel(account) {
	if (!account) return "";
	if (account.email) return account.plan ? `${account.email} · ${planLabel(account.plan)}` : account.email;
	if (account.kind === "chatgpt") return "ChatGPT";
	if (account.kind === "api_key") return "an API key codex holds";
	return account.kind;
}

/**
 * What a person needs to finish a pending login: the device-code flow is
 * a URL to open anywhere and a code to type there; the managed browser
 * flow is a URL that must be opened on the machine the server runs on,
 * because codex takes the callback on its own localhost port.
 * @param {CodexLogin | null | undefined} login
 * @returns {LoginInstructions | null}
 */
export function loginInstructions(login) {
	if (!login || login.state !== "pending") return null;
	if (login.method === "device_code" && login.verification_url) {
		return {
			url: login.verification_url,
			code: login.user_code ?? null,
			note: "Open the link on any device, sign in to ChatGPT, and enter the code there.",
		};
	}
	if (login.auth_url) {
		return {
			url: login.auth_url,
			code: null,
			note: "Open the link in a browser on the machine this server runs on; codex finishes the login there.",
		};
	}
	return null;
}

/**
 * The binary line under the tile's name: the release and where it is, or
 * the release Nolune supports when it is not there.
 * @param {CodexBinary} binary
 */
function binaryLine(binary) {
	if (binary.state === "ready") return `codex ${binary.version ?? binary.pinned_version} · ${binary.path ?? "on PATH"}`;
	return `Nolune supports codex ${binary.pinned_version}`;
}

/**
 * One state per status, with a headline, what to do about it, and which
 * actions apply. A status that has not loaded yet shows nothing to act on.
 * @param {CodexStatus | null | undefined} status
 * @returns {CodexView}
 */
export function codexView(status) {
	if (!status) {
		return { state: "loading", headline: "Checking codex…", detail: null, binary: "", canLogin: false, canLogout: false, instructions: null };
	}
	const binary = binaryLine(status.binary);
	const pin = status.binary.pinned_version;
	const none = { canLogin: false, canLogout: false, instructions: null, binary };
	switch (status.binary.state) {
		case "not_installed":
			return {
				...none,
				state: "not_installed",
				headline: "Codex is not installed on this server.",
				detail: `${sentence(status.binary.message)}Install codex ${pin} (npm install -g ${CODEX_PACKAGE}@${pin}) or set NOLUNE_CODEX_BIN to the binary, then reload this page.`,
			};
		case "incompatible":
			return {
				...none,
				state: "incompatible",
				headline: `Codex ${status.binary.version ?? "of another release"} is installed, but Nolune supports codex ${pin} only.`,
				detail: `${sentence(status.binary.message)}Install codex ${pin} (npm install -g ${CODEX_PACKAGE}@${pin}) or set NOLUNE_CODEX_BIN to that release, then reload this page.`,
			};
		case "unusable":
			return {
				...none,
				state: "unusable",
				headline: "The codex binary cannot run here.",
				detail: `${sentence(status.binary.message)}Reinstall codex ${pin} or set NOLUNE_CODEX_BIN to a working binary, then reload this page.`,
			};
		default:
			break;
	}
	if (status.error) {
		return {
			...none,
			state: "unavailable",
			headline: "Codex could not answer.",
			detail: `${sentence(status.error)}Logging in starts it again.`,
			canLogin: true,
		};
	}
	if (status.logged_in) {
		return {
			...none,
			state: "logged_in",
			headline: `Logged in as ${codexAccountLabel(status.account)}.`,
			detail: "Codex presets run on this login; no API key is involved.",
			canLogout: true,
		};
	}
	const login = status.login;
	if (login?.state === "pending") {
		return {
			...none,
			state: "pending",
			headline: login.method === "device_code" ? "Finish the login with the code below." : "Finish the login in the browser.",
			detail: null,
			canLogout: true,
			instructions: loginInstructions(login),
		};
	}
	if (login?.state === "failed") {
		return {
			...none,
			state: "failed",
			headline: "The login failed.",
			detail: login.error ?? "Codex did not say why.",
			canLogin: true,
		};
	}
	return {
		...none,
		state: "logged_out",
		headline: "Not logged in.",
		detail: "Log in with ChatGPT to run Codex presets; the login stays with codex on this server.",
		canLogin: true,
	};
}

/**
 * Whether a Codex preset can answer right now: the pinned binary, an
 * app-server that answered, and a login.
 * @param {CodexStatus | null | undefined} status
 */
export function codexReady(status) {
	return !!status && status.compatible && status.logged_in && !status.error;
}

/**
 * Where the login that was started stands, from a later status: still
 * pending, completed, failed, or replaced by another login (or cleared by
 * a logout). A login codex holds is completed whatever the record says:
 * the server ends a record only on the app-server's event for that id, so
 * a `codex login` run on the server leaves it pending while the status
 * already reports the account. An app-server that went away while it was
 * pending ends it.
 * @param {CodexStatus | null | undefined} status
 * @param {string} loginId
 * @returns {'pending' | 'completed' | 'failed' | 'replaced'}
 */
export function loginProgress(status, loginId) {
	if (status?.logged_in) return "completed";
	const login = status?.login;
	if (!login || login.id !== loginId) return "replaced";
	if (login.state === "pending") return status?.error ? "failed" : "pending";
	return login.state;
}

/**
 * Whether a pending login can be swapped for a device code: the managed
 * browser flow needs a browser on the machine the server runs on, which a
 * person on another device does not have; a device-code login already is
 * one, and a login that is over has nothing to swap.
 * @param {CodexLogin | null | undefined} login
 */
export function offersDeviceCode(login) {
	return !!login && login.state === "pending" && login.method === "browser";
}

/**
 * One sentence for a login or logout the server refused, keyed on its
 * typed `error`; the server's message names the path and both versions
 * already, so the copy adds what to do next.
 * @param {CodexFailure} outcome
 */
export function codexErrorCopy(outcome) {
	const said = outcome.message?.trim() || "Codex could not answer.";
	switch (outcome.error) {
		case "codex_not_installed":
		case "codex_incompatible":
		case "codex_unusable":
			return `${said}. Fix the codex install on this server, then try again.`;
		case "codex_unavailable":
			return `${said}. Try again in a moment.`;
		case "codex_refused":
			return `Codex refused: ${said}`;
		default:
			return said;
	}
}
