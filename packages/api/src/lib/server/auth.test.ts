import { describe, expect, it } from 'vitest';
import { createAuth, GATEWAY_CLIENT } from './auth.ts';
import type { Email } from './email.ts';
import { testDb } from './test/db.ts';

const BASE = 'http://localhost:5790';

async function setupAuth() {
	const sent: Email[] = [];
	const auth = createAuth({
		db: await testDb(),
		secret: 'a test secret that is long enough to sign with',
		baseURL: BASE,
		sendEmail: async (email) => void sent.push(email)
	});
	const post = (path: string, body: unknown, headers: Record<string, string> = {}) =>
		auth.handler(
			new Request(`${BASE}/api/auth${path}`, {
				method: 'POST',
				headers: { 'content-type': 'application/json', origin: BASE, ...headers },
				body: JSON.stringify(body)
			})
		);
	return { auth, sent, post };
}

/** Signs in with the code the email carried, and returns the session's cookie. */
async function signIn(setup: Awaited<ReturnType<typeof setupAuth>>, email: string) {
	await setup.post('/email-otp/send-verification-otp', { email, type: 'sign-in' });
	const code = /code is (\d{6})/.exec(setup.sent.at(-1)!.text)![1];
	const response = await setup.post('/sign-in/email-otp', { email, otp: code });
	expect(response.status).toBe(200);
	return response.headers
		.getSetCookie()
		.map((cookie) => cookie.split(';')[0])
		.join('; ');
}

describe('accounts', () => {
	it('signs someone in with the code it emails them, making the account the first time', async () => {
		const s = await setupAuth();
		const cookie = await signIn(s, 'anna@example.com');
		expect(s.sent[0]).toMatchObject({ to: 'anna@example.com' });
		expect(s.sent[0].subject).toMatch(/^\d{6} is your nolune code$/);
		const session = await s.auth.api.getSession({ headers: new Headers({ cookie }) });
		expect(session?.user.email).toBe('anna@example.com');
	});

	it('refuses a wrong code', async () => {
		const s = await setupAuth();
		await s.post('/email-otp/send-verification-otp', {
			email: 'anna@example.com',
			type: 'sign-in'
		});
		const response = await s.post('/sign-in/email-otp', {
			email: 'anna@example.com',
			otp: '000000'
		});
		expect(response.status).toBe(400);
	});

	it('links a gateway someone approves, whose token is then theirs', async () => {
		const s = await setupAuth();
		const asked = await s.post('/device/code', { client_id: GATEWAY_CLIENT });
		const { device_code, user_code, verification_uri, verification_uri_complete, interval } =
			await asked.json();
		expect(verification_uri).toBe(`${BASE}/link`);
		expect(verification_uri_complete).toBe(`${BASE}/link?user_code=${user_code}`);
		expect(interval).toBe(5);

		const token = () =>
			s.post('/device/token', {
				grant_type: 'urn:ietf:params:oauth:grant-type:device_code',
				device_code,
				client_id: GATEWAY_CLIENT
			});
		expect(await (await token()).json()).toMatchObject({ error: 'authorization_pending' });

		// The /link page shows the code to whoever's signed in, which makes it theirs to approve.
		const cookie = await signIn(s, 'anna@example.com');
		const shown = await s.auth.api.deviceVerify({
			query: { user_code },
			headers: new Headers({ cookie })
		});
		expect(shown).toMatchObject({ status: 'pending', client_id: GATEWAY_CLIENT });
		const approved = await s.post('/device/approve', { userCode: user_code }, { cookie });
		expect(approved.status).toBe(200);

		// The gateway waits its interval between asks, as RFC 8628 says.
		await new Promise((resolve) => setTimeout(resolve, 5_100));
		const granted = await (await token()).json();
		expect(granted).toMatchObject({ token_type: 'Bearer' });
		const session = await s.auth.api.getSession({
			headers: new Headers({ authorization: `Bearer ${granted.access_token}` })
		});
		expect(session?.user.email).toBe('anna@example.com');
	}, 40_000);

	it('gives device codes only to gateways', async () => {
		const s = await setupAuth();
		const response = await s.post('/device/code', { client_id: 'someone-else' });
		expect(response.status).toBe(400);
	});
});
