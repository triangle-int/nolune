// @ts-check
/**
 * The settings window's computer-use permissions (#20), as pure views over
 * the report the `cua_permissions` command returns: which block the page
 * renders, the status lines, the Accessibility and Screen Recording rows
 * as macOS granted them to the driver's own bundle (`com.trycua.driver`,
 * never this app), and what a grant did. Every sentence here describes
 * one-shot capture during an action; nothing promises more.
 */

/**
 * @typedef {"granted" | "denied" | "prompt_required" | "unavailable"} Permission
 * @typedef {{ kind: "supported", os: string, triple: string }
 *   | { kind: "unsupported" | "headless", os: string, triple: string, reason: string }} PlatformState
 * @typedef {{ kind: "pinned" | "stale" | "missing", version: string, driver: string }
 *   | { kind: "none" } | { kind: "unreadable", detail: string }} InstallState
 * @typedef {{ name: string, message: string, hint: string | null }} FailedCheck
 * @typedef {{ kind: "skipped" } | { kind: "absent" }
 *   | { kind: "unreachable", path: string, error: string }
 *   | { kind: "reported", path: string, version: string, compatible: boolean, incompatibility: string | null,
 *       health: "ok" | "degraded" | "failed", bundle: string | null, failed_checks: FailedCheck[] }} DriverState
 * @typedef {{ accessibility: Permission, screen_recording: Permission }} DriverPermissions
 * @typedef {{ platform: PlatformState, pinned_version: string, install_command: string, driver_bundle: string,
 *   install: InstallState, driver: DriverState, permissions: DriverPermissions | null, summary: string }} CuaPermissionsReport
 * @typedef {{ permission: string, driver_grant: boolean, opened_settings: boolean }} GrantOutcome
 * @typedef {"unsupported" | "headless" | "absent" | "unreachable" | "incompatible" | "ready"} PageState
 * @typedef {"ok" | "muted" | "error"} Tone
 * @typedef {{ tone: Tone, text: string }} StatusLine
 * @typedef {{ key: "accessibility" | "screen_recording", name: string, desc: string, state: Permission,
 *   stateText: string, canGrant: boolean, hint: string | null }} PermissionRow
 */

/** The permission names the page uses, by the report's keys. */
export const PERMISSION_NAMES = Object.freeze({
  accessibility: "Accessibility",
  screen_recording: "Screen recording",
});

/**
 * Which block the page renders for `report`.
 * @param {CuaPermissionsReport} report
 * @returns {PageState}
 */
export function pageState(report) {
  void report;
  throw new Error("pageState: not implemented");
}

/**
 * The section's lead: whose grants these are and what capture means.
 * @param {CuaPermissionsReport} report
 * @returns {string}
 */
export function intro(report) {
  void report;
  throw new Error("intro: not implemented");
}

/**
 * The status lines above the rows: the platform, the install against the
 * pin, the driver's version against the pin, its health.
 * @param {CuaPermissionsReport} report
 * @returns {StatusLine[]}
 */
export function statusLines(report) {
  void report;
  throw new Error("statusLines: not implemented");
}

/**
 * The state word for a permission.
 * @param {Permission} permission
 * @returns {string}
 */
export function stateText(permission) {
  void permission;
  throw new Error("stateText: not implemented");
}

/**
 * The Accessibility and Screen Recording rows; empty until a driver
 * reported.
 * @param {CuaPermissionsReport} report
 * @returns {PermissionRow[]}
 */
export function permissionRows(report) {
  void report;
  throw new Error("permissionRows: not implemented");
}

/**
 * What to tell the user a grant did, and what to do next.
 * @param {GrantOutcome} outcome
 * @param {CuaPermissionsReport} report
 * @returns {string}
 */
export function grantOutcomeText(outcome, report) {
  void outcome;
  void report;
  throw new Error("grantOutcomeText: not implemented");
}
