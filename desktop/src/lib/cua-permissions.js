// @ts-check
/**
 * The settings window's computer-use permissions (#20), as pure views over
 * the report the `cua_permissions` command returns: which block the page
 * renders, the status lines, the Accessibility and Screen Recording rows
 * as macOS granted them to the driver's own bundle (`com.trycua.driver`
 * for the pinned driver, never this app), and what a grant did. The grants
 * are attributed to the bundle the driver reported; a driver under another
 * bundle is named as such and nothing is granted through it. Every
 * sentence here describes one-shot capture during an action; nothing
 * promises more.
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
 *       health: "ok" | "degraded" | "failed", bundle: string | null, bundle_mismatch: string | null,
 *       failed_checks: FailedCheck[] }} DriverState
 * @typedef {{ accessibility: Permission, screen_recording: Permission }} DriverPermissions
 * @typedef {{ platform: PlatformState, pinned_version: string, install_command: string, driver_bundle: string,
 *   install: InstallState, driver: DriverState, permissions: DriverPermissions | null, summary: string }} CuaPermissionsReport
 * @typedef {{ permission: string, driver_grant: boolean, opened_settings: boolean }} GrantOutcome
 * @typedef {"unsupported" | "headless" | "absent" | "unreachable" | "incompatible" | "ready"} PageState
 * @typedef {Pick<CuaPermissionsReport, "driver_bundle"> & Partial<Pick<CuaPermissionsReport, "driver">>} BundleSource
 * @typedef {"ok" | "muted" | "error"} Tone
 * @typedef {{ tone: Tone, text: string }} StatusLine
 * @typedef {{ key: "accessibility" | "screen_recording", name: string, desc: string, state: Permission,
 *   stateText: string, canGrant: boolean, hint: string | null }} PermissionRow
 */

/** The bundle macOS attributes the pinned driver's grants to. */
export const DRIVER_BUNDLE = "com.trycua.driver";

/**
 * The bundle the grants on the page belong to: the one the driver's report
 * named, or the pinned driver's when no driver reported (or its report
 * names none).
 * @param {BundleSource} report
 * @returns {string}
 */
export function reportedBundle(report) {
  const driver = report.driver;
  return driver && driver.kind === "reported" && driver.bundle ? driver.bundle : report.driver_bundle;
}

/**
 * Whether the driver that reported holds its grants under a bundle other
 * than the pinned driver's: its rows are that bundle's, and nothing is
 * granted through it.
 * @param {BundleSource} report
 * @returns {boolean}
 */
export function bundleMismatch(report) {
  const driver = report.driver;
  return !!driver && driver.kind === "reported" && driver.bundle != null && driver.bundle !== report.driver_bundle;
}

/** The permission names the page uses, by the report's keys. */
export const PERMISSION_NAMES = Object.freeze({
  accessibility: "Accessibility",
  screen_recording: "Screen recording",
});

/** The state words, by the protocol's permission values. */
const STATE_TEXT = Object.freeze({
  granted: "Granted",
  denied: "Denied",
  prompt_required: "Not asked yet",
  unavailable: "Unavailable",
});

/** What each permission lets the driver do, in one-shot terms. */
const PERMISSION_DESC = Object.freeze({
  accessibility: "Lets the driver click, type and read windows while it carries out an action you asked for.",
  screen_recording:
    "Lets the driver take a one-shot window snapshot as part of an action. Nothing is recorded, kept or sent between actions.",
});

/** The TCC check that backs each row, for the driver's own hint. */
const PERMISSION_CHECK = Object.freeze({
  accessibility: "tcc_accessibility",
  screen_recording: "tcc_screen_recording",
});
/** @type {readonly string[]} */
const PERMISSION_CHECKS = Object.values(PERMISSION_CHECK);

/**
 * Which block the page renders for `report`.
 * @param {CuaPermissionsReport} report
 * @returns {PageState}
 */
export function pageState(report) {
  if (report.platform.kind === "unsupported") return "unsupported";
  if (report.platform.kind === "headless") return "headless";
  switch (report.driver.kind) {
    case "absent":
    case "skipped":
      return "absent";
    case "unreachable":
      return "unreachable";
    case "reported":
      return report.driver.compatible && !bundleMismatch(report) ? "ready" : "incompatible";
  }
}

/**
 * The section's lead: whose grants these are (the bundle the driver
 * reported) and what capture means.
 * @param {BundleSource} report
 * @returns {string}
 */
export function intro(report) {
  return (
    `Computer use runs through the Cua Driver, so macOS grants Accessibility and Screen Recording to the driver's own bundle (${reportedBundle(report)}), not to this app. ` +
    "Every screenshot is a one-shot window snapshot taken during an action you asked for; nothing is captured between actions."
  );
}

/**
 * The status lines above the rows: the platform, the install against the
 * pin, the driver's version against the pin, its health.
 * @param {CuaPermissionsReport} report
 * @returns {StatusLine[]}
 */
export function statusLines(report) {
  const { platform, install, driver } = report;
  if (platform.kind !== "supported") return [{ tone: "muted", text: platform.reason }];
  /** @type {StatusLine[]} */
  const lines = [];
  switch (install.kind) {
    case "pinned":
      lines.push({ tone: "ok", text: `Cua Driver ${install.version} is installed for Nolune (the pinned release).` });
      break;
    case "stale":
      lines.push({
        tone: "error",
        text: `Cua Driver ${install.version} is installed for Nolune, but this build expects ${report.pinned_version}; run \`${report.install_command}\`.`,
      });
      break;
    case "missing":
      lines.push({
        tone: "error",
        text: `Cua Driver ${install.version} was installed for Nolune, but its binary is gone; run \`${report.install_command} --force\`.`,
      });
      break;
    case "unreadable":
      lines.push({ tone: "error", text: `The driver install could not be read (${install.detail}); run \`${report.install_command} --force\`.` });
      break;
    case "none":
      if (driver.kind === "absent" || driver.kind === "skipped") {
        lines.push({ tone: "error", text: `No Cua Driver is installed on this computer; run \`${report.install_command}\`, then refresh.` });
      } else {
        lines.push({ tone: "muted", text: `No driver was installed for Nolune; the one at ${driver.path} is used instead.` });
      }
      break;
  }
  switch (driver.kind) {
    case "absent":
    case "skipped":
      break;
    case "unreachable":
      lines.push({ tone: "error", text: `The driver at ${driver.path} could not report: ${driver.error}` });
      break;
    case "reported": {
      if (driver.compatible) {
        lines.push({ tone: "ok", text: `The driver reports version ${driver.version}, which matches the pin.` });
      } else {
        lines.push({ tone: "error", text: driver.incompatibility ?? `The driver reports version ${driver.version}, not the pinned ${report.pinned_version}; run \`${report.install_command}\`.` });
      }
      if (bundleMismatch(report)) {
        lines.push({
          tone: "error",
          text:
            driver.bundle_mismatch ??
            `The driver at ${driver.path} holds its grants as ${driver.bundle}, not as CuaDriver (${report.driver_bundle}); the rows below are ${driver.bundle}'s and nothing is granted through it. Run \`${report.install_command}\`.`,
        });
      }
      const failed = driver.failed_checks.filter((check) => !PERMISSION_CHECKS.includes(check.name));
      if (driver.health === "ok") {
        lines.push({ tone: "ok", text: "The driver's health check passed." });
      } else {
        const details = failed.map((check) => (check.hint ? `${check.message} ${check.hint}` : check.message)).join(" ");
        lines.push({
          tone: "error",
          text: driver.health === "failed"
            ? `The driver reports a failed check${details ? `: ${details}` : "."}`
            : `The driver's health is degraded${details ? `: ${details}` : "; a permission below is missing."}`,
        });
      }
      break;
    }
  }
  return lines;
}

/**
 * The state word for a permission.
 * @param {Permission} permission
 * @returns {string}
 */
export function stateText(permission) {
  return STATE_TEXT[permission] ?? permission;
}

/**
 * The Accessibility and Screen Recording rows; empty until a driver
 * reported. Nothing is granted through a driver that is not the pinned
 * one, by version or by bundle: the install command comes first.
 * @param {CuaPermissionsReport} report
 * @returns {PermissionRow[]}
 */
export function permissionRows(report) {
  const { driver, permissions } = report;
  if (driver.kind !== "reported" || !permissions) return [];
  const pinnedDriver = driver.compatible && !bundleMismatch(report);
  return /** @type {const} */ (["accessibility", "screen_recording"]).map((key) => {
    const state = permissions[key];
    const check = driver.failed_checks.find((failed) => failed.name === PERMISSION_CHECK[key]);
    return {
      key,
      name: PERMISSION_NAMES[key],
      desc: PERMISSION_DESC[key],
      state,
      stateText: stateText(state),
      canGrant: pinnedDriver && state !== "granted" && state !== "unavailable",
      hint: check?.hint ?? null,
    };
  });
}

/**
 * What to tell the user a grant did, and what to do next.
 * @param {GrantOutcome} outcome
 * @param {CuaPermissionsReport} report
 * @returns {string}
 */
export function grantOutcomeText(outcome, report) {
  const name = PERMISSION_NAMES[/** @type {keyof typeof PERMISSION_NAMES} */ (outcome.permission)] ?? outcome.permission;
  const bundle = reportedBundle(report);
  const parts = [];
  if (outcome.driver_grant) {
    parts.push(`CuaDriver is asking macOS for ${name}; allow the prompt that names CuaDriver (${bundle}).`);
  }
  if (outcome.opened_settings) {
    parts.push(
      outcome.driver_grant
        ? `If no prompt appears, enable CuaDriver under ${name} in the System Settings pane that opened.`
        : `Enable CuaDriver (${bundle}) under ${name} in the System Settings pane that opened.`,
    );
  }
  if (parts.length === 0) parts.push(`Nothing could be started for ${name}; open System Settings and enable CuaDriver (${bundle}) yourself.`);
  parts.push("Then refresh the status here.");
  return parts.join(" ");
}
