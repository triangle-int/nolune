/*
 * Whether the nolune plan is open to everyone. A module of its own, with nothing else in it, so
 * nolune.dev (site/) reads the same switch as the gateway without taking in the gateway's code.
 */

/**
 * Open since 0.5.0. Were it closed again, the web UI would offer the plan only when
 * NOLUNE_PLAN_API_URL points nolune at an API (a local one, in development), the CLI would still
 * have it, and nolune.dev would show it only on its previews.
 */
export const NOLUNE_PLAN_OPEN: boolean = true;
