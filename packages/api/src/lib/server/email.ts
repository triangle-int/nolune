/*
 * Email, which only signing in sends so far: through Resend's API with plain fetch, as nolune
 * calls OpenAI's image API, rather than its SDK. Without a key (in development) the message is
 * printed instead, so a sign-in code can be read from the terminal.
 */

export interface Email {
	to: string;
	subject: string;
	text: string;
	html: string;
}

export type SendEmail = (email: Email) => Promise<void>;

export function emailSender(options: { apiKey?: string; from: string }): SendEmail {
	const { apiKey, from } = options;
	if (!apiKey) {
		return async ({ to, subject, text }) => {
			console.log(`[nolune api] email to ${to}: ${subject}\n${text}`);
		};
	}
	return async (email) => {
		const response = await fetch('https://api.resend.com/emails', {
			method: 'POST',
			headers: { authorization: `Bearer ${apiKey}`, 'content-type': 'application/json' },
			body: JSON.stringify({ from, ...email }),
			signal: AbortSignal.timeout(15_000)
		});
		if (!response.ok) {
			throw new Error(`Resend refused the email (${response.status}): ${await response.text()}`);
		}
	};
}

/** The code that signs someone in, as an email. */
export function signInCodeEmail(to: string, code: string): Email {
	const text = [
		`Your nolune sign-in code is ${code}.`,
		'',
		'Enter it on the page that asked for it. It works for 5 minutes.',
		"If you didn't ask for it, you can ignore this email: nobody can sign in without the code."
	].join('\n');
	const html = `<p>Your nolune sign-in code is</p>
<p style="font-size:28px;font-weight:600;letter-spacing:4px;font-family:ui-monospace,monospace">${code}</p>
<p>Enter it on the page that asked for it. It works for 5 minutes.</p>
<p style="color:#666">If you didn't ask for it, you can ignore this email: nobody can sign in without the code.</p>`;
	return { to, subject: `${code} is your nolune code`, text, html };
}
