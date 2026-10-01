import {
	bigint,
	boolean,
	index,
	integer,
	pgTable,
	primaryKey,
	text,
	timestamp
} from 'drizzle-orm/pg-core';

const at = (name: string) => timestamp(name, { withTimezone: true, mode: 'date' });
const createdAt = () => at('created_at').defaultNow().notNull();
const updatedAt = () =>
	at('updated_at')
		.defaultNow()
		.$onUpdate(() => new Date())
		.notNull();
/** Money, in millionths of a dollar (limits.ts). */
const micros = (name: string) => bigint(name, { mode: 'number' });

// --- better-auth's tables (keep in step with auth.ts) ---

export const user = pgTable('user', {
	id: text('id').primaryKey(),
	name: text('name').notNull(),
	email: text('email').notNull().unique(),
	emailVerified: boolean('email_verified').default(false).notNull(),
	image: text('image'),
	createdAt: createdAt(),
	updatedAt: updatedAt()
});

export const session = pgTable(
	'session',
	{
		id: text('id').primaryKey(),
		expiresAt: at('expires_at').notNull(),
		token: text('token').notNull().unique(),
		createdAt: createdAt(),
		updatedAt: updatedAt(),
		ipAddress: text('ip_address'),
		userAgent: text('user_agent'),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' })
	},
	(table) => [index('session_user_id_idx').on(table.userId)]
);

export const account = pgTable(
	'account',
	{
		id: text('id').primaryKey(),
		accountId: text('account_id').notNull(),
		providerId: text('provider_id').notNull(),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		accessToken: text('access_token'),
		refreshToken: text('refresh_token'),
		idToken: text('id_token'),
		accessTokenExpiresAt: at('access_token_expires_at'),
		refreshTokenExpiresAt: at('refresh_token_expires_at'),
		scope: text('scope'),
		password: text('password'),
		createdAt: createdAt(),
		updatedAt: updatedAt()
	},
	(table) => [index('account_user_id_idx').on(table.userId)]
);

export const verification = pgTable(
	'verification',
	{
		id: text('id').primaryKey(),
		identifier: text('identifier').notNull(),
		value: text('value').notNull(),
		expiresAt: at('expires_at').notNull(),
		createdAt: createdAt(),
		updatedAt: updatedAt()
	},
	(table) => [index('verification_identifier_idx').on(table.identifier)]
);

/** A gateway asking to be linked (OAuth's device authorization grant): better-auth's plugin. */
export const deviceCode = pgTable('device_code', {
	id: text('id').primaryKey(),
	deviceCode: text('device_code').notNull().unique(),
	userCode: text('user_code').notNull().unique(),
	userId: text('user_id').references(() => user.id, { onDelete: 'cascade' }),
	expiresAt: at('expires_at').notNull(),
	status: text('status').notNull(),
	lastPolledAt: at('last_polled_at'),
	pollingInterval: integer('polling_interval'),
	clientId: text('client_id'),
	scope: text('scope')
});

// --- The plan ---

/**
 * Someone's nolune plan: its limits and how much of them is spent (limits.ts's Account, but its
 * credits). A plan that ended keeps its row, with no limits, for the packs that outlast it.
 */
export const plan = pgTable('plan', {
	userId: text('user_id')
		.primaryKey()
		.references(() => user.id, { onDelete: 'cascade' }),
	windowLimit: micros('window_limit').notNull(),
	weekLimit: micros('week_limit').notNull(),
	startedAt: at('started_at').notNull(),
	renewsAt: at('renews_at'),
	windowOpenedAt: at('window_opened_at'),
	windowSpent: micros('window_spent').default(0).notNull(),
	weekStartedAt: at('week_started_at'),
	weekSpent: micros('week_spent').default(0).notNull(),
	extraPastLimits: boolean('extra_past_limits').default(false).notNull(),
	createdAt: createdAt(),
	updatedAt: updatedAt()
});

/** Credits from one grant: a period's, what carried over, or a pack (limits.ts's Credit). */
export const credit = pgTable(
	'credit',
	{
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		source: text('source').notNull(),
		kind: text('kind', { enum: ['plan', 'extra'] }).notNull(),
		left: micros('left').notNull(),
		expiresAt: at('expires_at'),
		createdAt: createdAt()
	},
	(table) => [primaryKey({ columns: [table.userId, table.source] })]
);
