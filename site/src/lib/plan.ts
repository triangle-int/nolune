import { NOLUNE_PLAN_OPEN } from '@nolune/core/nolune-plan-open';

/**
 * Whether the page shows the nolune plan: once it's open to everyone, and before that on previews
 * and the dev server, so it can be looked at.
 */
export const SHOW_PLAN = NOLUNE_PLAN_OPEN || __PREVIEW__;

/** The account page on nolune's API, where people sign in and subscribe. */
export const PLAN_ACCOUNT_URL = 'https://api.nolune.dev/';
