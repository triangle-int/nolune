/*
 * Whether the nolune plan is open to everyone. A module of its own, with nothing else in it, so
 * nolune.dev (site/) reads the same switch as the gateway without taking in the gateway's code.
 */

/**
 * Until it is, the web UI offers the plan only when NOLUNE_PLAN_API_URL points nolune at an API (a
 * local one, in development), the CLI always has it, and nolune.dev shows it only on its previews.
 */
export const NOLUNE_PLAN_OPEN: boolean = false;
