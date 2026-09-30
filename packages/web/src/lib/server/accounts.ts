import {
	EmailError,
	MAX_NAME_LENGTH,
	MIN_PASSWORD_LENGTH,
	PasswordError,
	UserNameError
} from '@nolune/core';
import type { Messages } from '$lib/i18n';

/**
 * Why an account can't have that name, email or password, in the interface's language; null when
 * the error is about something else. `name`: the one it was given.
 */
export function accountProblem(err: unknown, m: Messages, name: string): string | null {
	const t = m.account;
	if (err instanceof UserNameError) {
		return {
			required: t.nameRequired,
			tooLong: t.nameTooLong(MAX_NAME_LENGTH),
			email: t.nameHasAt,
			taken: t.nameTaken(name.trim())
		}[err.reason];
	}
	if (err instanceof EmailError) return err.reason === 'invalid' ? t.emailInvalid : t.emailTaken;
	if (err instanceof PasswordError) {
		return {
			tooShort: t.passwordTooShort(MIN_PASSWORD_LENGTH),
			tooSimple: t.passwordTooSimple,
			tooRepetitive: t.passwordTooRepetitive
		}[err.reason];
	}
	return null;
}
