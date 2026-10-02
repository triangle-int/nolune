import { betterAuth, type BetterAuthPlugin } from 'better-auth';
import { drizzleAdapter } from 'better-auth/adapters/drizzle';
import { bearer, deviceAuthorization, emailOTP } from 'better-auth/plugins';
import { schema, type Db } from './db.ts';
import { signInCodeEmail, type SendEmail } from './email.ts';

const DAY = 24 * 60 * 60;

/** The client id a gateway asks for a device code with. */
export const GATEWAY_CLIENT = 'nolune';

export interface AuthOptions {
	db: Db;
	secret: string;
	/** Where the service is (`https://api.nolune.dev`). */
	baseURL: string;
	sendEmail: SendEmail;
	/** SvelteKit's cookies, in the service; none in tests. */
	plugins?: BetterAuthPlugin[];
	/**
	 * Whether an address may have an account: until the plan is open, only the ones let in
	 * (`ALLOWED_EMAILS`). Others are sent no code, and no account is made for them however it's
	 * asked for. Anyone, when not given.
	 */
	mayJoin?: (email: string) => boolean;
}

/**
 * Accounts on nolune's API. People sign in with a code sent to their email, which makes the
 * account the first time: there's no password to keep. A gateway is linked with a device code
 * (`/link`), and the session that gives it is its bearer token for `/v1`.
 */
export function createAuth({
	db,
	secret,
	baseURL,
	sendEmail,
	plugins = [],
	mayJoin = () => true
}: AuthOptions) {
	return betterAuth({
		baseURL,
		secret,
		database: drizzleAdapter(db, { provider: 'pg', schema }),
		// A gateway's session is renewed as it's used, so one that's running stays signed in.
		session: { expiresIn: 90 * DAY, updateAge: DAY },
		databaseHooks: {
			user: { create: { before: async (user) => (mayJoin(user.email) ? undefined : false) } }
		},
		rateLimit: {
			customRules: {
				'/email-otp/send-verification-otp': { window: 60, max: 3 },
				'/sign-in/email-otp': { window: 60, max: 5 },
				'/device/approve': { window: 60, max: 10 }
			}
		},
		plugins: [
			emailOTP({
				otpLength: 6,
				expiresIn: 5 * 60,
				allowedAttempts: 5,
				async sendVerificationOTP({ email, otp }) {
					if (!mayJoin(email)) return;
					await sendEmail(signInCodeEmail(email, otp));
				}
			}),
			deviceAuthorization({
				verificationUri: '/link',
				expiresIn: '15m',
				interval: '5s',
				validateClient: (clientId) => clientId === GATEWAY_CLIENT
			}),
			bearer(),
			...plugins
		]
	});
}

/** Who may sign in: the addresses listed, or anyone when there's no list. */
export function allowed(list: string | undefined): (email: string) => boolean {
	const addresses = new Set(
		(list ?? '')
			.split(/[\s,]+/)
			.map((address) => address.toLowerCase())
			.filter(Boolean)
	);
	if (!addresses.size) return () => true;
	return (email) => addresses.has(email.trim().toLowerCase());
}

export type Auth = ReturnType<typeof createAuth>;
export type AuthUser = Auth['$Infer']['Session']['user'];
export type AuthSession = Auth['$Infer']['Session']['session'];
